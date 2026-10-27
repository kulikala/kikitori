"""kikitori の文字起こしの精度と速さを、音源ごとに測る。

使い方（jiwer と pyarrow が必要。uv なら `uv run --with jiwer --with pyarrow benchmarks/bench.py …`）:
  bench.py fetch SOURCE...              公開の音源を取得・生成する（fleurs・rz・long・synth・hour）
  bench.py run SOURCE|DIR [options]     文字起こしして、集計（CER・時間・最大メモリ）だけを表示する
  bench.py score SOURCE|DIR RESULT      出力のファイルを採点し直す（ほかの道具の出力も同じ形なら採点できる）
  bench.py list                         公開の音源と、取得済みかを表示する

音源は、manifest.json を置いたディレクトリ 1 つで表す。公開の音源は名前で、
リポジトリの外に置いた音源（手元だけで使う録音）はディレクトリのパスで渡す。
リポジトリの中のディレクトリは、公開の音源の置き場（既定は bench/）のほかは受け付けない。

manifest.json の形:
  {"kind": "clips",     "edge_free": false, "items": [{"id": "...", "audio": "相対パス", "text": "正解"}]}
  {"kind": "recording", "audio": "相対パス", "text": "正解"}
正解は "text" の代わりに "text_file"（manifest からの相対パス）でも書ける。正解の無い音声は両方を省く。
CER は正解のある音声だけで数える。出力の字数は、正解の無い音声で版どうしを比べるのに使う。

結果（各音声の出力）は、公開の音源なら置き場の results/ に、それ以外は音源のディレクトリの
results/ に書く。表示には、音声の名前も本文も出さない。
"""
import argparse
import json
import os
import re
import resource
import shutil
import subprocess
import sys
import tarfile
import time
import unicodedata
import urllib.request
import wave
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DATA = Path(os.environ.get("KIKITORI_BENCH_DATA") or REPO / "bench").resolve()
HF = "https://huggingface.co/datasets"
FLEURS = f"{HF}/google/fleurs/resolve/main/data/ja_jp"
RZ = f"{HF}/japanese-asr/ja_asr.reazonspeech_test/resolve/main/data/test-00000-of-00002.parquet"


# ---------- 採点 ----------

_D = {c: i for i, c in enumerate("〇一二三四五六七八九")}
_U = {"十": 10, "百": 100, "千": 1000}
_RUN = re.compile("[〇一二三四五六七八九十百千万]+")


def _kanji_run_to_int(run):
    """漢数字の連なりを算用数字にする。両側に同じ変換をかけるので、系統による表記の差が採点に出ない。"""
    if not any(c in "十百千万" for c in run):
        return "".join(str(_D[c]) for c in run)
    total, section, cur = 0, 0, 0
    for c in run:
        if c in _D:
            cur = _D[c]
        elif c in _U:
            section += (cur or 1) * _U[c]
            cur = 0
        else:
            total += (section + cur or 1) * 10000
            section = cur = 0
    return str(total + section + cur)


def norm(s):
    s = unicodedata.normalize("NFKC", s)
    s = _RUN.sub(lambda m: _kanji_run_to_int(m.group(0)), s)
    return re.sub(r"[\s、。，．,.!?！？「」『』・\-ー…~〜'\"“”()（）:：;；]", "", s)


def cer(refs, hyps, edge_free=False):
    """CER。edge_free なら、正解の前後にはみ出した挿入を数えない（切り出し音声にラベル外の発話が残るデータ向け）。"""
    import jiwer
    if not edge_free:
        return jiwer.cer(refs, hyps)
    err = tot = 0
    for r, h in zip(refs, hyps):
        ops = list(jiwer.process_characters(r, h).alignments[0])
        keep = [i for i, c in enumerate(ops) if c.type != "insert"]
        lo, hi = (keep[0], keep[-1]) if keep else (0, -1)
        for i, c in enumerate(ops):
            if c.type == "equal" or (c.type == "insert" and not lo < i < hi):
                continue
            if c.type == "insert":
                err += c.hyp_end_idx - c.hyp_start_idx
            elif c.type == "delete":
                err += c.ref_end_idx - c.ref_start_idx
            else:
                err += max(c.ref_end_idx - c.ref_start_idx, c.hyp_end_idx - c.hyp_start_idx)
        tot += len(r)
    return err / max(tot, 1)


# ---------- 音源 ----------

def is_inside(path, root):
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except ValueError:
        return False


def resolve_source(arg):
    """名前なら公開の音源の置き場、パスならそのディレクトリ。リポジトリの中は置き場だけを許す。"""
    p = Path(arg).expanduser()
    if not p.exists() and "/" not in arg:
        p = DATA / arg
    p = p.resolve()
    if is_inside(p, REPO) and not is_inside(p, DATA):
        sys.exit(f"bench: リポジトリの中の音源は、公開の音源の置き場（{DATA}）にあるものだけを使えます")
    if not (p / "manifest.json").exists():
        sys.exit(f"bench: manifest.json がありません: {p}（公開の音源なら `bench.py fetch {arg}`）")
    public = is_inside(p, DATA)
    return p, public


def duration(path):
    if path.suffix.lower() == ".wav":
        try:
            with wave.open(str(path)) as w:
                return w.getnframes() / w.getframerate()
        except wave.Error:  # 浮動小数の wav などは ffprobe に任せる
            pass
    out = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path)],
                         capture_output=True, text=True, check=True).stdout
    return float(out)


# ---------- 取得・生成 ----------

def download(url, dest):
    if dest.exists():
        return
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".part")
    print(f"取得: {url}")
    with urllib.request.urlopen(url) as r, open(tmp, "wb") as f:
        shutil.copyfileobj(r, f)
    tmp.rename(dest)


def write_manifest(d, manifest):
    (d / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=1), encoding="utf-8")


def fleurs_rows():
    """FLEURS 日本語の test の行（id・ファイル名・正解）。音声は test/ に展開する。"""
    d = DATA / "fleurs"
    download(f"{FLEURS}/test.tsv", d / "test.tsv")
    if not (d / "test").is_dir():
        download(f"{FLEURS}/audio/test.tar.gz", d / "test.tar.gz")
        with tarfile.open(d / "test.tar.gz") as t:
            t.extractall(d, filter="data")
    return [l.rstrip("\n").split("\t") for l in open(d / "test.tsv", encoding="utf-8")]


def fetch_fleurs():
    rows = fleurs_rows()[:100]
    write_manifest(DATA / "fleurs", {"kind": "clips", "items": [
        {"id": r[1], "audio": f"test/{r[1]}", "text": r[2]} for r in rows]})


def fetch_rz():
    """ReazonSpeech の評価用データの最初の parquet から、26 行おきに 100 行（行数 // 100 の間隔）。"""
    d = DATA / "rz"
    download(RZ, d / "t0.parquet")
    (d / "test").mkdir(exist_ok=True)
    import pyarrow.parquet as pq
    table = pq.read_table(d / "t0.parquet").to_pylist()
    step = len(table) // 100
    rows = []
    for i in range(100):
        x, name = table[i * step], f"rz{i:03d}"
        flac = d / "test" / f"{name}.flac"
        if not (d / "test" / f"{name}.wav").exists() and not flac.exists():
            flac.write_bytes(x["audio"]["bytes"])
        rows.append((name, x["transcription"]))
    items = []
    for name, text in rows:
        # 以前の置き場が変換済みの wav を持っていれば、それを使う
        audio = f"test/{name}.wav" if (d / "test" / f"{name}.wav").exists() else f"test/{name}.flac"
        items.append({"id": f"{name}.wav", "audio": audio, "text": text})  # 以前の出力と同じ ID
    write_manifest(d, {"kind": "clips", "edge_free": True, "items": items})


def concat(files, gaps, dest):
    """16 kHz モノラルの wav をつなぐ。gaps[i] 秒の無音を files[i] のあとに入れる。"""
    with wave.open(str(dest), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        for f, g in zip(files, gaps):
            w.writeframes(pcm16k(f))
            w.writeframes(b"\0\0" * int(16000 * g))


def pcm16k(path):
    return subprocess.run(["ffmpeg", "-v", "error", "-i", str(path), "-ac", "1", "-ar", "16000", "-f", "s16le", "-"],
                          capture_output=True, check=True).stdout


def fetch_long():
    """FLEURS の先頭 50 本を、間を空けずにつないだ 1 本（約 11 分）。"""
    rows = fleurs_rows()[:50]
    d = DATA / "long"
    d.mkdir(parents=True, exist_ok=True)
    concat([DATA / "fleurs" / "test" / r[1] for r in rows], [0.0] * len(rows), d / "long.wav")
    write_manifest(d, {"kind": "recording", "audio": "long.wav", "text": "".join(r[2] for r in rows)})


def synth_meeting():
    d = DATA / "synth"
    if not (d / "meeting.wav").exists():
        if sys.platform != "darwin":
            sys.exit("bench: synth は macOS の say で作ります")
        subprocess.run([sys.executable, "-I", str(REPO / "scripts" / "make_synth_meeting.py"), str(d)], check=True)
    return d, json.load(open(d / "truth.json", encoding="utf-8"))


def fetch_synth():
    d, truth = synth_meeting()
    write_manifest(d, {"kind": "recording", "audio": "meeting.wav", "text": "".join(t["text"] for t in truth)})


def fetch_hour():
    """合成会議 1 本と FLEURS 10 本を交互につなぎ、67 分を超えるまで続けた 1 本。"""
    sd, truth = synth_meeting()
    rows = fleurs_rows()
    d = DATA / "hour"
    d.mkdir(parents=True, exist_ok=True)
    files, gaps, text, total, k = [], [], [], 0.0, 0
    while total < 67 * 60:
        files.append(sd / "meeting.wav")
        gaps.append(1.0)
        text.append("".join(t["text"] for t in truth))
        total += duration(sd / "meeting.wav") + 1.0
        for r in rows[k:k + 10]:
            f = DATA / "fleurs" / "test" / r[1]
            files.append(f)
            gaps.append(0.5)
            text.append(r[2])
            total += duration(f) + 0.5
        k = (k + 10) % (len(rows) - 10)
    concat(files, gaps, d / "hour.wav")
    write_manifest(d, {"kind": "recording", "audio": "hour.wav", "text": "".join(text)})


FETCH = {"fleurs": fetch_fleurs, "rz": fetch_rz, "long": fetch_long, "synth": fetch_synth, "hour": fetch_hour}


# ---------- 実行 ----------

def transcribe(binary, audio, extra):
    p = subprocess.run([binary, "transcribe", str(audio), "--format", "json", "--yes", *extra],
                       capture_output=True, text=True)
    if p.returncode != 0 or not p.stdout.strip():
        return None
    return json.loads(p.stdout)


def peak_rss_bytes():
    r = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    return r if sys.platform == "darwin" else r * 1024


def load_items(src, m, n=None):
    items = m["items"][:n] if m["kind"] == "clips" else [{"id": "recording", **m}]
    for it in items:
        if "text_file" in it:
            it["text"] = (src / it["text_file"]).read_text(encoding="utf-8")
    return items


def report(src, public, m, items, result):
    """集計だけを出す。音声の名前と本文は出さない。"""
    hyp = result["hyp"]
    items = [it for it in items if it["id"] in hyp]
    failed = sum(1 for it in items if not hyp[it["id"]])
    audio_sec = sum(duration(src / it["audio"]) for it in items)
    sec = result["sec"]
    line = (f"  本数 {len(items)}（失敗 {failed}）  音声 {audio_sec:.0f} 秒  処理 {sec:.1f} 秒（{sec / max(audio_sec, 1e-9):.3f} 倍）"
            f"  出力 {sum(len(norm(hyp[it['id']])) for it in items)} 字")
    if result.get("peak_rss"):
        line += f"  最大メモリ {result['peak_rss'] / 1e9:.2f}GB"
    print(f"{src.name if public else '（手元の音源）'}  {result.get('version', '')}")
    print(line)
    scored = [it for it in items if it.get("text")]
    if scored:
        refs = [norm(it["text"]) for it in scored]
        hyps = [norm(hyp[it["id"]]) for it in scored]
        line = f"  CER {cer(refs, hyps) * 100:.1f}%（正解のある {len(scored)} 本）"
        if m.get("edge_free"):
            line += f"  端の挿入を除く {cer(refs, hyps, edge_free=True) * 100:.1f}%"
        print(line)


def results_dir(src, public):
    d = (DATA / "results") if public else (src / "results")
    d.mkdir(parents=True, exist_ok=True)
    return d


def run(a):
    src, public = resolve_source(a.source)
    m = json.load(open(src / "manifest.json", encoding="utf-8"))
    binary = str(Path(a.bin).expanduser().resolve()) if a.bin else shutil.which("kikitori") or "kikitori"
    version = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout.strip()
    items = load_items(src, m, a.n)
    hyp, t0 = {}, time.time()
    for it in items:
        segs = transcribe(binary, src / it["audio"], a.extra)
        hyp[it["id"]] = "" if segs is None else "".join(s["text"] for s in segs)
    result = {"version": version, "args": a.extra, "sec": time.time() - t0, "peak_rss": peak_rss_bytes(), "hyp": hyp}
    label = a.label or re.sub(r"[^0-9A-Za-z.-]+", "_", version.split()[-1] if version else "unknown")
    (results_dir(src, public) / f"{src.name}-{label}.json").write_text(json.dumps(result, ensure_ascii=False), encoding="utf-8")
    report(src, public, m, items, result)


def score(a):
    """ほかの道具の出力（{"hyp": {id: 本文}, "sec": 秒}）を、同じ音源の正解で採点する。"""
    src, public = resolve_source(a.source)
    m = json.load(open(src / "manifest.json", encoding="utf-8"))
    report(src, public, m, load_items(src, m), json.load(open(a.result, encoding="utf-8")))


def list_sources(_):
    for name in FETCH:
        print(f"{name:8} {'取得済み' if (DATA / name / 'manifest.json').exists() else '未取得'}")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    f = sub.add_parser("fetch")
    f.add_argument("sources", nargs="+", choices=list(FETCH))
    r = sub.add_parser("run")
    r.add_argument("source")
    r.add_argument("--bin", help="測る kikitori の実行ファイル（既定は PATH の kikitori）")
    r.add_argument("-n", type=int, default=None, help="clips の先頭から何本を使うか")
    r.add_argument("--label", help="結果のファイル名に付ける名前（既定は版）")
    r.add_argument("extra", nargs="*", help="`--` のあとに書いた引数を kikitori transcribe に渡す")
    sc = sub.add_parser("score")
    sc.add_argument("source")
    sc.add_argument("result", help="出力のファイル（run が results/ に書いたもの、または同じ形のもの）")
    sub.add_parser("list")
    a = ap.parse_args(argv)
    if a.cmd == "fetch":
        DATA.mkdir(parents=True, exist_ok=True)
        for s in a.sources:
            FETCH[s]()
            print(f"{s}: {DATA / s}")
    elif a.cmd == "run":
        run(a)
    elif a.cmd == "score":
        score(a)
    else:
        list_sources(a)


if __name__ == "__main__":
    main()
