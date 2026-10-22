"""辞書（表記と読みのペア）による置換。LLM は使わず、読みの一致だけで決める。

辞書ファイルは 1 行 1 語で「表記<TAB>読み」。読みはカタカナでもひらがなでもよい。
認識結果を形態素に分け、連続する形態素の読みが辞書の読みと一致したら表記に置き換える。
"""
import re
import unicodedata

_SMALL = str.maketrans("ァィゥェォッャュョヮ", "アイウエオツヤユヨワ")


def norm(reading: str) -> str:
    s = unicodedata.normalize("NFKC", reading)
    s = "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)  # ひらがな→カタカナ
    s = s.replace("ー", "")
    return s.translate(_SMALL)


def load(path):
    pairs = []
    for line in open(path, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line.strip() or line.startswith("#"):
            continue
        parts = re.split(r"[\t,]", line, maxsplit=1)
        if len(parts) == 2 and parts[0].strip() and parts[1].strip():
            pairs.append((parts[0].strip(), norm(parts[1].strip())))
    return sorted(pairs, key=lambda p: -len(p[1]))


class Replacer:
    def __init__(self, pairs):
        import fugashi

        self.pairs = pairs
        self.tagger = fugashi.Tagger()

    def _kana(self, w):
        f = w.feature
        k = getattr(f, "pron", None) or getattr(f, "kana", None)
        return norm(k) if k and k != "*" else norm(w.surface)

    def apply(self, text: str) -> str:
        if not self.pairs:
            return text
        ws = [(w.surface, self._kana(w)) for w in self.tagger(text)]
        out, i = [], 0
        while i < len(ws):
            hit = None
            for surf, rd in self.pairs:
                acc, j = "", i
                while j < len(ws) and len(acc) < len(rd):
                    acc += ws[j][1]
                    j += 1
                if acc == rd and len(rd) >= 2:
                    hit = (surf, j)
                    break
            if hit:
                out.append(hit[0])
                i = hit[1]
            else:
                out.append(ws[i][0])
                i += 1
        return "".join(out)
