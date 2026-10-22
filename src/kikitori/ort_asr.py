"""ONNX Runtime から直接動かす zipformer transducer（日本語専用モデル）。

sherpa-onnx の配布物には ROCm・Vulkan が無く、DirectML も含まれない。この層は、
onnxruntime（DirectML / ROCm / MIGraphX / CUDA / CoreML / CPU）の provider を直接選べるようにする。
重い encoder だけを指定の provider で動かし、小さな decoder と joiner は CPU で動かす。
"""
from pathlib import Path

import numpy as np

SR = 16000


def fbank(wave: np.ndarray, scale: float = 1.0) -> np.ndarray:
    import kaldi_native_fbank as knf

    o = knf.FbankOptions()
    o.frame_opts.dither = 0.0
    o.frame_opts.snip_edges = False
    o.frame_opts.samp_freq = SR
    o.mel_opts.num_bins = 80
    o.mel_opts.low_freq = 20.0
    o.mel_opts.high_freq = -400.0
    f = knf.OnlineFbank(o)
    f.accept_waveform(SR, (wave * scale).tolist())
    f.input_finished()
    return np.stack([f.get_frame(i) for i in range(f.num_frames_ready)]).astype(np.float32)


class OrtRecognizer:
    def __init__(self, model_dir: Path, tag: str, provider: str = "cpu", beam: int = 4, threads: int = 4, scale: float = 1.0):
        import onnxruntime as ort

        from .device import PROVIDERS

        if provider not in PROVIDERS:
            raise ValueError(f"未対応の provider: {provider}")
        want = [PROVIDERS[provider]]
        have = ort.get_available_providers()
        if want[0] not in have:
            raise RuntimeError(f"{want[0]} は、この onnxruntime では使えません（使える: {', '.join(have)}）")
        so = ort.SessionOptions()
        so.intra_op_num_threads = threads
        so.log_severity_level = 3
        if provider == "directml":  # DirectML EP の必須設定（メモリパターンの無効化と逐次実行）
            so.enable_mem_pattern = False
            so.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
        mk = lambda n, p: ort.InferenceSession(str(model_dir / f"{n}-{tag}.onnx"), so, providers=p)
        self.enc = mk("encoder", want if want[0] == "CPUExecutionProvider" else want + ["CPUExecutionProvider"])
        self.dec = mk("decoder", ["CPUExecutionProvider"])
        self.joi = mk("joiner", ["CPUExecutionProvider"])
        self.active = self.enc.get_providers()[0]
        if self.active != want[0]:  # 黙って CPU に戻ったときに、GPU として扱わない
            raise RuntimeError(f"{want[0]} を要求しましたが、{self.active} で動いています")
        self.tokens = []
        for line in open(model_dir / "tokens.txt", encoding="utf-8"):
            self.tokens.append(line.rstrip("\n").rsplit(None, 1)[0])
        import os
        self.beam, self.scale = int(os.environ.get("KIKITORI_BEAM", beam)), scale
        self._dec_cache = {}

    def _decoder(self, ctx):
        v = self._dec_cache.get(ctx)
        if v is None:
            v = self.dec.run(None, {"y": np.array([ctx], dtype=np.int64)})[0][0]
            if len(self._dec_cache) > 20000:
                self._dec_cache.clear()
            self._dec_cache[ctx] = v
        return v

    def _joiner(self, enc_frames, dec_outs):
        return self.joi.run(None, {"encoder_out": enc_frames, "decoder_out": dec_outs})[0]

    def transcribe_one(self, wave: np.ndarray) -> str:
        return self._transcribe(wave)

    def _transcribe(self, wave):
        x = fbank(wave, self.scale)
        enc, lens = self.enc.run(None, {"x": x[None], "x_lens": np.array([len(x)], dtype=np.int64)})
        enc = enc[0][: int(lens[0])]
        blank = 0
        beams = {(blank, blank): ((), 0.0)}  # 文脈 → (出力列, 対数確率)
        for t in range(enc.shape[0]):
            ctxs = list(beams)
            dec = np.stack([self._decoder(c) for c in ctxs])
            logits = self._joiner(np.repeat(enc[t:t + 1], len(ctxs), axis=0), dec)
            logp = logits - np.logaddexp.reduce(logits, axis=1, keepdims=True)
            cand = {}
            for h, ctx in enumerate(ctxs):
                seq, base = beams[ctx]
                for k in np.argpartition(-logp[h], self.beam)[: self.beam + 1]:
                    score = base + float(logp[h][k])
                    key, nseq = (ctx, seq) if k == blank else ((ctx[1], int(k)), seq + (int(k),))
                    if key in cand:
                        cs, cl = cand[key]
                        cand[key] = (cs if cl >= score else nseq, float(np.logaddexp(cl, score)))
                    else:
                        cand[key] = (nseq, score)
            beams = dict(sorted(cand.items(), key=lambda kv: -kv[1][1])[: self.beam])
        seq = max(beams.values(), key=lambda v: v[1])[0]
        return "".join(self.tokens[i] for i in seq).replace("▁", " ").strip()

    def transcribe(self, waves):
        return [self._transcribe(w) for w in waves]
