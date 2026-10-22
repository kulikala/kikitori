"""環境の事実から、補正のやり方（経路）を決める。モデルも通信も使わない純粋な関数にしてある。

| 事実 | 経路 |
|---|---|
| B・K を作れない（torch などが無い） | plain（kikitori の本文のまま） |
| 外部 LLM を使うと指定された | external（A・B・K を外部 LLM に渡す。ローカルで先に直さない） |
| 突き合わせのモデルが Ollama にあり、載る | merge（区間ごとに突き合わせ） |
| 用語置き換えのモデルと /v1/systemone があり、用語集がある | glossary（長さの規則＋用語の置き換え） |
| それ以外 | rule（長さの規則だけ） |

使うモデルは catalog の表から選ぶ。手元の Ollama なら機器に合う候補だけ、遠くの Ollama（機器が分からない）なら表の全候補を上から見る。
"""
from dataclasses import dataclass, field

from . import catalog


@dataclass
class Facts:
    evidence: bool  # Qwen3-ASR と kana-whisper を動かせる
    external: str = "off"  # off / bundle（呼び出し元の LLM が直す）/ claude（claude CLI に渡す）
    ollama: dict | None = None  # Ollama にあるモデルの {名前: バイト数}。つながらなければ None
    systemone: bool = False  # Ollama の向き先に /v1/systemone がある
    hardware: catalog.Hardware | None = None  # 手元の Ollama のときの機器。遠くの Ollama なら None
    glossary: bool = False
    merge_model: str | None = None  # 利用者が指定した突き合わせのモデル
    no_ollama: bool = False


@dataclass
class Plan:
    route: str
    model: str | None = None
    notes: list = field(default_factory=list)


def _candidates(f, role):
    if role == "merge" and f.merge_model:
        return [f.merge_model]
    if f.hardware is None:
        return catalog.all_candidates(role)
    return [m for m in catalog.CATALOG[role][f.hardware.kind] if catalog.fits(m, f.hardware)]


def choose(f: Facts) -> Plan:
    if not f.evidence:
        return Plan("plain", notes=["Qwen3-ASR と kana-whisper を動かせないため、kikitori の本文のまま出します"
                                    "（kikitori[refine] で入ります。Intel の Mac には入りません）"])
    if f.external != "off":
        return Plan("external")
    notes = []
    if f.no_ollama or f.ollama is None:
        if not f.no_ollama:
            notes.append("Ollama が答えないため、LLM を使わず長さの規則だけで直します（kikitori-refine --setup で起こせます）")
        return Plan("rule", notes=notes)
    for m in _candidates(f, "merge"):
        if m in f.ollama:
            return Plan("merge", m, notes)
    if f.merge_model:
        notes.append(f"{f.merge_model} が Ollama にありません（kikitori-refine --setup で取得します）")
    if not f.glossary:
        notes.append("用語集が無いため、用語の置き換えはしません")
        return Plan("rule", notes=notes)
    if not f.systemone:
        notes.append("Ollama の向き先に /v1/systemone が無いため（0.40.0 より古い）、用語の置き換えはしません")
        return Plan("rule", notes=notes)
    fix = _candidates(f, "fix")
    for m in fix:
        if m in f.ollama:
            return Plan("glossary", m, notes)
    notes.append(f"{' / '.join(fix) or '用語置き換えのモデル'} が Ollama に無いため、用語の置き換えはしません"
                 "（kikitori-refine --setup で取得します）")
    return Plan("rule", notes=notes)
