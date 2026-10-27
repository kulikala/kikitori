"""比べる相手として、mlx-whisper で同じ音源を起こし、bench.py score で採点できる形で書く。

使い方（Apple Silicon の Mac。uv なら `uv run --with mlx-whisper --with jiwer benchmarks/whisper.py …`）:
  whisper.py SOURCE|DIR MODEL [-n N]
  例: whisper.py fleurs mlx-community/whisper-large-v3-turbo
出力は bench.py run と同じ results/ に書く。表示には、音声の名前も本文も出さない。
"""
import argparse
import json
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bench  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("source")
    ap.add_argument("model")
    ap.add_argument("-n", type=int, default=None)
    a = ap.parse_args()
    import mlx_whisper

    src, public = bench.resolve_source(a.source)
    m = json.load(open(src / "manifest.json", encoding="utf-8"))
    items = bench.load_items(src, m, a.n)
    hyp, t0 = {}, time.time()
    for it in items:
        hyp[it["id"]] = mlx_whisper.transcribe(str(src / it["audio"]), path_or_hf_repo=a.model, language="ja",
                                               condition_on_previous_text=False)["text"]
    result = {"version": a.model, "sec": time.time() - t0, "hyp": hyp}
    label = re.sub(r"[^0-9A-Za-z.-]+", "_", a.model.split("/")[-1])
    out = bench.results_dir(src, public) / f"{src.name}-{label}.json"
    out.write_text(json.dumps(result, ensure_ascii=False), encoding="utf-8")
    bench.report(src, public, m, items, result)


if __name__ == "__main__":
    main()
