# kikitori

日本語の会議の録音を、話者ごとに分けて文字起こしするコマンドラインツールです。Rust で書いた 1 つの実行ファイルで、日本語に特化した音声認識と話者分離を、手元の PC だけで行います。誤りを直す補正（`kikitori refine`）では、文字起こしの結果と、別の 2 つの音声認識モデルの結果を LLM で突き合わせ、1 つのモデルでは防げない誤変換や取りこぼしを直します。

[機能](#機能) · [インストール](#インストール) · [使い方](#使い方) · [困ったとき](docs/USAGE.md#困ったとき) · [通信と書き込むもの](docs/DISCLOSURES.md) · [変更点](CHANGELOG.md)

```console
$ kikitori transcribe 定例会議.m4a --num-speakers 2
[00:00.00] 話者1: それでは定例会議を始めますまず先週の進捗から確認しましょう
[00:06.43] 話者2: 新しい料金プランの試験運用は予定どおり終わりましたユーザーからの問い合わせは先週より二割ほど減っています
```

## 機能

kikitori の機能は文字起こしと補正の 2 つに分かれます。補正は任意で、文字起こしだけでも使えます。

| 機能区分 | コマンド | 説明 |
|---|---|---|
| 文字起こし | `kikitori transcribe` | 録音を話者ごとに分けて文字にします。LLM は使いません。 |
| 補正 | `kikitori refine` | 録音を受け取って文字起こしし、その誤りを直します。同じ音声を、文字起こしとは別の 2 つの音声認識モデル（Qwen3-ASR と kana-whisper）でも文字起こしし、LLM でそれらを突き合わせて、誤変換や取りこぼしを直します。 |

## 特長

- **ローカルで完結します。** 文字起こしも補正も既定では手元の PC の中で処理します。文字起こしの本文が PC の外へ出るのは、補正で外部の LLM（`claude` コマンドや、kikitori を呼び出した AI エージェント）を指定したときと、Ollama（LLM を動かすソフトウェア）の接続先を別の PC にしたときだけです。
- **1 つの実行ファイルで動きます。** 文字起こしは Rust で書いた実行ファイルと、kikitori がダウンロードするモデルだけで動きます。インストールは 1 行のコマンドです。
- **高速に動きます。** Apple Silicon の Mac の CPU だけで、67 分の会議を、話者分離を含めて約 4.7 分で文字起こしします（[速さ](docs/MODELS.md#速さ)）。
- **話者分離まで 1 つのコマンドで行います。** 声のある時間帯の検出、話者分離、音声認識を続けて行い、話者つきの結果を出します。
- **日本語に特化した音声認識モデルを使います。** 文字起こしには ReazonSpeech を使います。放送の自然な発話の評価では Whisper large-v3-turbo より誤りが少なく、朗読の評価では Whisper がわずかに上回りました（[精度](#精度)）。日本語に英語が混ざる会議向けのモデルもあります。
- **複数のモデルの突き合わせで誤りを直します。** 補正は用語集の表記に直すほか、同じ音声を別の 2 つの音声認識モデルでも文字起こしし、LLM で突き合わせて直します。5 分の会議の録音 1 本で測った文字誤り率は、補正の前の 28.4% から、LLM で突き合わせて直すと約 19〜22% に下がりました（[補正](docs/USAGE.md#直し方は環境で決まる)）。
- **AI エージェントから使えます。** Claude Code と Codex に kikitori の使い方を教えるエージェント・スキルを、`kikitori skill install` で書き込めます（[エージェント・スキル](#エージェント・スキル)）。

## 向いている用途

kikitori が受け持つのは、録音したファイルを文字起こしし、その誤りを直すところまでです。会議の録音は Web 会議のツールや録音機で行い、要約や議事録の作成は、kikitori の出力を LLM や AI エージェントに渡して行います。

Web 会議や会議室で録音した日本語の会議を、あとから文字起こしする用途に向いています。録音を外部のサービスに送れない会議にも使えます（[特長](#特長)）。日本語に英語が混ざる会議の録音も文字起こしできます（[使い方](#文字起こし)）。

次の用途には向きません。

- 日本語を含まない録音（英語だけの録音や、日本語と英語以外の言語の録音）。補正は日本語の録音だけに対応しています。
- 話している音声をその場で文字にするリアルタイムの文字起こし。kikitori は録音が終わったファイルを文字起こしします。
- 話者に名前を付ける用途。話者は「話者1」「話者2」のような番号で出ます。

## 免責事項

文字起こしと補正の結果には、固有名詞や数字の誤り、話者の取り違えが残ることがあります。記録として残す前や社外に出す前に、人が確かめてください。kikitori は無保証で提供します（[ライセンス](#ライセンス)）。

## 対応環境

| OS | 文字起こし | 補正 |
|---|---|---|
| macOS（Apple Silicon） | CPU | GPU（Metal） |
| macOS（Intel） | 非対応 | 非対応 |
| Linux x86_64 | CPU | CPU、NVIDIA の GPU（CUDA 版） |
| Linux arm64 | CPU | CPU |
| Windows x64 | CPU | CPU、NVIDIA の GPU（CUDA 版） |
| Windows arm64 | CPU | CPU |

表の補正の欄は、補正が使う 2 つの音声認識モデルを CPU と GPU のどちらで動かすかを示しています。補正の LLM は Ollama（[別途必要なソフトウェア](#別途必要なソフトウェア)）が動かし、GPU を使うかどうかは Ollama が決めます。NVIDIA の GPU を使うには、NVIDIA の GPU 向けの実行ファイル（CUDA 版）をインストールします（[NVIDIA の GPU を使う](docs/INSTALL.md#nvidia-の-gpu-を使う)）。インストールしたあとで `kikitori doctor` を実行すると、その PC で GPU を使えるか、ダウンロード済みのモデル、ffmpeg と Ollama の有無が分かります。

## 必要なディスク容量

| 機能区分 | ダウンロードするモデル | 用途 | 容量 |
|---|---|---|---|
| 文字起こし | 文字起こしのモデル（ReazonSpeech） | 録音を文字にする | 約 750MB |
| 文字起こし | 日本語と英語の文字起こしのモデル（ReazonSpeech ja-en） | 日本語に英語が混ざる録音を文字にする。`--model ja-en` を初めて使うときにダウンロードする | 約 440MB（ダウンロードする量） |
| 文字起こし | 形態素解析の辞書（unidic-lite） | `--dict` で語の読みを調べる | 約 250MB（ダウンロードする量は約 47MB） |
| 補正 | 音声認識モデル（Qwen3-ASR と kana-whisper） | 誤りを直す根拠にするため、同じ音声を文字起こしし直す | 約 7.3GB |
| 補正 | 小さな LLM のモデル | 用語集の語に置き換えるかを選ぶ | 約 0.8GB |
| 補正 | 大きな LLM のモデル | 文字起こしと 2 つの認識結果を突き合わせて本文を直す。メモリが 26GB 以上の Apple Silicon の Mac か、GPU のメモリが 26GB 以上の NVIDIA の GPU のときだけダウンロードする | 約 18GB |

文字起こしだけなら合計で約 1GB（`--model ja-en` のモデルを除く）、補正まで使うと約 9GB、大きな LLM のモデルもダウンロードする PC では約 27GB です。大きな LLM のモデルをダウンロードするかどうかは、インストールのときに、表の条件に合うかで自動で決まります。保存先は [docs/DISCLOSURES.md の書き込むもの](docs/DISCLOSURES.md#書き込むもの) に書いてあります。

## 別途必要なソフトウェア

使う機能によっては、次のソフトウェアが別途必要です。必要なものを、それぞれの配布元の案内に従って各自でインストールしてください。

| ソフトウェア | バージョン | 要否 | 説明 |
|---|---|---|---|
| [Ollama](https://ollama.com) | 0.40.0 以上 | 任意。補正で、手元の PC の LLM を使う場合に必要 | 補正の LLM を手元の PC で動かします。kikitori は Ollama を起動しないので、補正の前に起動しておきます。Ollama が無いときの補正は、LLM を使わない直し方になります。 |
| [ffmpeg](https://ffmpeg.org) | 指定なし | 任意。kikitori が内蔵の仕組みで読めない音声（Opus の音声、Google Meet の webm、mkv・avi・mov の動画など）を読む場合に必要 | 音声を読み込みます。m4a・mp4・mp3・wav・flac・ogg は、ffmpeg が無くても読めます。 |
| [Claude Code](https://claude.com/claude-code) | 指定なし | 任意。補正で `--external claude` を使う場合に必要 | kikitori が `claude` コマンドを呼び出し、補正の LLM として使います。 |
| NVIDIA のドライバと CUDA Toolkit | CUDA 12 | 任意。CUDA 版で NVIDIA の GPU を使う場合に必要 | 補正の音声認識モデルを NVIDIA の GPU で動かします。GPU は compute capability 8.0 以上（GeForce RTX 30 系、A100 以降）が必要です。 |

補正が手元の PC で使える LLM は、Ollama のモデルだけです。LM Studio・llama.cpp・vLLM のような、OpenAI 互換の API で LLM を動かすソフトウェアには対応していません（[issue #1](https://github.com/kulikala/kikitori/issues/1)）。

## インストール

インストールが終わったら、新しいターミナルを開くと `kikitori` コマンドを使えます。管理者権限は必要ありません。

### macOS の場合

ターミナルで次のコマンドを実行します。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh
```

文字起こしだけを使う場合は、補正のモデル（音声認識モデルと LLM のモデル）をダウンロードしない次のコマンドを実行します。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | KIKITORI_NO_REFINE=1 sh
```

### Linux の場合

ターミナルで次のコマンドを実行します。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh
```

文字起こしだけを使う場合は、補正のモデル（音声認識モデルと LLM のモデル）をダウンロードしない次のコマンドを実行します。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | KIKITORI_NO_REFINE=1 sh
```

### Windows の場合

PowerShell で次のコマンドを実行します。

```powershell
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex
```

文字起こしだけを使う場合は、補正のモデル（音声認識モデルと LLM のモデル）をダウンロードしない次のコマンドを実行します。

```powershell
$env:KIKITORI_NO_REFINE = "1"
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex
```

### 詳しいインストール方法

あとから補正を使えるようにする方法、インストールのオプション（環境変数）、TTY の無い環境（AI エージェント・CI）でのインストール、更新、削除は [docs/INSTALL.md](docs/INSTALL.md) に書いてあります。更新には `kikitori update`、削除には `kikitori uninstall` を使います。

## 使い方

### 文字起こし

```bash
kikitori transcribe 会議.m4a                                  # 話者つきのテキスト
kikitori transcribe 会議.m4a --num-speakers 4 --format yaml   # 人数を指定して YAML で出力
kikitori transcribe 会議.m4a --dict 辞書.tsv --format vtt     # 固有名詞の辞書を使って WebVTT の字幕で出力
kikitori transcribe 会議.m4a --model ja-en                    # 日本語に英語が混ざる録音を文字起こし
```

人数が分かるときは、`--num-speakers` で指定してください。指定しないと、1 人を 2 人に分けたり、2 人を 1 人にまとめたりすることがあります。結果は stdout に、進み具合と警告は stderr に出ます。文字起こしの本文には句読点が付きません。補正で LLM が突き合わせて直した本文には、句読点が付きます。出力の形式は、人が読むテキスト（既定）・json・yaml・字幕（srt・vtt）から `--format` で選べます。

出力形式、辞書の書き方、共通のオプションは [docs/USAGE.md](docs/USAGE.md) に書いてあります。

### 補正

用語集（`--glossary`）を渡すと、社名や製品名をその表記に直します。

```bash
kikitori refine --plan --glossary 用語集.tsv        # どう直すかだけを確かめる
kikitori refine 会議.m4a --glossary 用語集.tsv      # 直す
```

LLM は既定では Ollama のモデルを使います。外部の LLM として、`claude` コマンドか、kikitori を呼び出した AI エージェントも使えます。どの LLM でどう直すかは、PC のメモリと、Ollama にあるモデルから kikitori が決めます。

仕組み、直し方ごとの精度、外部の LLM の使い方は [docs/USAGE.md の補正](docs/USAGE.md#補正kikitori-refine) に書いてあります。

## エージェント・スキル

`kikitori skill install` を実行すると、Claude Code（`--agent codex` なら Codex）に、kikitori の使い方を教えるエージェント・スキルが書き込まれます。`--log-format json` を付けると診断ログが JSON になり、失敗は種類ごとの終了コードで返ります。詳しくは [docs/USAGE.md のエージェントから使う](docs/USAGE.md#エージェントから使う) に書いてあります。

## 精度

文字起こしの CER（文字誤り率。低いほど正確）です。各データ 100 本を、句読点を除いて採点しました。

| データ | kikitori | Whisper large-v3-turbo |
|---|---|---|
| FLEURS 日本語（朗読） | 5.9% | **5.2%** |
| ReazonSpeech の評価用データ（放送の自然な発話） | **7.1%** | 10.9% |

ReazonSpeech のデータは kikitori の認識モデルと同じ系統なので、kikitori に有利です。会議の音声で比べた値ではありません。採点の仕方、速さ、使うモデルとライセンスは [docs/MODELS.md](docs/MODELS.md) に書いてあります。

## 開発者ガイド

```bash
cargo build --release                  # 実行ファイルをビルドする
cargo test --workspace                 # 通常のテスト（モデルが必要なテストは除く）
cargo clippy --workspace --all-targets -- -D warnings
```

ビルドの最初に、音声認識の実行に使うライブラリ sherpa-onnx の配布物をネットワークからダウンロードします。Windows arm64 でビルドするには、Visual Studio 2026 と clang が必要です。認識・音声の読み込み・出力形式などは Rust のクレートに分かれていて、自分のプログラムの依存関係に、git のリポジトリかローカルのパスを指定して加えられます。crates.io には公開していません。

| 文書 | 内容 |
|---|---|
| [docs/DESIGN.md](docs/DESIGN.md) | 設計の考え方、クレートの一覧と役割 |
| [docs/ENGINES.md](docs/ENGINES.md) | 認識エンジンの追加方法 |
| [docs/RELEASING.md](docs/RELEASING.md) | リリースの手順と、配布物の一覧 |
| [docs/WRITING.md](docs/WRITING.md)・[docs/README-GUIDE.md](docs/README-GUIDE.md) | 文書の書き方と、README の書き方 |

## ライセンス

kikitori のコードは [Apache License 2.0](LICENSE) です。モデルのライセンスは [docs/MODELS.md](docs/MODELS.md#モデルとライセンス) に書いてあります。
