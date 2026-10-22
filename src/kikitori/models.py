"""必要なモデルの取得。すべて GitHub Releases の公開ファイルで、認証は要らない。"""
import os
import shutil
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

REL = "https://github.com/k2-fsa/sherpa-onnx/releases/download"
# (展開後に存在を確かめるパス, URL, 圧縮ファイルか)
ASR = {
    "ja": ("sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01/tokens.txt",
           f"{REL}/asr-models/sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01.tar.bz2", True),
    "ja-en": ("sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17/tokens.txt",
              f"{REL}/asr-models/sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17.tar.bz2", True),
}
COMMON = [
    ("sherpa-onnx-pyannote-segmentation-3-0/model.onnx",
     f"{REL}/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2", True),
    ("3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx",
     f"{REL}/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx", False),
    ("silero_vad.onnx", f"{REL}/asr-models/silero_vad.onnx", False),
]


def default_dir() -> Path:
    if os.environ.get("KIKITORI_MODELS"):
        return Path(os.environ["KIKITORI_MODELS"])
    base = os.environ.get("XDG_CACHE_HOME") or str(Path.home() / ".cache")
    return Path(base) / "kikitori" / "models"


def required(model: str = "ja"):
    return [ASR[model]] + COMMON


REQUIRED = required("ja")


def missing(root: Path, model: str = "ja"):
    return [m for m in required(model) if not (root / m[0]).exists()]


def _download(url: str, dest: Path):
    with urllib.request.urlopen(url, timeout=60) as r, open(dest, "wb") as f:
        total = int(r.headers.get("Content-Length") or 0)
        done = 0
        while chunk := r.read(1 << 20):
            f.write(chunk)
            done += len(chunk)
            if total and sys.stderr.isatty():
                print(f"\r  {done >> 20}/{total >> 20} MB", end="", file=sys.stderr)
            elif total and done * 4 // total > (done - len(chunk)) * 4 // total:
                print(f"  {done * 100 // total}%", file=sys.stderr)
        if sys.stderr.isatty():
            print(file=sys.stderr)
        if total and done != total:
            raise RuntimeError(f"ダウンロードが途中で切れました: {url}")


def ensure(root: Path, model: str = "ja"):
    todo = missing(root, model)
    if not todo:
        return
    root.mkdir(parents=True, exist_ok=True)
    print(f"モデルを取得します（{root}）。認証は不要です。", file=sys.stderr)
    for rel, url, archive in todo:
        print(f"- {url.rsplit('/', 1)[-1]}", file=sys.stderr)
        with tempfile.TemporaryDirectory(dir=root) as tmp:
            part = Path(tmp) / "dl"
            _download(url, part)
            if archive:
                with tarfile.open(part) as t:
                    t.extractall(tmp, filter="data")
                part.unlink()
                for child in Path(tmp).iterdir():
                    dst = root / child.name
                    if dst.exists():
                        shutil.rmtree(dst)
                    shutil.move(str(child), dst)
            else:
                shutil.move(str(part), root / rel)
    if missing(root, model):
        raise SystemExit("モデルの取得後も足りないファイルがあります: " + ", ".join(m[0] for m in missing(root, model)))
