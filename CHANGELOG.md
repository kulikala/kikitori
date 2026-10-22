# Changelog

この形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版の付け方は [Semantic Versioning](https://semver.org/lang/ja/) に従います。
0.x の間は、利用者の手順や出力が変わる変更で minor を上げます。

## [Unreleased]

最初の公開版です。

### Added

- `kikitori`: 日本語の会議録音を、話者ごとに分けて文字起こしする。ReazonSpeech（認識）、Silero VAD（発話区間）、pyannote segmentation-3.0 と CAM++（話者）を使い、処理はすべて手元で完結する。
- `kikitori-refine`: 同じ区間を Qwen3-ASR と kana-whisper でも認識し、それを根拠に LLM で本文を直す。外部 LLM（呼び出し元の LLM か claude CLI）、Ollama の 27B での突き合わせ、tev1 での用語の置き換え、長さの規則の中から、環境に合うものを選ぶ。
- `kikitori-refine --setup`: Ollama が無ければ入れて起こし、PC の機器（Apple Silicon・NVIDIA・それ以外）とメモリに合うモデルを選んで取得する。
- `install.sh`・`install.ps1`: 1 行で kikitori を入れ、環境を作る。
- 出力形式 txt・json・yaml・srt・vtt。
- `--version`。
