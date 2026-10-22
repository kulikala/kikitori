"""provider × 精度ごとの認識の速さ（ウォームアップ後）。使い方: bench_device.py"""
import sys, time
from pathlib import Path
import numpy as np
from kikitori import audio, device, engine, pipeline
root = Path(__file__).resolve().parents[1]; m = root / "models"
rows = [l.split("\t") for l in open(root / "bench/fleurs/test.tsv", encoding="utf-8")][:30]
wavs = [audio.load(str(root / "bench/fleurs/test" / r[1])) for r in rows]
dur = sum(len(w) for w in wavs) / 16000
def run(asr, ws):
    sts = []
    for w in ws:
        s = asr.create_stream(); s.accept_waveform(16000, w); sts.append(s)
    asr.decode_streams(sts)
    return ["".join(s.result.text for s in sts)]
for prov in sys.argv[1].split(","):
    for prec in sys.argv[2].split(","):
        with device._Capture() as c:
            try:
                t = time.time(); asr = engine.make_asr(m, prov, 4, prec); load = time.time() - t
                run(asr, wavs[:2]); t = time.time(); run(asr, wavs); dt = time.time() - t
                res = f"RTF {dt/dur:.3f} ({dur/dt:.0f}x) 読込 {load:.1f}s"
            except Exception as e:
                res = f"失敗 {e}"
        warn = [l for l in c.text.splitlines() if any(k in l.lower() for k in device.BAD)]
        print(f"{prov:8s} {prec:5s} {res}  警告{len(warn)}件 {warn[:1]}", flush=True)
