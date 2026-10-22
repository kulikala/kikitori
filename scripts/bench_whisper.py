import os
import json, sys, time
from pathlib import Path
import mlx_whisper
n = int(sys.argv[1]); repo = sys.argv[2]; out = sys.argv[3]
root = Path(__file__).resolve().parents[1]
rows = [l.split("\t") for l in open(root / "bench" / os.environ.get("BENCH", "fleurs") / "test.tsv", encoding="utf-8")][:n]
res, t0 = {}, time.time()
for r in rows:
    res[r[1]] = mlx_whisper.transcribe(str(root / "bench" / os.environ.get("BENCH", "fleurs") / "test" / r[1]), path_or_hf_repo=repo, language="ja", condition_on_previous_text=False)["text"]
json.dump({"hyp": res, "sec": time.time() - t0}, open(out, "w"), ensure_ascii=False)
print(repo, f"処理 {time.time()-t0:.0f}s")
