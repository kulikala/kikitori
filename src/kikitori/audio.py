"""ffmpeg（外部依存）で 16 kHz モノラルの float32 に変換する。"""
import shutil
import subprocess

import numpy as np

SR = 16000


def load(path: str) -> np.ndarray:
    if shutil.which("ffmpeg") is None:
        raise SystemExit("ffmpeg が見つかりません。ffmpeg を PATH に入れてください。")
    cmd = ["ffmpeg", "-v", "error", "-i", path, "-vn", "-ac", "1", "-ar", str(SR), "-f", "f32le", "-"]
    p = subprocess.run(cmd, capture_output=True)
    if p.returncode != 0:
        raise SystemExit(f"ffmpeg が失敗しました: {p.stderr.decode(errors='replace')}")
    return np.frombuffer(p.stdout, dtype=np.float32)
