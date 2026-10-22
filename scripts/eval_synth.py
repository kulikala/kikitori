"""合成会議の採点。使い方: eval_synth.py 出力.json 正解.json
話者の正解率は 0.1 秒刻みで、話者番号の対応づけを総当たりして最良のものを採る。CER は時刻順に連結して比べる。"""
import itertools, json, re, sys, unicodedata
import jiwer
out = json.load(open(sys.argv[1])); truth = json.load(open(sys.argv[2]))
def norm(s):
    s = unicodedata.normalize("NFKC", s); return re.sub(r"[\s、。，．,.!?！？]", "", s)
T = max(t["end"] for t in truth)
def at(segs, t):
    for s in segs:
        if s["start"] - 0.2 <= t <= s["end"] + 0.2 and "speaker" in s: return s["speaker"]
frames = [round(i * 0.1, 1) for i in range(int(T * 10))]
tr = [(f, at(truth, f)) for f in frames]; tr = [(f, s) for f, s in tr if s is not None]
# 出力側は「その時刻の文字が属する区間」。区間は start のみなので次の区間の start までを担当とみなす
ou = sorted(out, key=lambda s: s["start"])
def out_at(t):
    cur = None
    for s in ou:
        if s["start"] <= t + 0.2: cur = s["speaker"]
    return cur
ospk = sorted({s["speaker"] for s in out}); tspk = sorted({s["speaker"] for s in truth})
best = (0, None)
for perm in itertools.permutations(ospk + [-1] * max(0, len(tspk) - len(ospk)), len(tspk)):
    m = dict(zip(tspk, perm)); acc = sum(out_at(f) == m[s] for f, s in tr) / len(tr)
    if acc > best[0]: best = (acc, m)
hyp = norm("".join(s["text"] for s in ou)); ref = norm("".join(t["text"] for t in truth))
print(f"話者の正解率 {best[0]*100:.1f}%（推定 {len(ospk)} 人 / 正解 {len(tspk)} 人）  CER {jiwer.cer(ref, hyp)*100:.1f}%  区間数 {len(out)}（正解 {len(truth)}）")
