"""本文 A と同じ区間を、別の 2 つの認識で文字にする（B: Qwen3-ASR、K: kana-whisper のカタカナ）。
B は A と同じ音声（余白つきの区間）を、K は区間の前後 0.2 秒を足した元の音声を認識する。
どちらも torch を使うので、1 つずつ読み込み、使い終えたら手放してから次へ進む。
"""
import gc
import importlib.util

import numpy as np

SR = 16000
QWEN = "neosophie/Qwen3-ASR-1.7B-JA"
KANA = "sbintuitions/kana-whisper"
KANA_PAD = 0.2


def available():
    return all(importlib.util.find_spec(m) for m in ("torch", "transformers", "qwen_asr"))


def _device():
    import torch
    if torch.backends.mps.is_available():
        return "mps", torch.float16
    if torch.cuda.is_available():
        return "cuda", torch.float16
    return "cpu", torch.float32


def _free():
    import torch
    gc.collect()
    if torch.backends.mps.is_available():
        torch.mps.empty_cache()
    if torch.cuda.is_available():
        torch.cuda.empty_cache()


def qwen(waves, log=lambda m: None):
    import torch
    from qwen_asr import Qwen3ASRModel
    dev, dtype = _device()
    log(f"Qwen3-ASR を {dev} で読み込みます")
    m = Qwen3ASRModel.from_pretrained(QWEN, dtype=dtype, device_map=dev)
    out = []
    with torch.no_grad():
        for w in waves:
            out.append(m.transcribe(audio=(w.astype(np.float32), SR), language="Japanese")[0].text.strip())
    del m
    _free()
    return out


def kana(samples, spans, log=lambda m: None):
    import torch
    from transformers import WhisperForConditionalGeneration, WhisperProcessor
    log("kana-whisper を CPU で読み込みます")
    p = WhisperProcessor.from_pretrained(KANA)
    m = WhisperForConditionalGeneration.from_pretrained(KANA).eval()
    out = []
    for a, b in spans:
        w = samples[int(max(0, a - KANA_PAD) * SR):int((b + KANA_PAD) * SR)].astype(np.float32)
        with torch.no_grad():
            ids = m.generate(p(w, sampling_rate=SR, return_tensors="pt").input_features, max_new_tokens=160)
        out.append(p.batch_decode(ids, skip_special_tokens=True)[0])
    del m
    _free()
    return out


class Tee:
    """pipeline.run に渡す認識器の包み。本文 A を返しつつ、渡された音声区間を取っておく（B を同じ音声で起こすため）。"""

    def __init__(self, asr):
        self.asr, self.waves, self.texts = asr, [], []

    def transcribe(self, waves):
        texts = self.asr.transcribe(waves)
        self.waves, self.texts = list(waves), list(texts)
        return texts

    def kept(self):
        """pipeline.run が残した区間（本文が空でないもの）の音声。"""
        return [w for w, t in zip(self.waves, self.texts) if t]
