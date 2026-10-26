"""2 つの文字起こし（kikitori の --format json）を比べ、本文の食い違いと話者の一致を出す。

使い方: parity.py REF.json HYP.json [--truth TRUTH.json] [--max-cer 0.5] [--max-speaker 2.0]

- 正解（truth）があれば、REF と HYP のそれぞれの正解に対する CER と話者の一致率を出し、その差を線と比べる。
- 正解が無ければ、REF を正解とみなして HYP の CER と話者の一致率を出し、CER と (100 - 一致率) を線と比べる。
- 線を超えたら 1 で終わる。
CER は記号・空白を除き NFKC で揃えてから、文字単位の編集距離で数える。
話者の一致率は、時間の重なりで話者の対応（並べ替え）を最もよく取ったときの、一致する時間の割合。
"""
import argparse
import itertools
import json
import re
import sys
import unicodedata


def norm(s):
    s = unicodedata.normalize("NFKC", s)
    return re.sub(r"[\s、。，．,.!?！？「」『』・…ー-]", "", s)


def edit(a, b):
    d = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        p, d[0] = d[0], i
        for j, y in enumerate(b, 1):
            p, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, p + (x != y))
    return d[-1]


def cer(ref, hyp):
    r, h = norm("".join(s["text"] for s in ref)), norm("".join(s["text"] for s in hyp))
    return 100.0 * edit(r, h) / max(len(r), 1)


def speaker_agreement(ref, hyp):
    """重なる時間のうち、話者の対応を最もよく取ったときに一致する割合（%）。"""
    ov = {}
    total = 0.0
    for r in ref:
        for h in hyp:
            o = min(r["end"], h["end"]) - max(r["start"], h["start"])
            if o > 0:
                ov[(r["speaker"], h["speaker"])] = ov.get((r["speaker"], h["speaker"]), 0.0) + o
                total += o
    if total == 0:
        return 0.0
    rs = sorted({k[0] for k in ov})
    hs = sorted({k[1] for k in ov})
    best = 0.0
    if len(hs) <= 7:
        for perm in itertools.permutations(rs + [None] * max(0, len(hs) - len(rs)), len(hs)):
            best = max(best, sum(ov.get((r, h), 0.0) for r, h in zip(perm, hs)))
    else:  # 話者が多いときは貪欲に対応を取る
        used = set()
        for (r, h), v in sorted(ov.items(), key=lambda kv: -kv[1]):
            if r not in used and ("h", h) not in used:
                used |= {r, ("h", h)}
                best += v
    return 100.0 * best / total


def load(p):
    d = json.load(open(p, encoding="utf-8"))
    return d["segments"] if isinstance(d, dict) else d


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ref")
    ap.add_argument("hyp")
    ap.add_argument("--truth")
    ap.add_argument("--max-cer", type=float, default=0.5, help="CER の差の上限（ポイント）")
    ap.add_argument("--max-speaker", type=float, default=2.0, help="話者の一致率の差の上限（ポイント）")
    a = ap.parse_args()
    ref, hyp = load(a.ref), load(a.hyp)
    if a.truth:
        t = load(a.truth)
        c_ref, c_hyp = cer(t, ref), cer(t, hyp)
        s_ref, s_hyp = speaker_agreement(t, ref), speaker_agreement(t, hyp)
        dc, ds = c_hyp - c_ref, s_ref - s_hyp
        print(f"CER  ref {c_ref:.2f}%  hyp {c_hyp:.2f}%  差 {dc:+.2f}")
        print(f"話者 ref {s_ref:.1f}%  hyp {s_hyp:.1f}%  差 {-ds:+.1f}")
    else:
        dc, ds = cer(ref, hyp), 100.0 - speaker_agreement(ref, hyp)
        print(f"CER（ref に対する hyp）{dc:.2f}%  話者の一致 {100 - ds:.1f}%")
    ok = dc <= a.max_cer and ds <= a.max_speaker
    print("OK" if ok else "線を超えた")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
