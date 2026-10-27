# インストール・更新・削除

kikitori のインストール、更新、削除の詳しい説明です。概要は [README](../README.md) にあります。

## 対応環境

| OS | 文字起こし | 補正 |
|---|---|---|
| macOS（Apple Silicon） | CPU | GPU（Metal） |
| Linux x86_64 | CPU | CPU、NVIDIA の GPU（CUDA 版） |
| Linux arm64 | CPU | CPU |
| Windows x64 | CPU | CPU、NVIDIA の GPU（CUDA 版） |
| Windows arm64 | CPU | CPU |
| macOS（Intel） | 非対応 | 非対応 |

補正（`kikitori refine`）は、同じ音声を、文字起こしとは別の 2 つの音声認識モデルでも文字起こしし、その結果を根拠にして誤りを直します。2 つのモデル、Qwen3-ASR（B）と kana-whisper（K）を、まとめて B・K と呼びます。表の補正の欄は B・K を CPU と GPU のどちらで動かすかを示しています。補正の LLM は Ollama が動かし、GPU を使うかどうかは Ollama が決めます。

B・K を CPU と GPU のどちらで動かすかは、`kikitori refine` の `--device` で選べます。値は `auto`（既定）・`cpu`・`metal`・`cuda` です。`auto` は、Mac では Metal を、CUDA 版では使えれば CUDA を、それ以外では CPU を選びます。CUDA 版は NVIDIA の GPU 向けの実行ファイルで、Linux x86_64 と Windows x64 にあります。CUDA 版でない実行ファイルを、通常版と呼びます。

`kikitori doctor` を実行すると、その PC の OS、使える CPU・GPU と既定で使うもの、ダウンロード済みのモデルと容量、ffmpeg と Ollama の有無、補正がどの直し方になる見込みか、空きメモリが分かります。`kikitori doctor --json` は機械が読む形で出します。

### NVIDIA の GPU を使う

NVIDIA の GPU がある PC でも、既定でインストールされるのは通常版で、B・K は CPU で動きます。GPU を使うには、`KIKITORI_VARIANT=cuda` を付けて CUDA 版をインストールします（[インストールの環境変数](#インストールの環境変数)）。CUDA 版が GPU を使うには、次の 3 つが必要です。どれかが欠けていると、CUDA 版も CPU で動きます。

- compute capability 8.0 以上の GPU（GeForce RTX 30 系、A100 以降）。GPU の値は `nvidia-smi --query-gpu=compute_cap --format=csv,noheader` で分かります（例 `8.6`）。
- NVIDIA のドライバ。
- CUDA 12 の cuBLAS と cuRAND（NVIDIA の計算ライブラリ）。

CUDA 版をインストールしたあと、`kikitori doctor` の `cuda:` の行が「利用可能」なら GPU を使えます。この行はライブラリを読み込んで実際に初期化した結果です。

## 必要なディスク容量

| 機能区分 | ダウンロードするモデル | 用途 | 容量 |
|---|---|---|---|
| 文字起こし | 文字起こしのモデル（ReazonSpeech） | 録音を文字にする | 約 750MB |
| 文字起こし | 日本語と英語の文字起こしのモデル（ReazonSpeech ja-en） | 日本語に英語が混ざる録音を文字にする。`--model ja-en` を初めて使うときにダウンロードする | 約 440MB（ダウンロードする量） |
| 文字起こし | 形態素解析の辞書（unidic-lite） | 本文に句読点を付け、`--dict` で語の読みを調べる。ダウンロードするのは約 47MB で、展開すると約 250MB になる | 約 250MB |
| 補正 | B・K | 誤りを直す根拠にするため、同じ音声を文字起こしし直す | 約 7.3GB |
| 補正 | 小さな LLM のモデル | 用語集の語に置き換えるかを選ぶ | 約 0.8GB |
| 補正 | 大きな LLM のモデル | 文字起こしと B・K の認識結果を突き合わせて本文を直す。メモリが 26GB 以上の Apple Silicon の Mac か、GPU のメモリが 26GB 以上の NVIDIA の GPU のときだけダウンロードする（[機器とモデルの選び方](USAGE.md#機器とモデルの選び方)） | 約 18GB |

- 補正まで使うと、合計で約 9GB、大きな LLM のモデルもダウンロードする PC では約 27GB です。大きな LLM のモデルをダウンロードするかどうかは、表の条件に合うかで自動で決まります。
- 文字起こしのモデルと辞書をダウンロードする前に、ダウンロードする量（約 800MB）の 2 倍の空きがあるかを確かめます。
- ダウンロードの前には何をダウンロードするかと容量を出して確認を求めます。すでに手元にあるモデルはダウンロードし直しません。
- `kikitori refine --setup --dry-run` を実行すると、その PC で補正のためにダウンロードするものと容量を、ダウンロードせずに確かめられます。

保存先は [DISCLOSURES.md の書き込むもの](DISCLOSURES.md#書き込むもの) に書いてあります。

## 別途必要なソフトウェア

使う機能によっては、次のソフトウェアが別途必要です。必要なものを、それぞれの配布元の案内に従って各自でインストールしてください。`kikitori doctor` で、ffmpeg と Ollama があるかを確かめられます。

| ソフトウェア | バージョン | 要否 | 説明 |
|---|---|---|---|
| [Ollama](#ollama) | 0.40.0 以上 | 任意。補正で、手元の PC の LLM を使う場合に必要 | 補正の LLM を手元の PC で動かします。 |
| [ffmpeg](#ffmpeg) | 指定なし | 任意。kikitori が内蔵の仕組みで読めない音声（Opus の音声、Google Meet の webm、mkv・avi・mov の動画など）を読む場合に必要 | 音声を読み込みます。 |
| [Claude Code](#claude-code) | 指定なし | 任意。補正で `--external claude` を使う場合に必要 | kikitori が `claude` コマンドを呼び出し、補正の LLM として使います。 |
| [NVIDIA のドライバと CUDA Toolkit](#nvidia-のドライバと-cuda-toolkit) | CUDA 12 | 任意。CUDA 版で NVIDIA の GPU を使う場合に必要 | B・K を NVIDIA の GPU で動かします。 |

### Ollama

配布元は https://ollama.com です。

- 補正が手元の PC で使える LLM は、Ollama のモデルだけです。LM Studio・llama.cpp・vLLM のような、OpenAI 互換の API で LLM を動かすソフトウェアには対応していません（[issue #1](https://github.com/kulikala/kikitori/issues/1)）。
- Ollama が無いときの補正は、LLM を使わない直し方になります。
- kikitori は Ollama を起動しません。補正の前に Ollama を起動しておいてください。
- LLM のモデルは、`kikitori refine --setup` が Ollama にダウンロードさせます。Ollama をあとからインストールしたときは、起動してから `kikitori refine --setup` を実行し直すと、LLM のモデルをダウンロードします。
- Ollama の接続先は、環境変数 `KIKITORI_OLLAMA`、それが無ければ `OLLAMA_HOST`、どちらも無ければ `localhost:11434` です。

### ffmpeg

配布元は https://ffmpeg.org です。m4a（AAC-LC、HE-AAC v1・v2）・mp4・mp3・wav・flac・ogg は、ffmpeg が無くても読めます。

### Claude Code

配布元は https://claude.com/claude-code です。`--external claude` を付けると、補正の本文が Anthropic のサービスへ送られます。

### NVIDIA のドライバと CUDA Toolkit

CUDA 版が使う cuBLAS と cuRAND は、NVIDIA の CUDA Toolkit 12（https://developer.nvidia.com/cuda-downloads）に含まれます。GPU の条件は [NVIDIA の GPU を使う](#nvidia-の-gpu-を使う) に書いてあります。

## インストール

管理者権限は必要ありません。PATH の変更は新しく開いたターミナルから反映されるので、インストールが終わったら新しいターミナルを開くと、`kikitori` コマンドを使えます。

### macOS の場合

ターミナルで次のコマンドを実行します。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh
```

### Linux の場合

ターミナルで次のコマンドを実行します。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh
```

### Windows の場合

PowerShell で次のコマンドを実行します。

```powershell
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex
```

### インストールの環境変数

macOS と Linux では環境変数を `| sh` の `sh` の直前に付けます。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | KIKITORI_NO_REFINE=1 sh
```

Windows では先に環境変数を設定してから、インストールのコマンドを実行します。

```powershell
$env:KIKITORI_NO_REFINE = "1"
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex
```

| 環境変数 | 説明 |
|---|---|
| `KIKITORI_NO_REFINE=1` | 補正のモデル（B・K、LLM のモデル）をダウンロードせず、文字起こしだけをインストールする |
| `KIKITORI_VARIANT=cuda` | CUDA 版をインストールする（Linux x86_64・Windows x64） |
| `KIKITORI_VERSION` | インストールする版を指定する（例 `0.3.0`。`v` は付けない） |
| `KIKITORI_INSTALL_DIR` | 実行ファイルの置き場を変える |
| `KIKITORI_NO_MODIFY_PATH=1` | PATH を書き換えない |
| `KIKITORI_YES=1` | 確認を省く（TTY の無い環境向け） |

### あとから補正を使う

`KIKITORI_NO_REFINE=1` でインストールしたあとで補正を使うときは、`kikitori refine --setup` を実行します。`kikitori refine --setup --no-hf`（hf は B・K の配布元の Hugging Face）は、B・K をダウンロードせずに、LLM のモデルだけを用意します。この場合、初めて補正を実行するときに、B・K をダウンロードしてよいかを尋ねられます。B・K だけをダウンロードするには、`kikitori models pull --refine` を実行します。

### TTY の無い環境でインストールする

kikitori がダウンロードや削除の前に出す確認の質問は、標準入力がパイプのときは、TTY（ターミナル）に直接出ます。そのため、人が自分のターミナルで `curl … | sh` のようにインストールのコマンドを実行しても、確認の質問に答えられます。

エージェントの実行、コンテナ、CI のように TTY が無い環境では、確認の質問に答えられないので、モデルをダウンロードせずに実行ファイルだけがインストールされます。モデルまでダウンロードするには、`KIKITORI_YES=1` を付けてインストールのコマンドを実行するか、あとから `kikitori --yes setup`（文字起こしのモデルをダウンロードする。補正も使うなら、続けて `kikitori refine --setup --yes`）を実行します。インストールスクリプトは、モデルのダウンロードが済まなくても終了コード 0 で終わります。何がインストールされたかは、`kikitori doctor` で確かめます。

## 更新

```bash
kikitori update            # 最新版に更新する。モデルはダウンロードし直さない
kikitori update --dry-run  # 何を更新するかを確かめるだけ
```

- 新しい版が起動しないときは、実行ファイルを元の版に戻します。
- `kikitori skill install` で書き込んだスキルも新しい版の内容に書き換えます。ただし 0.3.0 から更新したときは、スキルが 0.3.0 の内容のまま残るので、更新のあとで `kikitori skill install` を実行し直します。更新する前の版は更新する前に `kikitori --version` で確かめられます。
- 通常版と CUDA 版を切り替えるときは、`--variant cuda`（CUDA 版へ）か `--variant cpu`（通常版へ）を付けます。
- インストールのコマンドを実行し直しても更新できます。

## 削除

```bash
kikitori uninstall --dry-run   # 消すものと残すものの一覧を出すだけ
kikitori uninstall             # 一覧を出し、確認したうえで消す
```

インストールスクリプトと kikitori は、置いたものを 1 行ずつインストール記録（`receipt.txt`）に書きます。インストール記録の場所は `~/.config/kikitori/receipt.txt`（Windows は `%APPDATA%\kikitori\receipt.txt`）です。`kikitori uninstall` は、インストール記録と既定の置き場を見て消します。

消すものは次のとおりです。

- 実行ファイル（`kikitori-refine` を含む）と、PATH のために足した行（`# added by kikitori` の印が付いた行。Windows は利用者の環境変数）
- モデルと形態素解析の辞書（`~/.cache/kikitori`。Windows は `%USERPROFILE%\.cache\kikitori`）
- エージェント用のスキル
- kikitori がダウンロードした B・K と、kikitori が Ollama にダウンロードさせた LLM のモデル。Ollama が止まっているときは、LLM のモデルを消すコマンドを出して残します。

残すものは次のとおりです。

- Ollama 本体と、利用者が自分でダウンロードした Ollama のモデル。
- Hugging Face のキャッシュ（`~/.cache/huggingface/hub`。`HF_HOME` や `HF_HUB_CACHE` で場所が変わります）のうち、kikitori がダウンロードしていないもの。ほかのツールが先にダウンロードしていた B・K もここに入ります。`--dry-run` の「残すもの」に場所が出るので、要らなければそのフォルダを消します。
- 利用者が書いた辞書や用語集のファイル。

`--keep-models` を付けると、モデル・形態素解析の辞書・B・K・LLM のモデルを残し、実行ファイル、スキル、PATH の行だけを消します。利用者が既定の置き場の外に自分で置いたもので、インストール記録に無いものは、一覧に出ず、消えません。消す前に `--dry-run` の一覧を確かめてください。

## 旧版 0.2.x から移行する

0.2.x は Python で書かれた版です。`uv tool` でインストールした 0.2.x があると、インストールスクリプトが 0.2.x をアンインストールしてよいかを尋ね、アンインストールしてから新しい実行ファイルを置きます。TTY の無い環境では `KIKITORI_YES=1` を付けて実行します。pip など `uv tool` 以外の方法でインストールした 0.2.x は、インストールスクリプトがアンインストールしないので、先に `pip uninstall kikitori` などでアンインストールしてください。

文字起こしのモデルと B・K は 0.2.x と同じ置き場にあるので、ダウンロードし直しません。主な違いは次のとおりです。全部の一覧は [CHANGELOG.md](../CHANGELOG.md) にあります。

- `kikitori-refine` は `kikitori refine` になります。`kikitori-refine` の名前は 0.3.x の間だけ `kikitori refine` に転送する名前として残ります。
- `kikitori --setup` は `kikitori setup` に、`kikitori-refine --setup` は `kikitori refine --setup` になります。
- 失敗の終了コードは 1 から、種類ごとの 2〜11 になります。
- `--device` の値は `auto`・`cpu`・`metal`・`cuda` になります。文字起こしはいつも CPU で動き、GPU を使えるのは補正の B・K だけになります。
- 録音全体の出力は 0.2.x と完全には一致しません。音声の読み込み方が違うので区切りが変わり、本文と話者の割り当ても変わることがあります。
