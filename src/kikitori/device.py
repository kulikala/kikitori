"""使える計算機を実測で選ぶ。指定が使えない・遅い場合は CPU に戻る。

候補は (バックエンド, provider)。GPU は ONNX Runtime の provider で選び、CPU は C++ の sherpa-onnx を使う。
"""
import os
import sys
import tempfile
import time

# OS ごとの GPU 候補（速い順に試走して選ぶ）
GPU = {
    "darwin": ["coreml"],
    "linux": ["cuda", "migraphx", "rocm"],
    "win32": ["cuda", "directml"],
}
# provider 名 → onnxruntime 側の名前
PROVIDERS = {
    "cpu": "CPUExecutionProvider",
    "coreml": "CoreMLExecutionProvider",
    "cuda": "CUDAExecutionProvider",
    "directml": "DmlExecutionProvider",
    "rocm": "ROCMExecutionProvider",
    "migraphx": "MIGraphXExecutionProvider",
}
BAD = ("not available", "fallback", "failed", "unsupported", "error", "cannot")


class _Capture:
    """C 側が stderr に出す警告を捕まえる。"""

    def __enter__(self):
        self.f = tempfile.TemporaryFile(mode="w+b")
        self.old = os.dup(2)
        os.dup2(self.f.fileno(), 2)
        return self

    def __exit__(self, *a):
        os.dup2(self.old, 2)
        os.close(self.old)
        self.f.seek(0)
        self.text = self.f.read().decode(errors="replace")
        self.f.close()


def ort_providers():
    try:
        import onnxruntime as ort

        return ort.get_available_providers()
    except Exception:  # noqa: BLE001
        return []


def candidates(requested: str, model: str = "ja", precision: str = "fp32"):
    """(ラベル, バックエンド, provider) の列。

    ONNX Runtime の経路は日本語専用モデルの fp32 だけが対応する。この経路のほうが探索が精密で、
    CPU でも少し正確なので、onnxruntime があれば CPU でもこちらを使う。なければ sherpa-onnx の CPU に戻る。
    """
    sherpa = ("cpu", "sherpa", "cpu")
    if requested == "sherpa":
        return [sherpa]
    if (model, precision) != ("ja", "fp32"):
        return [sherpa]  # ONNX Runtime 経路が対応しない組み合わせは、最初から sherpa-onnx の CPU にする
    have = set(ort_providers())
    cpu_ort = [("cpu", "ort", "cpu")] if PROVIDERS["cpu"] in have else []
    if requested == "cpu":
        return cpu_ort + [sherpa]
    if requested != "auto":
        return [(requested, "ort", requested)] + cpu_ort + [sherpa]
    return [(g, "ort", g) for g in GPU.get(sys.platform, []) if PROVIDERS[g] in have] + cpu_ort + [sherpa]


def try_candidate(make, backend, provider, probe):
    """認識器を作って試走する。(認識器, 秒, 警告) を返す。失敗は認識器が None。"""
    try:
        with _Capture() as c:
            asr = make(backend, provider)
            probe(asr)  # 1 回目は初期化やコンパイルを含むので、時間は 2 回目で測る
            t = time.time()
            probe(asr)
            dt = time.time() - t
    except (Exception, SystemExit) as e:  # noqa: BLE001
        return None, 0.0, f"{type(e).__name__}: {e}"
    bad = provider != "cpu" and any(k in c.text.lower() for k in BAD)
    return (None if bad else asr), dt, c.text.strip()


def choose(make, probe, requested="auto", log=lambda m: None, model="ja", precision="fp32"):
    """CPU を先に測って基準にし、GPU は CPU より 10% 以上速いときだけ選ぶ。

    GPU を明示したときは、作れた最初の GPU を使う（速さの比較はしない）。作れなければ CPU に戻る。
    """
    cands = candidates(requested, model, precision)
    cpus = [c for c in cands if c[0] == "cpu"]
    gpus = [c for c in cands if c[0] != "cpu"]
    reasons = []

    def attempt(c):
        asr, dt, msg = try_candidate(make, c[1], c[2], probe)
        if asr is None:
            log(f"{c[0]}: 使えません {msg[:160]}")
            reasons.append(f"{c[0]}: {msg}")
        else:
            log(f"{c[0]}: 試走 {dt:.2f}s")
        return asr, dt

    if requested not in ("auto", "cpu", "sherpa"):
        for c in gpus:
            asr, _ = attempt(c)
            if asr is not None:
                return c[0], asr
    base = None
    for c in cpus:  # 作れた最初の CPU 候補が基準
        asr, dt = attempt(c)
        if asr is not None:
            base = (c[0], asr, dt)
            break
    if requested == "auto":
        best = None
        for c in gpus:
            asr, dt = attempt(c)
            if asr is not None and (best is None or dt < best[2]):
                best = (c[0], asr, dt)
        if best is not None and (base is None or best[2] < base[2] * 0.9):
            return best[0], best[1]
    if base is None:
        raise SystemExit("認識器を作れませんでした。" + " / ".join(reasons))
    return base[0], base[1]
