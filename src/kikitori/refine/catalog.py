"""機器の判定と、補正に使う Ollama のモデルの表。セットアップ（pull するモデル）と経路選び（使うモデル）が同じ表と式を使う。

| 機器 | merge（突き合わせ） | fix（用語の置き換え） |
|---|---|---|
| apple（Apple Silicon） | qwen3.8:27b-mlx | tev1:0.8b-mxfp8 |
| nvidia | qwen3.8:27b-q4_K_M | tev1:0.8b-q8_0 |
| cpu（それ以外） | 使わない | tev1:0.8b-q8_0 |

Apple Silicon は Ollama の MLX エンジンで動く MLX 版、それ以外は GGUF 版。27B を CPU で回すと遅すぎるので、cpu では突き合わせをしない。
"""
import os
import platform
import shutil
import subprocess
from dataclasses import dataclass

GB = 2**30
HEADROOM = 8 * GB  # モデルのほかに空けておく分（OS とほかのアプリ、KV キャッシュ）
KINDS = ("apple", "nvidia", "cpu")

# 役目ごと・機器の種類ごとの候補（先にあるものほど優先）と、そのおおよその大きさ（ollama.com/library の tags ページの値）
CATALOG = {
    "merge": {"apple": ["qwen3.8:27b-mlx"], "nvidia": ["qwen3.8:27b-q4_K_M"], "cpu": []},
    "fix": {"apple": ["tev1:0.8b-mxfp8"], "nvidia": ["tev1:0.8b-q8_0"], "cpu": ["tev1:0.8b-q8_0"]},
}
SIZES = {"qwen3.8:27b-mlx": 18 * GB, "qwen3.8:27b-q4_K_M": 18 * GB,
         "tev1:0.8b-mxfp8": int(0.8 * GB), "tev1:0.8b-q8_0": int(0.8 * GB)}
MIN_OLLAMA = (0, 40, 0)  # tev1 の選択（/v1/systemone）と tev1:0.8b-mxfp8 が求める版


@dataclass
class Hardware:
    kind: str  # apple / nvidia / cpu
    memory: int | None  # モデルを載せる場所の大きさ（apple は統合メモリ、nvidia は最大 1 枚の VRAM、cpu は RAM）
    why: str = ""


def _run(cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=10).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ""


def _ram():
    try:
        return os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")
    except (ValueError, OSError, AttributeError):
        return None


def detect(override=None):
    """この PC の種類を調べる。override（--hardware / KIKITORI_HARDWARE）があればそれを使う。"""
    kind = override or os.environ.get("KIKITORI_HARDWARE")
    if kind:
        if kind not in KINDS:
            raise SystemExit(f"--hardware は {' / '.join(KINDS)} のどれかです: {kind}")
        mem = _nvidia_vram() if kind == "nvidia" else _ram()
        return Hardware(kind, mem, "指定による")
    if platform.system() == "Darwin" and _run(["sysctl", "-n", "hw.optional.arm64"]) == "1":
        return Hardware("apple", _ram(), "Apple Silicon")
    vram = _nvidia_vram()
    if vram:
        return Hardware("nvidia", vram, "NVIDIA の GPU")
    return Hardware("cpu", _ram(), "Apple Silicon・NVIDIA のどちらでもない")


def _nvidia_vram():
    if not shutil.which("nvidia-smi"):
        return None
    vals = [int(v) for v in _run(["nvidia-smi", "--query-gpu=memory.total", "--format=csv,noheader,nounits"]).split()
            if v.isdigit()]
    return max(vals) * 2**20 if vals else None


def fits(model, hw):
    return hw.memory is None or hw.memory >= SIZES.get(model, 0) + HEADROOM


def pick(hw):
    """この機器で使うモデル {役目: モデル or None} と、選んだ理由の行。"""
    out, why = {}, []
    for role, by_kind in CATALOG.items():
        cands = by_kind[hw.kind]
        ok = [m for m in cands if fits(m, hw)]
        out[role] = ok[0] if ok else None
        if not cands:
            why.append(f"{role}: {hw.kind} では使わない")
        elif not ok:
            why.append(f"{role}: {cands[0]} は {SIZES[cands[0]] // GB}GB で、この機器の {hw.memory / GB:.0f}GB に載らない"
                       f"（{(SIZES[cands[0]] + HEADROOM) // GB}GB 以上が要る）")
        else:
            why.append(f"{role}: {ok[0]}")
    return out, why


def all_candidates(role):
    """機器が分からない（遠くの Ollama）ときに見る、表の全候補（重複なし、表の順）。"""
    seen = []
    for kind in KINDS:
        for m in CATALOG[role][kind]:
            if m not in seen:
                seen.append(m)
    return seen
