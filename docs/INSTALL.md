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

Intel の Mac には対応していません。

補正（`kikitori refine`）は、同じ音声を、文字起こしとは別の 2 つの音声認識モデルでも文字起こしし、その結果を根拠にして誤りを直します。2 つのモデル、Qwen3-ASR（B）と kana-whisper（K）を、まとめて B・K と呼びます。表の補正の欄は B・K を CPU と GPU のどちらで動かすかを示しています。補正の LLM は Ollama が動かし、GPU を使うかどうかは Ollama が決めます。

B・K を CPU と GPU のどちらで動かすかは、`kikitori refine` の `--device` で選べます。値は `auto`（既定）・`cpu`・`metal`・`cuda` です。`auto` は、Mac では Metal を、CUDA 版では使えれば CUDA を、それ以外では CPU を選びます。CUDA 版は NVIDIA の GPU 向けの実行ファイルで、Linux x86_64 と Windows x64 にあります。CUDA 版でない実行ファイルを、ここでは通常版と呼びます。

`kikitori doctor` を実行すると、その PC の OS、使える CPU・GPU と既定で使うもの、取得済みのモデルと容量、ffmpeg と Ollama の有無、補正がどの直し方になる見込みか、空きメモリが分かります。`kikitori doctor --json` は機械が読む形で出します。

### NVIDIA の GPU を使う

NVIDIA の GPU がある PC でも、既定でインストールされるのは通常版で、B・K は CPU で動きます。GPU を使うには、`KIKITORI_VARIANT=cuda` を付けて CUDA 版をインストールします（[インストールの環境変数](#インストールの環境変数)）。CUDA 版が GPU を使うには、次の 3 つが要ります。どれかが欠けていると、CUDA 版も CPU で動きます。

- compute capability 8.0 以上の GPU（GeForce RTX 30 系、A100 以降）。GPU の値は `nvidia-smi --query-gpu=compute_cap --format=csv,noheader` で分かります（例 `8.6`）。
- NVIDIA のドライバ。
- CUDA 12 の cuBLAS と cuRAND（NVIDIA の計算ライブラリ）。

CUDA 版をインストールしたあと、`kikitori doctor` の `cuda:` の行が「利用可能」なら GPU を使えます。この行はライブラリを読み込んで実際に初期化した結果です。

## 必要なディスク容量

| 使う機能 | 取得するもの | 容量 |
|---|---|---|
| 文字起こし | 文字起こしのモデル | 約 750MB |
| 文字起こし | 形態素解析の辞書（`--dict` が語の読みを引くのに使う） | 約 250MB（取得するのは約 47MB） |
| 補正 | B・K | 約 6.8GB |
| 補正 | LLM のモデル（用語の置き換えに使う小さなモデル） | 約 0.8GB |
| 補正 | LLM のモデル（突き合わせに使う大きなモデル） | 約 18GB |

- 文字起こしのモデルと辞書を取得する前に、取得する量（約 800MB）の 2 倍の空きがあるかを確かめます。
- 大きな LLM のモデルを取得するのは、Apple Silicon の Mac で本体のメモリが、NVIDIA の GPU で GPU のメモリが 26GB 以上あるときだけです（[機器とモデルの選び方](USAGE.md#機器とモデルの選び方)）。
- 補正まで使うと、合計で約 9GB、大きな LLM のモデルも取得する PC では約 27GB です。大きな LLM のモデルを取得するかどうかは、PC のメモリを見て自動で決まります。
- 取得の前には、何を取得するかと容量を出して確認を求めます。すでに手元にあるモデルは取得し直しません。
- `kikitori refine --setup --dry-run` を実行すると、その PC で補正のために取得するものと容量を、取得せずに確かめられます。

保存先は [DISCLOSURES.md の書き込むもの](DISCLOSURES.md#書き込むもの) に書いてあります。

## 別にインストールするソフトウェア

次のソフトウェアは kikitori に含まれません。使う機能に合わせて、それぞれの案内に従ってインストールしてください。`kikitori doctor` で、ffmpeg と Ollama があるかを確かめられます。

### Ollama

補正で手元の PC の LLM を使うときに要ります。0.40.0 以上を使ってください。入手先は https://ollama.com です。

- 補正が手元の PC で使える LLM は、Ollama のモデルだけです。LM Studio・llama.cpp・vLLM のような、OpenAI 互換の API で LLM を動かすソフトウェアには対応していません（[issue #1](https://github.com/kulikala/kikitori/issues/1)）。
- kikitori は Ollama を起動しません。補正の前に Ollama を起動しておいてください。
- LLM のモデルは、`kikitori refine --setup` が Ollama に取得させます。Ollama をあとからインストールしたときは、起動してから `kikitori refine --setup` を実行し直すと、LLM のモデルを取得します。
- Ollama の接続先は、環境変数 `KIKITORI_OLLAMA`、それが無ければ `OLLAMA_HOST`、どちらも無ければ `localhost:11434` です。

### ffmpeg

Opus の音声（Google Meet の webm など）を読むときに要ります。入手先は https://ffmpeg.org です。m4a（AAC-LC、HE-AAC v1・v2）・mp4・mp3・wav・flac・ogg は、ffmpeg が無くても読めます。

### Claude Code

補正で `--external claude` を使うときに要ります。kikitori は Claude Code の `claude` コマンドを呼び出します。入手先は https://claude.com/claude-code です。

### NVIDIA のドライバと CUDA 12 のライブラリ

CUDA 版で NVIDIA の GPU を使うときに要ります。cuBLAS と cuRAND は NVIDIA の CUDA Toolkit 12（https://developer.nvidia.com/cuda-downloads）に含まれます。GPU の条件は [NVIDIA の GPU を使う](#nvidia-の-gpu-を使う) にあります。

## インストール

macOS/Linux でのインストール方法:

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh
```

Windows でのインストール方法（PowerShell）:

```powershell
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex
```

管理者権限は要りません。インストールが終わったら、新しい端末を開いて（PATH の変更が反映されます）、次のように実行します。

```bash
kikitori transcribe 会議.m4a --num-speakers 4
```

### インストールの環境変数

macOS/Linux では環境変数をパイプの先の `sh` の前に付けます。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | KIKITORI_NO_REFINE=1 sh
```

Windows では先に環境変数を設定してから、インストールのコマンドを実行します。

```powershell
$env:KIKITORI_NO_REFINE = "1"
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex
```

| 環境変数 | 働き |
|---|---|
| `KIKITORI_NO_REFINE=1` | 補正のモデル（B・K、LLM のモデル）を取得せず、文字起こしだけをインストールする |
| `KIKITORI_VARIANT=cuda` | CUDA 版をインストールする（Linux x86_64・Windows x64） |
| `KIKITORI_VERSION` | インストールする版を指定する（例 `0.3.0`。`v` は付けない） |
| `KIKITORI_INSTALL_DIR` | 実行ファイルの置き場を変える |
| `KIKITORI_NO_MODIFY_PATH=1` | PATH を書き換えない |
| `KIKITORI_YES=1` | 確認を省く（端末の無い環境向け） |

### あとから補正を使う

`KIKITORI_NO_REFINE=1` でインストールしたあとで補正を使うときは、`kikitori refine --setup` を実行します。`kikitori refine --setup --no-hf` は、B・K を取得せずに、LLM のモデルだけを用意します。そのときの B・K は初めて補正するときに取得してよいかを尋ねます。B・K だけを取得するには、`kikitori models pull --refine` を実行します。

### 端末の無い環境でインストールする

kikitori が取得や削除の前に出す確認の質問は、標準入力ではなく端末に直接出ます（macOS・Linux は `/dev/tty`、Windows は `CON`）。そのため、人が自分の端末でインストールのコマンドを実行すれば、パイプで渡していても確認の質問に答えられます。

エージェントの実行、コンテナ、CI のように端末が無い環境では、確認の質問に答えられないので、モデルを取得せずに実行ファイルだけがインストールされます。モデルまで取得するには、`KIKITORI_YES=1` を付けてインストールのコマンドを実行するか、あとから `kikitori --yes setup`（文字起こしのモデルを取得する。補正も使うなら、続けて `kikitori refine --setup --yes`）を実行します。インストールスクリプトは、モデルの取得が済まなくても終了コード 0 で終わります。何がインストールされたかは、`kikitori doctor` で確かめます。

## 更新

```bash
kikitori update            # 最新版に更新する。モデルは取得し直さない
kikitori update --dry-run  # 何を更新するかを確かめるだけ
```

- 新しい版が起動しないときは、実行ファイルを元の版に戻します。
- `kikitori skill install` で書き込んだスキルも、新しい版の内容に書き換えます。ただし 0.3.0 から更新したときは、スキルが 0.3.0 の内容のまま残るので、更新のあとで `kikitori skill install` を実行し直します（更新の前の版は `kikitori --version` で分かります）。
- 通常版と CUDA 版を切り替えるときは、`--variant cuda`（CUDA 版へ）か `--variant cpu`（通常版へ）を付けます。
- インストールのコマンドを実行し直しても更新できます。

## 削除

```bash
kikitori uninstall --dry-run   # 消すものと残すものの一覧を出すだけ
kikitori uninstall             # 一覧を出し、確認したうえで消す
```

インストールスクリプトと kikitori は、置いたものを 1 行ずつインストール記録（`receipt.txt`）に書きます。インストール記録の場所は `~/.config/kikitori/receipt.txt`（Windows は `%APPDATA%\kikitori\receipt.txt`）です。`kikitori uninstall` は、インストール記録と既定の置き場を見て消します。

消すもの:

- 実行ファイル（`kikitori-refine` を含む）と、PATH のために足した行（`# added by kikitori` の印が付いた行。Windows は利用者の環境変数）
- モデルと形態素解析の辞書（`~/.cache/kikitori`。Windows は `%USERPROFILE%\.cache\kikitori`）
- エージェント用のスキル
- kikitori が取得した B・K と、kikitori が Ollama に取得させた LLM のモデル。Ollama が止まっているときは、LLM のモデルを消すコマンドを出して残します。

残すもの:

- Ollama 本体と、利用者が自分で取得した Ollama のモデル。
- Hugging Face のキャッシュ（`~/.cache/huggingface/hub`。`HF_HOME` や `HF_HUB_CACHE` で場所が変わります）のうち、kikitori が取得していないもの。ほかのツールが先に取得していた B・K もここに入ります。`--dry-run` の「残すもの」に場所が出るので、要らなければそのフォルダを消します。
- 利用者が書いた辞書や用語集のファイル。

`--keep-models` を付けると、モデル・形態素解析の辞書・B・K・LLM のモデルを残し、実行ファイル、スキル、PATH の行だけを消します。利用者が既定の置き場の外に自分で置いたもので、インストール記録に無いものは、一覧に出ず、消えません。消す前に `--dry-run` の一覧を確かめてください。

## 旧版 0.2.x から移行する

0.2.x は Python で書かれた版です。`uv tool` でインストールした 0.2.x があると、インストールスクリプトが 0.2.x をアンインストールしてよいかを尋ね、アンインストールしてから新しい実行ファイルを置きます。端末の無い環境では、`KIKITORI_YES=1` を付けて実行します。pip など `uv tool` 以外の方法でインストールした 0.2.x は、インストールスクリプトがアンインストールしないので、先に `pip uninstall kikitori` などでアンインストールしてください。

文字起こしのモデルと B・K は 0.2.x と同じ置き場にあるので、取得し直しません。主な違いは次のとおりです。全部の一覧は [CHANGELOG.md](../CHANGELOG.md) にあります。

- `kikitori-refine` は `kikitori refine` になります。`kikitori-refine` の名前は、0.3.x の間だけ `kikitori refine` に転送する名前として残ります。
- `kikitori --setup` は `kikitori setup` に、`kikitori-refine --setup` は `kikitori refine --setup` になります。
- 失敗の終了コードは、1 から、種類ごとの 2〜11 になります。
- `--device` の値は `auto`・`cpu`・`metal`・`cuda` になります。文字起こしはいつも CPU で動き、GPU を使えるのは補正の B・K だけになります。
- 録音全体の出力は 0.2.x と完全には一致しません。音声の読み込み方が違うので区切りが変わり、本文と話者の割り当ても変わることがあります。
