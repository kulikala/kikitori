"""別の ASR モデルを同じ VAD 区間で FLEURS に当てる。使い方: bench_model.py N parakeet|sensevoice 出力.json"""
import os
import json, sys, time
from pathlib import Path
import numpy as np, sherpa_onnx as so
from kikitori import audio, engine, pipeline
n, kind, out = int(sys.argv[1]), sys.argv[2], sys.argv[3]
root = Path(__file__).resolve().parents[1]; m = root / "models"
if kind == "parakeet":
    d = m / "sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8"
    asr = so.OfflineRecognizer.from_nemo_ctc(model=str(d / "model.int8.onnx"), tokens=str(d / "tokens.txt"), num_threads=4)
elif kind == "reazon_ja":
    d = m / "sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01"
    f = lambda k: str(next(d.glob(f"{k}-epoch-99-avg-1.onnx")))
    asr = so.OfflineRecognizer.from_transducer(encoder=f("encoder"), decoder=f("decoder"), joiner=f("joiner"), tokens=str(d / "tokens.txt"), num_threads=4, decoding_method="modified_beam_search")
else:
    d = m / "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
    asr = so.OfflineRecognizer.from_sense_voice(model=str(d / "model.int8.onnx"), tokens=str(d / "tokens.txt"), language="ja", use_itn=False, num_threads=4)
rows = [l.split("\t") for l in open(root / "bench" / os.environ.get("BENCH", "fleurs") / "test.tsv", encoding="utf-8")][:n]
vad = engine.make_vad(m, 4); res, t0 = {}, time.time()
for r in rows:
    x = audio.load(str(root / "bench" / os.environ.get("BENCH", "fleurs") / "test" / r[1]))
    ch = pipeline.vad_chunks(engine.make_vad(m, 4), x)
    sts = []
    for _, w, _, _ in ch:
        s = asr.create_stream(); s.accept_waveform(16000, w); sts.append(s)
    if sts:
        asr.decode_streams(sts)
    res[r[1]] = "".join(s.result.text for s in sts)
json.dump({"hyp": res, "sec": time.time() - t0}, open(out, "w"), ensure_ascii=False)
print(kind, f"{time.time()-t0:.0f}s")
