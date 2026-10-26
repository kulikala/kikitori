"""Python 版 (kikitori-py021) から、kikitori-refine の Rust テスト用の基準データを作る。

py021 の仮想環境で実行する:
  KIKITORI_PY021=<Python 版 0.2.1 のソースの場所> <その venv の python> gen_golden.py

出力:
  prompts_golden.json  指示文・問い・parse の読み・chunks・bundle・length_rule・candidates
  merge_golden.json    merge と glossary_fix の、Ollama を仮で差し替えたときの答え
"""
import json
import os
import re
import sys

sys.path.insert(0, os.path.join(os.environ["KIKITORI_PY021"], "src"))

from kikitori.pipeline import Segment
from kikitori.refine import external, local, ollama, prompts

OUT = os.path.dirname(os.path.abspath(__file__))

A6 = ["あ", "いA", "うB", "ええ", "おおおお", "かかか"]
B6 = ["アア", "いB", "ウB", "エB", "オB", "きK"]
K6 = ["ア", "カ", "ウ", "エ", "オ", "キ"]

BUNDLE_SEGS = [Segment(0.0, 1.0, 0, "あ"), Segment(1.0, 2.0, 1, "い")]
BUNDLE = external.bundle(BUNDLE_SEGS, ["あ", "い"], ["ア", "イ"], ["ア", "イ"], [("北山精機", "")])
prompts_golden = {
    "merge_system": prompts.system("merge", [("北山精機", "")]),
    "fixev_system": prompts.system("fixev", []),
    "glossary_system": prompts.system("merge", [("北山精機", "得意先")]),
    "merge_user_3id": prompts.user("merge", [2, 3, 4], A6, B6, K6),
    "fixev_user_1id": prompts.user("fixev", [0], ["電話しましたか？"], ["電話しましたか"],
                                   ["デンワシマシタカ"]),
    "parse_numbered": {str(i): s for i, s in
                       sorted(prompts.parse("説明の行\n[0] こんにちは\n  [12]  さようなら \n[x] 無視").items())},
    "parse_eat_newline": {str(i): s for i, s in
                          sorted(prompts.parse("[0]\n[1] foo").items())},
    "parse_no_space": {str(i): s for i, s in
                       sorted(prompts.parse("[1]\n[9]佐藤").items())},
    "chunks_t2": prompts.chunks(5, 2),
    "chunks_t3_p2": prompts.chunks(7, 3),
    "bundle": BUNDLE,
    "bundle_text": external.dumps(BUNDLE),
    "length_rule": local.length_rule(["はい", "今日は晴れです", ""],
                                     ["この商品は自社製品です", "今日は晴れです。明日も", "あ"]),
    "candidates_lab_1": local.candidates("北山正気さんの工場", "キタヤマセーキサンノコージョー",
                                         [("北山精機", "得意先")]),
    "candidates_lab_0": local.candidates("北山正気さんの工場", "アイウエオ", [("北山精機", "得意先")]),
}
with open(os.path.join(OUT, "prompts_golden.json"), "w", encoding="utf8") as f:
    json.dump(prompts_golden, f, ensure_ascii=False, indent=1)
    f.write("\n")

merge_golden = {}

# --- merge: Ollama を仮で差し替える。区間 1 だけ失敗し、それ以外は番号の行を返す ---
elems = ["[0] 三郎はケーキを作った", "[1] 次郎はりんごを剥いた", "[2] 三郎はベッドで寝る"]
ANSWERS = {0: "[0] 三郎はケーキを作った", 1: None, 2: "[2] 三郎はベッドで寝る"}
G1 = [("北山精機", "")]


class Boom(Exception):
    pass


def fake_chat(model, sys_, user):
    m = re.search(r"\n\n\[(\d+)\]", user) or re.search(r"^\s*\[(\d+)\]", user)
    i = int(m.group(1))
    ans = ANSWERS[i]
    if ans is None:
        raise Boom("拒否された")
    return ans


ollama.chat = fake_chat
warned = []
out = local.merge("m1", ["一郎", "二郎", "三郎"], ["一ろ", "二ろ", "三ろ"], ["イチ", "ニ", "サン"], G1,
                  log=warned.append)
merge_golden["merge_texts"] = out
merge_golden["merge_log"] = warned

# --- glossary_fix: /v1/systemone を仮で差し替える ---
GL = [("北山精機", "得意先")]
TEXTS = ["北山正気さんの工場", "北山精機はもうある", "関係ない話"]
KANA = ["キタヤマセーキサンノコージョー", "キタヤマセイキサ", "アイウエオ"]
ANSWERS = {
    0: {"keep": 0.4, "c0": 0.6},
    1: {"keep": 0.9, "c0": 0.1},
    2: None,  # 候補が無いので問わない
}
reqs = []


def fake_post(path, body, timeout=None):
    reqs.append((path, body))
    state = body["state"]
    i = TEXTS.index(state["sentence"])
    if ANSWERS[i] is None:
        raise Boom("拒否された")
    return {"answers": {"fix": {"probabilities": ANSWERS[i]}}}


ollama.post = fake_post
texts = ["北山正気さんの工場", "北山精機はもうある", "関係ない話"]
log = []
out = local.glossary_fix("m1", texts, KANA, GL, log=log.append)
merge_golden["glossary_texts"] = out
merge_golden["glossary_log"] = log
merge_golden["glossary_inputs"] = texts
merge_golden["systemone_body"] = reqs[0][1]
with open(os.path.join(OUT, "merge_golden.json"), "w", encoding="utf8") as f:
    json.dump(merge_golden, f, ensure_ascii=False, indent=1)
    f.write("\n")
print("fixed: fixtures written to", OUT)
