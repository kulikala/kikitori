# kikitori

日本語の会議の録音を、話者ごとに分けて文字にするコマンドラインツールです。

[導入](#導入) · [使い方](#使い方) · [補正](#補正) · [困ったとき](docs/USAGE.md#困ったとき) · [通信と書き込むもの](docs/DISCLOSURES.md) · [変更点](CHANGELOG.md)

```console
$ kikitori transcribe 定例会議.m4a --num-speakers 2
[00:00.00] 話者1: それでは定例会議を始めますまず先週の進捗から確認しましょう
[00:06.43] 話者2: 新しい料金プランの試験運用は予定どおり終わりましたユーザーからの問い合わせは先週より二割ほど減っています
```

- **手元の PC で完結します。** 文字起こしも補正も既定では手元の PC で動き、録音も文字も外へ送りません。モデルの取得にも、アカウントや API キーは要りません。
- **1 本の実行ファイルで動きます。** 文字起こしに要るのは、実行ファイルとモデルだけです。Python などの実行環境は要りません。macOS（Apple Silicon）、Linux、Windows に対応します。
- **誤りを直す補正があります。** `kikitori refine` は、同じ音声を別の 2 つの音声認識でも文字にし、LLM と突き合わせて、誤変換や取りこぼしを直します。LLM は、手元の PC で動かす Ollama か、外部の LLM（`claude` コマンド、または kikitori を呼んだエージェント）を選べます。
- **エージェントから使えます。** Claude Code・Codex 用のスキル、JSON の出力と診断ログ、失敗の種類ごとの終了コードがあります。

## 向いている使い方

Web 会議や会議室で録った日本語の会議を、あとから文字にする使い方に向いています。m4a・mp4・mp3・wav・flac・ogg はそのまま読めます。Google Meet の webm のような Opus の音声には、[ffmpeg](https://ffmpeg.org) が要ります。出力は、人が読むテキストのほか、json・yaml・字幕（srt・vtt）を選べます。

次の使い方には向きません。

- **日本語以外の録音。** 日本語に英語が混ざる録音は `--model ja-en` で起こせます。英語だけの録音や、ほかの言語は対象外です。補正は日本語だけです。
- **句読点の付いた本文が要る使い方。** 本文に句読点は付きません。
- **話している最中に字幕を出す使い方。** kikitori は録音したファイルを文字にします。
- **話者に名前を付ける使い方。** 話者は「話者1」「話者2」のような番号で出ます。名前との対応づけは、結果を読む側が行います。
- **Intel の Mac。** 0.3 の実行ファイルはありません。Python 版 0.2.x を使い続けてください（[Intel の Mac](docs/INSTALL.md#intel-の-mac)）。

## 動く環境

| OS | 確かめた範囲 |
|---|---|
| macOS（Apple Silicon） | 導入、文字起こし、補正、削除 |
| Linux arm64 | 導入、文字起こし、補正 |
| Windows arm64 | 導入、文字起こし、削除 |
| Linux x86_64・Windows x64 | 起動と文字起こし（arm64 の PC 上のエミュレーション） |

文字起こしはどの環境でも CPU で動きます。GPU は補正の一部に使い、Mac では Apple Silicon の GPU を使います。NVIDIA の GPU 向けの CUDA 版もありますが、NVIDIA の GPU を積んだ PC では確かめていません。GPU が無い PC でも、同じ使い方で全機能を使えます。詳しくは [INSTALL.md の動く環境](docs/INSTALL.md#動く環境) にあります。自分の PC で何が動くかは、入れたあと `kikitori doctor` で分かります。

## 導入

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh     # macOS・Linux
```

```powershell
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex            # Windows
```

導入スクリプトは実行ファイルを置き（macOS・Linux は `~/.local/bin`、Windows は `%LOCALAPPDATA%\Programs\kikitori`）、PATH に足し、文字起こしのモデル（約 800MB）を取得し、補正の環境を用意します。補正の環境では、Ollama（手元で LLM を動かすソフト）が無ければ公式の導入スクリプトで入れ、補正に使うモデルを取得します。取得する量は、本体のメモリが 26GB 以上ある Apple Silicon の Mac と、GPU のメモリが 26GB 以上ある NVIDIA の GPU の PC で約 27GB、それ以外で約 8.5GB です。補正を動かすには、さらに 6〜10GB ほどの空きメモリが要ります。取得や導入の前には、何をするかと容量を出して確認を求めます。kikitori の導入に管理者権限は要りません。Ollama は kikitori を削除しても残ります。書き込むものの全部は [docs/DISCLOSURES.md](docs/DISCLOSURES.md) にあります。

文字起こしだけを入れるなら、`KIKITORI_NO_REFINE=1` を付けます（`curl -fsSL …/install.sh | KIKITORI_NO_REFINE=1 sh`。Windows の付け方は [docs/INSTALL.md](docs/INSTALL.md#導入の環境変数)）。

入れ終わったら新しい端末を開き、1 本を起こしてみます。

```bash
kikitori transcribe 会議.m4a --num-speakers 4
```

導入の環境変数、端末の無い環境（エージェント・CI）での入れ方、NVIDIA の GPU の使い方、更新、削除、Python 版 0.2.x からの移行は、[docs/INSTALL.md](docs/INSTALL.md) にあります。更新は `kikitori update`、削除は `kikitori uninstall` です。

## 使い方

```bash
kikitori transcribe 会議.m4a                                  # 話者つきのテキスト
kikitori transcribe 会議.m4a --num-speakers 4 --format yaml   # 人数を決めて YAML
kikitori transcribe 会議.m4a --dict 辞書.tsv --format vtt     # 固有名詞の辞書を使って WebVTT の字幕
```

人数が分かるときは `--num-speakers` で渡してください。渡さないと、1 人を 2 人に分けたり、2 人を 1 人にまとめたりすることがあります。結果は stdout に、進み具合と警告は stderr に出ます。

出力形式、辞書の書き方、共通の旗は [docs/USAGE.md](docs/USAGE.md) にあります。

## 補正

`kikitori refine` は、文字起こしの誤りを直します。同じ音声を、文字起こしとは別の 2 つの音声認識モデル（Qwen3-ASR と kana-whisper）でも文字にし、LLM にそれらを突き合わせさせて、本文を直します。LLM は、既定では手元の PC で動く Ollama を使い、指定すれば外部の LLM も使えます。用語集（`--glossary`）を渡すと、社名や製品名をその表記に直します。

```bash
kikitori refine --plan --glossary 用語集.tsv        # どう直すかだけを見る
kikitori refine 会議.m4a --glossary 用語集.tsv      # 直す
```

どう直すかは PC のメモリと、入っているモデルで決まります。メモリの多い Apple Silicon の Mac や NVIDIA の GPU では大きな LLM が突き合わせ、それ以外の PC では軽い直し方になります。仕組み、直し方ごとの精度、外部の LLM（`claude` コマンドか、kikitori を呼んだエージェント）で直す方法は [docs/USAGE.md の補正](docs/USAGE.md#補正kikitori-refine) にあります。本文が PC の外へ出るのは、外部の LLM を指定したときと、Ollama を別の PC に向けたときだけです。

## エージェントから使う

`kikitori skill install` が、Claude Code（`--agent codex` なら Codex）に kikitori の使い方を教えるスキルを書き込みます。`--log-format json` で診断ログが JSON になり、失敗は種類ごとの終了コードで返ります。詳しくは [docs/USAGE.md のエージェントから使う](docs/USAGE.md#エージェントから使う) にあります。

## 精度

文字起こしの CER（文字誤り率。低いほど正確）です。各データ 100 本を、句読点を除いて採点しました。

| データ | kikitori | Whisper large-v3-turbo |
|---|---|---|
| FLEURS 日本語（朗読） | 5.9% | **5.2%** |
| ReazonSpeech の評価用データ（放送の自然な発話） | **7.1%** | 10.9% |

ReazonSpeech のデータは、kikitori の認識モデルと同じ系統なので、kikitori に有利です。会議の音声で比べた値ではありません。採点の仕方、速さ、使うモデルとライセンスは [docs/MODELS.md](docs/MODELS.md) にあります。

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
