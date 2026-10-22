"""LLM への指示と、区間をまとめた問い・答えの読み取り。
指示の文は、正解 A で測ったときの文そのまま（言い回しを変えると、測った値が当てはまらなくなる）。
区間は [番号] の行で渡し、答えも「[番号] 本文」の行で受け取る。
"""
import re

PREV, AFTER = 3, 2  # 文脈として添える、直前・直後の区間の数（本文 A）


def common(glossary):
    return ("言い淀み・相づち・繰り返しも、話されていれば残す。要約・言い換え・敬語の整形をしない。"
            "固有名詞は、音が近ければ用語集の表記にする。"
            "出力は、入力の各区間につき 1 行ずつ「[番号] 本文」の形で書く。番号は入力と同じにし、説明を書かない。"
            "用語集: " + "、".join(f"{w}（{d}）" if d else w for w, d in glossary))


def system(mode, glossary):
    head = {
        "merge": ("あなたは会議の文字起こしの校正者。同じ音声区間を 3 つの方式で認識した結果が、区間ごとに与えられる。"
                  "A と B は漢字かな交じり、K は音をカタカナで聞き取ったもの。どれも誤りを含み、取りこぼしもある。"
                  "区間ごとに、実際に話された文を一つ書け。A・B・K のどれかに根拠がある語だけを使い、根拠の無い語を足さない。"
                  "片方にしか無い部分も、K の音と合えば残す。"),
        "fixev": ("あなたは会議の文字起こしの校正者。音声認識の結果（本文）が区間ごとに与えられる。本文には誤変換や取りこぼしがある。"
                  "根拠として、同じ区間を別の音声認識で文字にしたもの（B）と、音をカタカナで聞き取ったもの（K）が添えられる。B と K も誤りを含み、"
                  "B は話されていない文を作ることがある。前後の文脈と B・K をもとに、本文の誤りを直し、取りこぼしを補え。"
                  "B にも K にも根拠の無い語は足さない。B にしか無い部分は、K の音と合うときだけ採る。正しい部分は変えない。"),
    }[mode]
    return head + common(glossary)


def block(mode, i, a, b, k):
    if mode == "merge":
        return f"[{i}]\nA: {a}\nB: {b}\nK: {k}"
    return f"[{i}]\n本文: {a}\nB: {b}\nK: {k}"


def user(mode, ids, A, B, K):
    """区間番号の並び ids をまとめた 1 回分の問い。文脈には、直前・直後の本文 A を添える。"""
    prev = "".join(A[j] for j in range(max(0, ids[0] - PREV), ids[0]))
    after = "".join(A[j] for j in range(ids[-1] + 1, min(len(A), ids[-1] + 1 + AFTER)))
    return (f"直前の発言（参考）: {prev or '（なし）'}\n直後の発言（参考）: {after or '（なし）'}\n\n"
            + "\n\n".join(block(mode, i, A[i], B[i], K[i]) for i in ids))


def chunks(n, size):
    return [list(range(s, min(n, s + size))) for s in range(0, n, size)]


def parse(text):
    """「[番号] 本文」の行を {番号: 本文} にする。それ以外の行は読み捨てる。"""
    return {int(m.group(1)): m.group(2).strip() for m in re.finditer(r"^\s*\[(\d+)\]\s*(.*)$", text, re.M)}
