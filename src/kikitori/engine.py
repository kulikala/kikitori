"""sherpa-onnx の薄い層。推論エンジンはこのファイルの裏に隔離する。"""
from pathlib import Path

import sherpa_onnx as so

# 日本語専用（既定）と、日英混在（英語の幻覚が出やすい）
ASR_MODELS = {
    "ja": ("sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01", "epoch-99-avg-1", ("fp32", "int8")),
    "ja-en": ("sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17", "epoch-35-avg-1", ("fp32", "fp16", "int8")),
}
SEG = "sherpa-onnx-pyannote-segmentation-3-0/model.onnx"
EMB = "3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx"


BATCH_SAMPLES = 16000 * 60  # 1 バッチの（最長の波形 × 本数）の上限。大きいとメモリが急増する


class SherpaASR:
    """sherpa-onnx の認識器を、波形の列 → 文字列の列の形で使えるようにする。"""

    def __init__(self, rec):
        self.rec = rec

    def transcribe(self, waves):
        """長さの近い順に、合計の長さに上限を設けた小さなバッチで認識する（メモリを抑えるため）。"""
        out = [""] * len(waves)
        order = sorted(range(len(waves)), key=lambda i: len(waves[i]))
        batch = []
        for i in order:
            longest = len(waves[i])  # 昇順なので、いま足す波形が最長
            if batch and (len(batch) + 1) * longest > BATCH_SAMPLES:
                self._decode(waves, batch, out)
                batch = []
            batch.append(i)
        if batch:
            self._decode(waves, batch, out)
        return out

    def _decode(self, waves, idx, out):
        streams = []
        for i in idx:
            st = self.rec.create_stream()
            st.accept_waveform(16000, waves[i])
            streams.append(st)
        self.rec.decode_streams(streams)
        for i, st in zip(idx, streams):
            out[i] = st.result.text.strip()


def make_asr(models: Path, provider: str, threads: int, precision: str, model: str = "ja", backend: str = "sherpa"):
    if backend == "ort":
        from .ort_asr import OrtRecognizer

        if model != "ja" or precision != "fp32":
            raise ValueError("ort バックエンドは、日本語専用モデル(ja)の fp32 に対応しています")
        return OrtRecognizer(models / ASR_MODELS["ja"][0], ASR_MODELS["ja"][1], provider, threads=threads)
    return SherpaASR(_make_sherpa(models, provider, threads, precision, model))


def _make_sherpa(models: Path, provider: str, threads: int, precision: str, model: str):
    dirname, tag, precisions = ASR_MODELS[model]
    if precision not in precisions:
        raise SystemExit(f"モデル {model} は精度 {precision} に対応していません（{', '.join(precisions)}）")
    d = models / dirname
    sfx = {"int8": ".int8.onnx", "fp16": ".fp16.onnx", "fp32": ".onnx"}[precision]
    return so.OfflineRecognizer.from_transducer(
        encoder=str(d / f"encoder-{tag}{sfx}"),
        decoder=str(d / f"decoder-{tag}{sfx}"),
        joiner=str(d / f"joiner-{tag}{sfx}"),
        tokens=str(d / "tokens.txt"),
        num_threads=threads,
        sample_rate=16000,
        decoding_method="modified_beam_search",
        provider=provider,
    )


def make_vad(models: Path, threads: int):
    cfg = so.VadModelConfig()
    cfg.silero_vad.model = str(models / "silero_vad.onnx")
    cfg.silero_vad.threshold = 0.3
    cfg.silero_vad.min_silence_duration = 0.5
    cfg.silero_vad.min_speech_duration = 0.1
    cfg.silero_vad.max_speech_duration = 25.0
    cfg.sample_rate = 16000
    cfg.num_threads = threads
    return so.VoiceActivityDetector(cfg, buffer_size_in_seconds=60)


def make_diarizer(models: Path, threads: int, num_speakers: int, threshold: float):
    cfg = so.OfflineSpeakerDiarizationConfig(
        segmentation=so.OfflineSpeakerSegmentationModelConfig(
            pyannote=so.OfflineSpeakerSegmentationPyannoteModelConfig(model=str(models / SEG)),
            num_threads=threads,
        ),
        embedding=so.SpeakerEmbeddingExtractorConfig(model=str(models / EMB), num_threads=threads),
        clustering=so.FastClusteringConfig(num_clusters=num_speakers, threshold=threshold),
        min_duration_on=0.3,
        min_duration_off=0.5,
    )
    return so.OfflineSpeakerDiarization(cfg)


def make_embedder(models: Path, threads: int):
    return so.SpeakerEmbeddingExtractor(
        so.SpeakerEmbeddingExtractorConfig(model=str(models / EMB), num_threads=threads))
