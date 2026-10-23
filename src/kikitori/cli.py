import argparse
import json
import sys
import time
from functools import partial
from pathlib import Path

from . import __version__, audio, device, dictionary, engine, models as modelstore, pipeline


def fmt(t):
    return f"{int(t // 60):02d}:{t % 60:05.2f}"


FORMATS = ["txt", "json", "yaml", "srt", "vtt"]
# YAML の二重引用符の中で改行として扱われる文字と、制御文字
_YAML_ESC = {c: f"\\u{c:04x}" for c in [*range(0x20), 0x7F, 0x85, 0x2028, 0x2029]} | {0x22: '\\"', 0x5C: "\\\\"}


def _yaml_str(t):
    return '"' + t.translate(_YAML_ESC) + '"'


def _vtt_text(t):
    return t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def emit(segs, format):
    if format == "json":
        print(json.dumps([s.__dict__ for s in segs], ensure_ascii=False, indent=1))
    elif format == "yaml":
        for s in segs:
            print(f"- start: {float(s.start)!r}\n  end: {float(s.end)!r}\n  speaker: {s.speaker}\n  text: {_yaml_str(s.text)}")
    elif format == "srt":
        for i, s in enumerate(segs, 1):
            f = lambda t: f"{int(t // 3600):02d}:{int(t % 3600 // 60):02d}:{t % 60:06.3f}".replace(".", ",")
            print(f"{i}\n{f(s.start)} --> {f(max(s.end, s.start + 1))}\n話者{s.speaker + 1}: {s.text}\n")
    elif format == "vtt":
        f = lambda t: f"{int(t // 3600):02d}:{int(t % 3600 // 60):02d}:{t % 60:06.3f}"
        print("WEBVTT\n")
        for s in segs:
            print(f"{f(s.start)} --> {f(s.end)}\n<v 話者{s.speaker + 1}>{_vtt_text(s.text)}\n")
    else:
        for s in segs:
            print(f"[{fmt(s.start)}] 話者{s.speaker + 1}: {s.text}")


def main(argv=None):
    ap = argparse.ArgumentParser(prog="kikitori", description="日本語の会議録音を話者分離つきで文字起こしする")
    ap.add_argument("--version", action="version", version=f"kikitori {__version__}")
    ap.add_argument("input", nargs="?", help="音声ファイル。省略して --setup だけでもよい")
    ap.add_argument("--setup", action="store_true", help="モデルを取得して終了する")
    ap.add_argument("--models", default=str(modelstore.default_dir()), help="モデルの置き場（環境変数 KIKITORI_MODELS でも指定可）")
    ap.add_argument("--device", default="auto", help="auto / cpu / coreml / cuda / directml / migraphx / rocm（使えなければ CPU に戻る）")
    ap.add_argument("--model", default="ja", choices=["ja", "ja-en"], help="ja: 日本語専用（既定）/ ja-en: 日英混在")
    ap.add_argument("--precision", default="fp32", choices=["int8", "fp16", "fp32"])
    ap.add_argument("--threads", type=int, default=4)
    ap.add_argument("--num-speakers", type=int, default=-1, help="話者数が分かるときは指定すると精度が上がる")
    ap.add_argument("--threshold", type=float, default=0.6, help="話者数を指定しないときのクラスタリング閾値（大きいほど話者が少なくなる）")
    ap.add_argument("--dict", help="辞書ファイル（1行1語、表記<TAB>読み）")
    ap.add_argument("--format", default="txt", choices=FORMATS,
                    help="txt: 1 行 1 発言 / json・yaml: LLM や道具で読む / srt・vtt: 字幕")
    ap.add_argument("-v", "--verbose", action="store_true")
    a = ap.parse_args(argv)
    log = (lambda m: print(f"# {m}", file=sys.stderr)) if a.verbose else (lambda m: None)
    models = Path(a.models)
    modelstore.ensure(models, a.model)
    if a.setup or not a.input:
        print(f"モデルは {models} にあります", file=sys.stderr)
        return
    if a.dict and not dictionary.available():
        print("--dict には形態素解析の fugashi が要りますが、この環境にはありません"
              "（Windows on ARM には fugashi の配布物がありません）", file=sys.stderr)
        sys.exit(2)
    t0 = time.time()
    x = audio.load(a.input)
    make = lambda backend, p: engine.make_asr(models, p, a.threads, a.precision, a.model, backend)

    def probe(asr):
        asr.transcribe([x[:16000 * 8]])  # 実際の音声の先頭 8 秒で試す

    prov, asr = device.choose(make, probe, a.device, log, a.model, a.precision)
    log(f"provider={prov} precision={a.precision}")
    rep = dictionary.Replacer(dictionary.load(a.dict)) if a.dict else None
    segs = pipeline.run(x, asr, models, a.threads, a.num_speakers, a.threshold, rep, log)
    emit(segs, a.format)
    print(f"# 音声 {len(x) / 16000:.1f}s / 処理 {time.time() - t0:.1f}s / provider {prov}", file=sys.stderr)
