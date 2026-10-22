import argparse
import json
import os
import sys
import time
from pathlib import Path

from .. import __version__, audio, device, dictionary, engine, models as modelstore, pipeline
from ..cli import FORMATS, emit
from . import catalog, evidence, external, local, ollama, route, setup


def load_glossary(path):
    """1 行 1 語、「表記<TAB>補足」。補足（読み・説明）は無くてもよい。"""
    out = []
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if line.strip() and not line.startswith("#"):
            w, _, d = line.partition("\t")
            out.append((w.strip(), d.strip()))
    return out


def facts(a, has_evidence):
    no_ollama = a.no_ollama or a.external != "off" or not has_evidence
    tags = None if no_ollama else ollama.models()
    here = ollama.is_local()
    hw = catalog.detect(a.hardware) if (here or a.hardware or os.environ.get("KIKITORI_HARDWARE")) else None
    return route.Facts(evidence=has_evidence, external=a.external, ollama=tags,
                       systemone=bool(tags) and ollama.has_systemone(), hardware=hw,
                       glossary=bool(a.glossary), merge_model=a.merge_model, no_ollama=a.no_ollama)


def show(plan):
    names = {"plain": "kikitori の本文のまま", "external": "外部 LLM で直す（A・B・K を渡す）",
             "merge": "Ollama の大きなモデルで区間ごとに突き合わせ", "glossary": "長さの規則＋tev1 で用語の置き換え",
             "rule": "長さの規則だけ（LLM を使わない）"}
    for n in plan.notes:
        print(f"# {n}", file=sys.stderr)
    print(f"# 補正の経路: {plan.route} - {names[plan.route]}{'（' + plan.model + '）' if plan.model else ''}", file=sys.stderr)


def main(argv=None):
    ap = argparse.ArgumentParser(prog="kikitori-refine",
                                 description="kikitori の本文を、別の認識結果（Qwen3-ASR・kana-whisper）と LLM で補正する")
    ap.add_argument("--version", action="version", version=f"kikitori-refine {__version__}")
    ap.add_argument("input", nargs="?", help="音声ファイル")
    ap.add_argument("--setup", action="store_true",
                    help="補正の環境を作る: Ollama が無ければ入れて起こし、この PC に合うモデルと Hugging Face のモデルを取得する")
    ap.add_argument("--dry-run", action="store_true", help="--setup で、何を入れるかだけを出して終える")
    ap.add_argument("--yes", action="store_true", help="--setup で、入れる前の確認を省く")
    ap.add_argument("--no-hf", action="store_true", help="--setup で、Hugging Face のモデルを先に取得しない")
    ap.add_argument("--hardware", choices=catalog.KINDS,
                    help="機器の判定を上書きする（apple / nvidia / cpu。環境変数 KIKITORI_HARDWARE でも可）")
    ap.add_argument("--model", default="ja", help="認識のモデル。補正は日本語（ja）だけに対応する")
    ap.add_argument("--glossary", help="用語集（1 行 1 語、表記<TAB>補足）。固有名詞を LLM に教える")
    ap.add_argument("--external", default="off", choices=["off", "bundle", "claude"],
                    help="外部 LLM で直す。bundle: 問いを JSON で出し、呼び出し元の LLM が直す / claude: claude CLI に渡す。"
                         "本文が PC の外へ出る")
    ap.add_argument("--apply", metavar="BUNDLE", help="--external bundle で出した JSON に、LLM の答え（stdin か --reply）を戻す")
    ap.add_argument("--reply", help="--apply で戻す答えのファイル（省略すると stdin）")
    ap.add_argument("--no-ollama", action="store_true", help="Ollama を使わない")
    ap.add_argument("--merge-model", help="突き合わせに使う Ollama のモデル（既定: この PC に合うものを表から選ぶ）")
    ap.add_argument("--plan", action="store_true", help="どの経路で補正するかだけを出して終える")
    ap.add_argument("--save-evidence", metavar="FILE", help="A・B・K を区間ごとに JSON で保存する")
    ap.add_argument("--evidence", metavar="FILE", help="--save-evidence で保存した A・B・K を使い、認識を省く")
    ap.add_argument("--models", default=str(modelstore.default_dir()))
    ap.add_argument("--device", default="auto")
    ap.add_argument("--threads", type=int, default=4)
    ap.add_argument("--num-speakers", type=int, default=-1)
    ap.add_argument("--threshold", type=float, default=0.6)
    ap.add_argument("--dict", help="kikitori の辞書（1 行 1 語、表記<TAB>読み）")
    ap.add_argument("--format", default="txt", choices=FORMATS,
                    help="txt: 1 行 1 発言 / json・yaml: LLM や道具で読む / srt・vtt: 字幕")
    ap.add_argument("-v", "--verbose", action="store_true")
    a = ap.parse_args(argv)
    log = (lambda m: print(f"# {m}", file=sys.stderr)) if a.verbose else (lambda m: None)
    if a.model != "ja":
        print("kikitori-refine の補正は、日本語の会議（--model ja）だけに対応します。"
              "英語が混ざる会議は kikitori --model ja-en で起こしてください（補正はできません）", file=sys.stderr)
        sys.exit(2)
    if a.setup:
        st = setup.gather(a.hardware)
        code = setup.run_setup(st, setup.plan_setup(st, a.no_hf), a.yes, a.dry_run)
        if code == 0:
            show(route.choose(facts(a, evidence.available())))
        sys.exit(code)

    if a.apply:
        b = json.loads(Path(a.apply).read_text(encoding="utf-8"))
        reply = Path(a.reply).read_text(encoding="utf-8") if a.reply else sys.stdin.read()
        texts, miss = external.apply(b, reply)
        if miss:
            print(f"警告: {len(miss)} 区間の答えがありませんでした。本文のまま残します（{miss[:10]}）", file=sys.stderr)
        emit([pipeline.Segment(s["start"], s["end"], s["speaker"], t) for s, t in zip(b["segments"], texts)], a.format)
        return

    glossary = load_glossary(a.glossary) if a.glossary else []
    plan = route.choose(facts(a, bool(a.evidence) or evidence.available()))
    show(plan)
    if a.plan:
        return
    t0 = time.time()
    if a.evidence:
        ev = json.loads(Path(a.evidence).read_text(encoding="utf-8"))
        segs = [pipeline.Segment(*e[:4]) for e in ev]
        A, B, K = [e[3] for e in ev], [e[4] for e in ev], [e[5] for e in ev]
        if plan.route == "plain":
            emit(segs, a.format)
            return
    else:
        if not a.input:
            ap.error("音声ファイルを指定してください")
        models = Path(a.models)
        modelstore.ensure(models, "ja")
        x = audio.load(a.input)
        make = lambda backend, p: engine.make_asr(models, p, a.threads, "fp32", "ja", backend)
        prov, asr = device.choose(make, lambda r: r.transcribe([x[:16000 * 8]]), a.device, log, "ja", "fp32")
        tee = evidence.Tee(asr)
        rep = dictionary.Replacer(dictionary.load(a.dict)) if a.dict else None
        segs = pipeline.run(x, tee, models, a.threads, a.num_speakers, a.threshold, rep, log)
        log(f"kikitori {len(segs)} 区間 {time.time() - t0:.0f}s")
        if plan.route == "plain":
            emit(segs, a.format)
            return
        A = [s.text for s in segs]
        t = time.time()
        B = evidence.qwen(tee.kept(), log)
        K = evidence.kana(x, [(s.start, s.end) for s in segs], log)
        log(f"Qwen3-ASR と kana-whisper {time.time() - t:.0f}s")
    if a.save_evidence:
        Path(a.save_evidence).write_text(json.dumps([[s.start, s.end, s.speaker, s.text, b, k] for s, b, k in zip(segs, B, K)],
                                                    ensure_ascii=False))

    if plan.route == "external":
        bun = external.bundle(segs, A, B, K, glossary)
        if a.external == "bundle":
            print(external.dumps(bun))
            return
        texts, miss = external.apply(bun, external.claude(bun["system"], [c["user"] for c in bun["chunks"]], log))
        if miss:
            print(f"警告: {len(miss)} 区間の答えがありませんでした。本文のまま残します（{miss[:10]}）", file=sys.stderr)
    elif plan.route == "merge":
        texts = local.merge(plan.model, A, B, K, glossary, log)
    else:
        texts = local.length_rule(A, B)
        if plan.route == "glossary":
            texts = local.glossary_fix(plan.model, texts, K, glossary, log)
    for s, tx in zip(segs, texts):
        s.text = tx
    emit(segs, a.format)
    print(f"# 処理 {time.time() - t0:.1f}s / 経路 {plan.route}", file=sys.stderr)
