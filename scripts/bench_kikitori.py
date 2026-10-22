"""FLEURS ja の先頭 N 本を kikitori で認識して JSON に保存する。"""
import os
import json, sys, time
from pathlib import Path
from kikitori import audio, engine, pipeline

n = int(sys.argv[1]); prec = sys.argv[2]; out = sys.argv[3]
root = Path(__file__).resolve().parents[1]; models = root / "models"
rows = [l.split("\t") for l in open(root / "bench" / os.environ.get("BENCH", "fleurs") / "test.tsv", encoding="utf-8")][:n]
asr = engine.make_asr(models, os.environ.get("DEVICE", "cpu"), 4, prec, os.environ.get("MODEL", "ja"), os.environ.get("BACKEND", "sherpa"))
res, t0, dur = {}, time.time(), 0.0
for r in rows:
    x = audio.load(str(root / "bench" / os.environ.get("BENCH", "fleurs") / "test" / r[1])); dur += len(x) / 16000
    segs = pipeline.run(x, asr, models, 4, 1)
    res[r[1]] = "".join(s.text for s in segs)
json.dump({"hyp": res, "sec": time.time() - t0, "audio_sec": dur}, open(out, "w"), ensure_ascii=False)
print(prec, f"処理 {time.time()-t0:.0f}s / 音声 {dur:.0f}s")
