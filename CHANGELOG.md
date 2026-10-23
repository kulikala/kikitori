# Changelog

この形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版の付け方は [Semantic Versioning](https://semver.org/lang/ja/) に従います。
0.x の間は、利用者の手順や出力が変わる変更で minor を上げます。

## [Unreleased]

### Added

- 設計の考え方の文書（`docs/DESIGN.md`）と、文書の書き方の文書（`docs/WRITING.md`）。

## [0.2.1] - 2026-10-23

### Fixed

- Windows on ARM で導入が止まらないようにした。CPU 版の onnxruntime を入れ、補正は入れない（補正が使う部品に ARM 版 Windows 用の配布物が無い）。辞書の置換（`--dict`）に使う fugashi も入れず、`--dict` は案内を出して終わる。
- Windows PowerShell 5.1 で `irm … | iex` したとき、導入スクリプトの日本語が化けないようにした。`install.ps1` は ASCII だけの入口になり、本体の `install-windows.ps1` を UTF-8 として読んで実行する。
- Windows の導入スクリプトが、終わったあとに利用者の PowerShell を閉じないようにした。
- macOS で、Ollama の公式の導入スクリプトが入れた直後の起動に失敗しても、`kikitori-refine --setup` が止まらず、Ollama を起動して続けるようにした。

## [0.2.0] - 2026-10-23

最初の公開版です。

### Added

- `kikitori`: 日本語の会議録音を、話者ごとに分けて文字起こしする。ReazonSpeech（認識）、Silero VAD（発話区間）、pyannote segmentation-3.0 と CAM++（話者）を使い、処理はすべて手元で完結する。
- `kikitori-refine`: 同じ区間を Qwen3-ASR と kana-whisper でも認識し、それを根拠に LLM で本文を直す。外部 LLM（呼び出し元の LLM か claude CLI）、Ollama の 27B での突き合わせ、tev1 での用語の置き換え、長さの規則の中から、環境に合うものを選ぶ。
- `kikitori-refine --setup`: Ollama が無ければ入れて起こし、PC の機器（Apple Silicon・NVIDIA・それ以外）とメモリに合うモデルを選んで取得する。
- `install.sh`・`install.ps1`: 1 行で kikitori を入れ、環境を作る。
- 出力形式 txt・json・yaml・srt・vtt。
- `--version`。
