#!/bin/sh
# 認証なしで全モデルの URL が取得できるかを確認する。HF_TOKEN は外して実行する。
unset HF_TOKEN HUGGING_FACE_HUB_TOKEN
while read -r url; do
  [ -z "$url" ] && continue
  code=$(curl -sIL -o /dev/null -w '%{http_code}' --max-time 30 "$url")
  echo "$code $url"
done <<'URLS'
https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2
https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx
https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx
https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17.tar.bz2
https://huggingface.co/pyannote/speaker-diarization-community-1/resolve/main/config.yaml
URLS
