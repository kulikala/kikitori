# kikitori

日本語の会議の録音を、話者ごとに分けて文字起こしするコマンドラインツールです。

[インストール](#インストール) · [使い方](#使い方) · [補正](#補正) · [困ったとき](docs/USAGE.md#困ったとき) · [通信と書き込むもの](docs/DISCLOSURES.md) · [変更点](CHANGELOG.md)

```console
$ kikitori transcribe 定例会議.m4a --num-speakers 2
[00:00.00] 話者1: それでは定例会議を始めますまず先週の進捗から確認しましょう
[00:06.43] 話者2: 新しい料金プランの試験運用は予定どおり終わりましたユーザーからの問い合わせは先週より二割ほど減っています
```

## 特長

- **手元の PC で完結します。** 文字起こしも補正も既定では手元の PC で動きます。録音も文字も外へ送りません。モデルの取得に、アカウントや API キーは要りません。
- **Python などを用意する必要がありません。** 文字起こしに要るのは、実行ファイル 1 つと、kikitori が取得するモデルだけです。
- **誤りを直す補正があります。** 補正のコマンド `kikitori refine` は同じ音声を別の 2 つの音声認識でも文字起こしし、LLM でそれらを突き合わせて、誤変換や取りこぼしを直します。
- **エージェントから使えます。** Claude Code・Codex 用のスキル、JSON の出力と診断ログ、失敗の種類ごとの終了コードがあります。

## 向いている用途

Web 会議や会議室で録音した日本語の会議を、あとから文字起こしする用途に向いています。出力の形式は、人が読むテキスト（既定）・json・yaml・字幕（srt・vtt）から選べます。

次の用途には向きません。

- 日本語以外の録音。日本語に英語が混ざる録音は、`--model ja-en` で文字起こしできます。英語だけの録音と、ほかの言語には対応していません。補正は日本語だけです。
- 句読点の付いた本文が要る用途。本文に句読点は付きません。
- 話している最中に字幕を出す用途。kikitori は録音したファイルを文字起こしします。
- 話者に名前を付ける用途。話者は「話者1」「話者2」のような番号で出ます。

## 対応環境

| OS | 文字起こし | 補正 |
|---|---|---|
| macOS（Apple Silicon） | CPU | GPU（Metal） |
| Linux x86_64 | CPU | CPU、NVIDIA の GPU（CUDA 版） |
| Linux arm64 | CPU | CPU |
| Windows x64 | CPU | CPU、NVIDIA の GPU（CUDA 版） |
| Windows arm64 | CPU | CPU |

Intel の Mac には対応していません。

表の補正の欄は、補正が使う 2 つの音声認識モデルを、CPU と GPU のどちらで動かすかを示しています。補正の LLM は Ollama が動かし、GPU を使うかどうかは Ollama が決めます。NVIDIA の GPU を使うには、CUDA 版をインストールします（[NVIDIA の GPU を使う](docs/INSTALL.md#nvidia-の-gpu-を使う)）。インストールしたあとで `kikitori doctor` を実行すると、その PC で GPU を使えるか、取得済みのモデル、ffmpeg と Ollama の有無が分かります。

## 必要なディスク容量

| 使う機能 | 取得するもの | 容量 |
|---|---|---|
| 文字起こし | 文字起こしのモデル、形態素解析の辞書 | 約 1GB |
| 補正 | 補正の音声認識モデル 2 つ | 約 6.8GB |
| 補正 | LLM のモデル（用語の置き換えに使う小さなモデル） | 約 0.8GB |
| 補正 | LLM のモデル（突き合わせに使う大きなモデル。メモリが 26GB 以上の Apple Silicon の Mac か、GPU のメモリが 26GB 以上の NVIDIA の GPU のときだけ） | 約 18GB |

補正まで使うと、合計で約 9GB、大きな LLM のモデルも取得する PC では約 27GB です。大きな LLM のモデルを取得するかどうかは、インストールのときに、表の条件に合うかで自動で決まります。保存先は [docs/DISCLOSURES.md の書き込むもの](docs/DISCLOSURES.md#書き込むもの) に書いてあります。

## 別にインストールするソフトウェア

次のソフトウェアは kikitori に含まれません。使う機能に合わせて、それぞれの案内に従ってインストールしてください。

| ソフトウェア | 要る場面 |
|---|---|
| [Ollama](https://ollama.com)（0.40.0 以上） | 補正で、手元の PC の LLM を使うとき |
| [ffmpeg](https://ffmpeg.org) | Opus の音声（Google Meet の webm など）を読むとき |
| [Claude Code](https://claude.com/claude-code)（`claude` コマンド） | 補正で `--external claude` を使うとき |
| NVIDIA のドライバと CUDA 12 のライブラリ | CUDA 版で NVIDIA の GPU を使うとき |

補正が手元の PC で使える LLM は、Ollama のモデルだけです。LM Studio・llama.cpp・vLLM のような、OpenAI 互換の API で LLM を動かすソフトウェアには対応していません（[issue #1](https://github.com/kulikala/kikitori/issues/1)）。m4a・mp4・mp3・wav・flac・ogg は、ffmpeg が無くても読めます。

## インストール

macOS/Linux でのインストール方法:

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh
```

Windows でのインストール方法（PowerShell）:

```powershell
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex
```

文字起こしだけを使うときは、補正のモデルを取得せずにインストールできます。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | KIKITORI_NO_REFINE=1 sh
```

```powershell
$env:KIKITORI_NO_REFINE = "1"
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex
```

あとから補正を使うときは、`kikitori refine --setup` を実行します。

インストールが終わったら、新しい端末を開いて、次のように実行します。

```bash
kikitori transcribe 会議.m4a --num-speakers 4
```

ほかの環境変数、端末の無い環境（エージェント・CI）でのインストール、更新、削除は [docs/INSTALL.md](docs/INSTALL.md) にあります。更新は `kikitori update`、削除は `kikitori uninstall` で行います。

## 使い方

```bash
kikitori transcribe 会議.m4a                                  # 話者つきのテキスト
kikitori transcribe 会議.m4a --num-speakers 4 --format yaml   # 人数を指定して YAML で出力
kikitori transcribe 会議.m4a --dict 辞書.tsv --format vtt     # 固有名詞の辞書を使って WebVTT の字幕で出力
```

人数が分かるときは、`--num-speakers` で指定してください。指定しないと、1 人を 2 人に分けたり、2 人を 1 人にまとめたりすることがあります。結果は stdout に、進み具合と警告は stderr に出ます。

出力形式、辞書の書き方、共通のオプションは [docs/USAGE.md](docs/USAGE.md) にあります。

## 補正

`kikitori refine` は、文字起こしの誤りを直します。同じ音声を、文字起こしとは別の 2 つの音声認識モデル（Qwen3-ASR と kana-whisper）でも文字起こしし、LLM でそれらを突き合わせて本文を直します。用語集（`--glossary`）を渡すと、社名や製品名をその表記に直します。

```bash
kikitori refine --plan --glossary 用語集.tsv        # どう直すかだけを確かめる
kikitori refine 会議.m4a --glossary 用語集.tsv      # 直す
```

LLM は既定では Ollama のモデルを使います。Ollama が無いときは、LLM を使わない直し方になります。外部の LLM として、`claude` コマンドか、kikitori を呼び出したエージェントも使えます。どの LLM でどう直すかは、PC のメモリと、Ollama にあるモデルで決まります。本文が PC の外へ出るのは、外部の LLM を指定したときと、Ollama の接続先を別の PC にしたときだけです。

仕組み、直し方ごとの精度、外部の LLM の使い方は [docs/USAGE.md の補正](docs/USAGE.md#補正kikitori-refine) にあります。

## エージェントから使う

`kikitori skill install` を実行すると、Claude Code（`--agent codex` なら Codex）に kikitori の使い方を教えるスキルが書き込まれます。`--log-format json` を付けると診断ログが JSON になり、失敗は種類ごとの終了コードで返ります。詳しくは [docs/USAGE.md のエージェントから使う](docs/USAGE.md#エージェントから使う) にあります。

## 精度

文字起こしの CER（文字誤り率。低いほど正確）です。各データ 100 本を、句読点を除いて採点しました。

| データ | kikitori | Whisper large-v3-turbo |
|---|---|---|
| FLEURS 日本語（朗読） | 5.9% | **5.2%** |
| ReazonSpeech の評価用データ（放送の自然な発話） | **7.1%** | 10.9% |

ReazonSpeech のデータは kikitori の認識モデルと同じ系統なので、kikitori に有利です。会議の音声で比べた値ではありません。採点の仕方、速さ、使うモデルとライセンスは [docs/MODELS.md](docs/MODELS.md) にあります。

## 開発

```bash
cargo build --release                  # 実行ファイルを作る
cargo test --workspace                 # 通常のテスト（モデルが要るテストは除く）
cargo clippy --workspace --all-targets -- -D warnings
```

ビルドの最初に、sherpa-onnx の配布物をネットワークから取得します。Windows arm64 でビルドするには、Visual Studio 2026 と clang が要ります。認識・音声の読み込み・出力形式などは Rust のクレートに分かれていて、自分のプログラムから git かパスで依存できます。crates.io には公開していません。

| 文書 | 中身 |
|---|---|
| [docs/DESIGN.md](docs/DESIGN.md) | 設計の考え方、クレートの一覧と役割 |
| [docs/ENGINES.md](docs/ENGINES.md) | 認識エンジンの足し方 |
| [docs/RELEASING.md](docs/RELEASING.md) | リリースの手順と、配る資産の一覧 |
| [docs/WRITING.md](docs/WRITING.md)・[docs/README-GUIDE.md](docs/README-GUIDE.md) | 文書の書き方と、README の書き方 |

## ライセンス

kikitori のコードは [Apache License 2.0](LICENSE) です。モデルのライセンスは [docs/MODELS.md](docs/MODELS.md#モデルとライセンス) にあります。
