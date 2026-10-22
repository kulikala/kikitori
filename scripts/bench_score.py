import os
import json, re, sys, unicodedata
from pathlib import Path
import jiwer
root = Path(__file__).resolve().parents[1]
ref = {l.split("\t")[1]: l.split("\t")[2] for l in open(root / "bench" / os.environ.get("BENCH", "fleurs") / "test.tsv", encoding="utf-8")}
_D = {c: i for i, c in enumerate("〇一二三四五六七八九")}
_U = {"十": 10, "百": 100, "千": 1000}
_RUN = re.compile("[〇一二三四五六七八九十百千万]+")


def _kanji_run_to_int(run: str) -> str:
    """漢数字の連なりを算用数字にする。両側に同じ変換をかけるので、系統による表記の差が採点に出ない。"""
    if not any(c in "十百千万" for c in run):
        return "".join(str(_D[c]) for c in run)  # 二〇二〇 のような位取りなし
    total, section, cur = 0, 0, 0
    for c in run:
        if c in _D:
            cur = _D[c]
        elif c in _U:
            section += (cur or 1) * _U[c]
            cur = 0
        else:  # 万
            total += (section + cur or 1) * 10000
            section = cur = 0
    return str(total + section + cur)


def norm(s):
    s = unicodedata.normalize("NFKC", s)
    s = _RUN.sub(lambda m: _kanji_run_to_int(m.group(0)), s)
    return re.sub(r"[\s、。，．,.!?！？「」『』・\-ー…~〜'\"“”()（）:：;；]", "", s)


def edge_free_cer(refs, hyps):
    """正解の前後にはみ出した挿入を数えない CER。切り出し音声に、ラベル外の発話が残るデータセット向け。"""
    err = tot = 0
    for r, h in zip(refs, hyps):
        o = jiwer.process_characters(r, h)
        ops = [c for c in o.alignments[0]]
        keep = [i for i, c in enumerate(ops) if c.type != "insert"]
        lo, hi = (keep[0], keep[-1]) if keep else (0, -1)
        for i, c in enumerate(ops):
            n = c.ref_end_idx - c.ref_start_idx if c.type != "insert" else c.hyp_end_idx - c.hyp_start_idx
            if c.type == "equal":
                continue
            if c.type == "insert" and not (lo < i < hi):
                continue
            err += n if c.type != "substitute" else max(c.ref_end_idx - c.ref_start_idx, c.hyp_end_idx - c.hyp_start_idx)
        tot += len(r)
    return err / max(tot, 1)


if __name__ == "__main__":
    for f in sys.argv[1:]:
        d = json.load(open(f)); ks = list(d["hyp"])
        cer = jiwer.cer([norm(ref[k]) for k in ks], [norm(d["hyp"][k]) for k in ks])
        e = edge_free_cer([norm(ref[k]) for k in ks], [norm(d["hyp"][k]) for k in ks])
        print(f"{Path(f).stem}: CER {cer*100:.1f}%  端の挿入を除く {e*100:.1f}%  ({len(ks)}本, {d['sec']:.0f}s)")
