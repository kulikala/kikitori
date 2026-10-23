"""手元で完結する補正: 長さの規則、Ollama の大きなモデルでの突き合わせ、tev1 での用語の置き換え。"""
import re
import sys

from . import ollama, prompts

SEP = re.compile(r"[、。,.\s]")


def length_rule(A, B):
    """B（Qwen3-ASR）が A より十分に長い区間だけ B を採る。A の取りこぼしを B で補うための規則。"""
    def pick(a, b):
        ra, rb = SEP.sub("", a), SEP.sub("", b)
        return b if len(rb) > len(ra) * 1.6 + 3 else a
    return [pick(a, b) for a, b in zip(A, B)]


def merge(model, A, B, K, glossary, log=lambda m: None):
    """区間ごとに 1 回、A・B・K を突き合わせさせる。答えが返らなかった区間は A のまま。"""
    sys_ = prompts.system("merge", glossary)
    out = list(A)
    for i in range(len(A)):
        try:
            got = prompts.parse(ollama.chat(model, sys_, prompts.user("merge", [i], A, B, K)))
        except Exception as e:  # 生成が止まった区間（繰り返しの打ち切りなど）は A のまま
            print(f"警告: 区間 {i} の突き合わせに失敗しました（{e}）。本文のまま残します", file=sys.stderr)
            continue
        if i in got:
            out[i] = got[i]
        log(f"{i}/{len(A)} {out[i]}")
    return out


# --- 用語の置き換え（音のカタカナ K に用語の読みが現れ、本文にその語が無い区間だけを問う） ---

_tagger = None


def _tg():
    global _tagger
    if _tagger is None:
        import fugashi  # Windows on ARM には無い。補正の中でだけ使う

        _tagger = fugashi.Tagger()
    return _tagger


def _kata(s):
    return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)


def reading(s):
    r = []
    for w in _tg()(s):
        k = getattr(w.feature, "kana", None)
        r.append(k if k and k != "*" else w.surface)
    return _kata("".join(r))


def _norm(k):
    """長音・促音・「ウ」の揺れと記号を落として、音の比較をゆるくする。"""
    return "".join(c for c in _kata(k) if c not in "ーウッ、。 ・?？!！")


def _lev(a, b):
    d = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        p, d[0] = d[0], i
        for j, y in enumerate(b, 1):
            p, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, p + (x != y))
    return d[-1]


def _fuzzy_in(sub, s):
    n = len(sub)
    if n == 0 or len(s) == 0:
        return n
    return min(_lev(s[i:i + L], sub) for L in {max(1, n - 1), n, n + 1} for i in range(0, max(1, len(s) - L + 1)))


RATIO = 0.34


def candidates(text, kana, glossary):
    """本文のうち、読みが用語に近い形態素 1〜3 個を、その用語に置き換えた文の候補。"""
    nk = _norm(kana)
    toks, pos = [], 0
    for w in _tg()(text):
        i = text.find(w.surface, pos)
        if i < 0:
            continue
        toks.append((i, i + len(w.surface)))
        pos = i + len(w.surface)
    cands = {}
    for w, _ in glossary:
        rw = _norm(reading(w))
        if w in text or _fuzzy_in(rw, nk) > max(1, int(len(rw) * RATIO)):
            continue  # 本文に既にある語と、音に現れない語は候補にしない
        best = None
        for a in range(len(toks)):
            for b in range(a, min(len(toks), a + 3)):
                s, e = toks[a][0], toks[b][1]
                span = text[s:e]
                if w in span or span in w or abs(len(span) - len(w)) > 2:
                    continue
                dist = _lev(_norm(reading(span)), rw)
                if best is None or dist < best[0]:
                    best = (dist, s, e)
        if best and best[0] <= max(2, len(rw) // 2 + 1):
            _, s, e = best
            cands[text[:s] + w + text[e:]] = f"「{text[s:e]}」を「{w}」に"
    return list(cands.items())[:10]


def _ask(model, sentence, kana, opts, before, after, glossary):
    crit = {"keep": f"そのまま: {sentence}"} | {f"c{i}": f"{t}（{why}）" for i, (t, why) in enumerate(opts)}
    ins = ("sentence は会議の音声認識の結果で、誤変換を含むことがある。sound は同じ音声をカタカナで聞き取ったもので、"
           "音の手がかりになる（ただし誤りも含む）。sentence の正しい文として最も可能性が高いものはどれか。before と after は前後の発言。"
           "用語集: " + "、".join(f"{w}（{reading(w)}。{d}）" for w, d in glossary))
    body = {"model": model, "state": {"before": before, "sentence": sentence, "sound": kana, "after": after},
            "questions": {"fix": {"type": "choice", "instructions": ins, "criteria": crit}}}
    return ollama.post("/v1/systemone", body, 120)["answers"]["fix"]["probabilities"]


def glossary_fix(model, texts, K, glossary, log=lambda m: None):
    """候補がある区間だけ tev1 に問い、「そのまま」以外が最も確からしければ置き換える。"""
    out = list(texts)
    for i, (text, kana) in enumerate(zip(texts, K)):
        opts = candidates(text, kana, glossary)
        if not opts:
            continue
        try:
            pr = _ask(model, text, kana, opts, "".join(texts[max(0, i - 1):i]), "".join(texts[i + 1:i + 2]), glossary)
        except Exception as e:
            print(f"警告: 区間 {i} の用語の確認に失敗しました（{e}）。そのまま残します", file=sys.stderr)
            continue
        best = max(pr, key=pr.get)
        if best != "keep":
            out[i] = opts[int(best[1:])][0]
            log(f"{i} {opts[int(best[1:])][1]}")
    return out
