# 導入・更新・削除

kikitori を入れる、更新する、取り外すときの詳しい説明です。概要は [README](../README.md) にあります。

## 動く環境

文字起こしはどの環境でも CPU で動きます。補正（`kikitori refine`）は、誤りを直す根拠にするため、同じ音声を文字起こしとは別の 2 つの音声認識モデルでも文字にします。この 2 つを B・K と呼びます。GPU を使うのは B・K だけです。補正の LLM は Ollama（手元で LLM を動かすソフト）が動かし、GPU の扱いも Ollama が決めます。GPU が無い PC でも、同じ使い方で全機能を使えます。

この文書と `kikitori doctor` は認識モデルを動かす装置（CPU か GPU）を計算機と呼びます。計算機は `--device` で選び、値は `auto`（既定）・`cpu`・`metal`・`cuda` です。`auto` は、Mac では Metal、CUDA 版では使えれば CUDA、それ以外では CPU を選びます。CUDA 版は NVIDIA の GPU 向けの実行ファイルで、それ以外を通常版と呼びます。

| OS | B・K を動かす計算機 | 確かめた範囲 |
|---|---|---|
| macOS（Apple Silicon） | Metal（Apple Silicon の GPU） | 導入、文字起こし、補正、削除 |
| Linux arm64 | CPU | 導入、文字起こし、補正 |
| Windows arm64 | CPU | 導入、文字起こし、削除 |
| Linux x86_64 | CPU | 起動と文字起こし（arm64 の PC 上のエミュレーション） |
| Windows x64 | CPU | 起動と文字起こし（arm64 の PC 上のエミュレーション） |
| Linux x86_64・Windows x64 の CUDA 版 | CUDA（NVIDIA の GPU）。条件を満たさなければ CPU | ビルドと、GPU の無い PC で CPU に切り替わるまで。NVIDIA の GPU を積んだ PC では確かめていません |

Windows での補正と、x86_64 の実機での動作は確かめていません。

自分の PC で何が動くかは `kikitori doctor` で分かります。OS、使える計算機と既定で使う計算機、取得済みのモデルと容量、ffmpeg と Ollama の有無、補正がどの直し方になる見込みか、空きメモリを出します。`kikitori doctor --json` は機械が読む形です。

### NVIDIA の GPU を使う

NVIDIA の GPU がある PC でも、既定で入るのは通常版で、B・K は CPU で動きます。GPU を使うには、導入のときに `KIKITORI_VARIANT=cuda` を付けて CUDA 版を入れます。CUDA 版が GPU を使うには、次の 3 つがそろっている必要があります。どれかが欠けていれば、CUDA 版も CPU で動きます。

- compute capability 8.0 以上の GPU（GeForce RTX 30 系、A100 以降）。自分の GPU の値は `nvidia-smi --query-gpu=compute_cap --format=csv,noheader` で分かります（例 `8.6`）。
- NVIDIA のドライバ。
- CUDA 12 の cuBLAS と cuRAND（NVIDIA の計算ライブラリ）。

CUDA 版を入れたあと、`kikitori doctor` の `cuda:` の行が「利用可能」なら GPU を使えます。この行はライブラリを読み込んで実際に初期化した結果です。

### 読める音声

m4a（AAC-LC、HE-AAC v1・v2）・mp4・mp3・wav・flac・ogg は、そのまま読めます。Google Meet の webm のような Opus の音声を読むには [ffmpeg](https://ffmpeg.org) が要ります。

## 導入

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh     # macOS・Linux
```

```powershell
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex            # Windows
```

導入スクリプトは次の順に進みます。

1. Python 版 0.2.x が `uv tool` で入っていれば、外してよいかを尋ねてから外します（[Python 版 0.2.x からの移行](#python-版-02x-からの移行)）。
2. GitHub の Release から、自分の環境の実行ファイルを取得し、チェックサムを確かめて置きます。チェックサムも同じ Release から取るので、確かめられるのは取得の途中で壊れていないことです。置き場は、macOS・Linux が `~/.local/bin`、Windows が `%LOCALAPPDATA%\Programs\kikitori` です。置き場が PATH に無ければ、PATH に足します。同じ置き場に、Python 版の名前 `kikitori-refine` も置きます。0.3.x の間だけ、`kikitori refine` に転送する名前です。
3. 文字起こしのモデル（約 750MB）と、`--dict`（固有名詞の表記を直す辞書）が語の読みを引くための形態素解析の辞書（取得は約 47MB、展開すると約 250MB）を取得します。`kikitori setup` と同じ働きです。
4. 補正の環境を用意します。`kikitori refine --setup` と同じ働きです。Ollama が無ければ公式の導入スクリプトで入れ、この PC に合う LLM のモデルと、B・K（約 6.8GB）を取得します。LLM のモデルはどの PC でも用語の置き換えに使う小さなモデル（0.8GB）を取得します。Apple Silicon で本体のメモリが、NVIDIA で GPU のメモリが 26GB 以上ある PC では、突き合わせに使う大きなモデル（18GB）も取得します（[機器とモデルの選び方](USAGE.md#機器とモデルの選び方)）。大きなモデルまで入ると、全部で約 27GB です。

置いたものは削除のために受け取り記録へ書き留めます（[削除](#削除)）。

手順 3 と 4 では、取得や導入の前に、何をするか（Ollama を入れることを含む）と容量を出して確認を求めます。すでに手元にあるモデルは取得し直しません。実行ファイルを入れたあとなら、`kikitori refine --setup --dry-run` で、何が入るかを入れずに見られます。

手順 3 か 4 が止まっても、導入スクリプトは「Note: kikitori setup stopped (exit 6) …」のように止まった手順と終了コードを出し、終了コード 0 で終わります。実行ファイルは入っていて、止まった手順のあとは入っていません。止まる主な場面は次のとおりです。

- 確認を断ったとき、または端末が無くて確認できないとき（終了コード 6。[端末の無い環境で入れる](#端末の無い環境で入れる)）。
- ディスクの空きが足りないとき（終了コード 7）。`kikitori setup` は約 1.6GB の空きを、`kikitori refine --setup` は取得する B・K と LLM のモデルの分の空きを、取得の前に確かめ、空きと要る量を出します。
- Ollama の導入に管理者権限が要る環境（主に Linux）で、権限が無いとき（終了コード 10）。kikitori の導入そのものに管理者権限は要りません。打つべきコマンドが出るので、管理者権限で打ってから `kikitori refine --setup` を打ち直します。

入れ終わったら、新しい端末を開いて（PATH の変更が反映されます）、1 本を起こしてみます。

```bash
kikitori transcribe 会議.m4a --num-speakers 4
```

### 導入の環境変数

環境変数はパイプの先の `sh` の前に付けます（`curl -fsSL …/install.sh | KIKITORI_NO_REFINE=1 sh`）。Windows では、先に `$env:KIKITORI_NO_REFINE = "1"` を打ってから導入の 1 行を打ちます。

| 環境変数 | 働き |
|---|---|
| `KIKITORI_NO_REFINE=1` | 補正の環境（B・K、Ollama、LLM のモデル）を入れず、文字起こしだけを入れる。取得は約 800MB |
| `KIKITORI_VARIANT=cuda` | CUDA 版を入れる（Linux x86_64・Windows x64） |
| `KIKITORI_VERSION` | 入れる版を決める（例 `0.3.0`。`v` は付けない） |
| `KIKITORI_INSTALL_DIR` | 実行ファイルの置き場を変える |
| `KIKITORI_NO_MODIFY_PATH=1` | PATH を書き換えない |
| `KIKITORI_YES=1` | 確認を省く（端末の無い環境向け） |

文字起こしだけを使うなら、補正の環境は要りません。あとから補正を足すときは `kikitori refine --setup` を打ちます。`kikitori refine --setup --no-hf` は B・K を取得せずに残りを用意し、B・K は初めて補正するときに取得してよいかを尋ねます。B・K だけを取得するには `kikitori models pull --refine` を打ちます。

### 端末の無い環境で入れる

kikitori の確認は、標準入力ではなく端末に直接尋ねます（macOS・Linux は `/dev/tty`、Windows は `CON`）。導入スクリプトが Python 版を外してよいかを尋ねる問いも同じです。そのため、人が自分の端末で導入の 1 行を打てば、パイプで渡していても確認に答えられます。

エージェントの実行、コンテナ、CI のように端末が無い環境では、確認を待たずに終了コード 6 で止まり、導入スクリプトは上のとおり終了コード 0 で終わります。端末の無い環境では、`KIKITORI_YES=1` を付けて導入の 1 行を打つか、あとから `kikitori --yes setup`（補正も使うなら続けて `kikitori refine --setup --yes`）を打ちます。導入が済んだかは、導入スクリプトの終了コードでは分からないので、`kikitori doctor` で確かめます。

### Intel の Mac

Intel の Mac 向けの実行ファイルはなく、導入スクリプトは「Intel Macs are not supported by kikitori 0.3」と出して止まります。Intel の Mac では Python 版 0.2.x を使ってください。初めて入れるときは次の 1 行です。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/download/v0.2.1/install.sh | sh
```

## 更新

```bash
kikitori update            # 最新版へ。モデルは取り直さない
kikitori update --dry-run  # 何を更新するかを見るだけ
```

`kikitori update` は、新しい版が起動しないときは元の版に戻して終わります。`kikitori skill install` で書いたスキルも、新しい版の内容に書き換えます。ただし 0.3.0 から更新したときは、スキルが 0.3.0 の内容のまま残るので、更新のあとで `kikitori skill install` を打ち直します。通常版と CUDA 版を替えるときは、`--variant cuda`（CUDA 版へ）か `--variant cpu`（通常版へ）を付けます。導入の 1 行を打ち直しても更新できます。

## 削除

```bash
kikitori uninstall --dry-run   # 消すものと残すものを一覧にするだけ
kikitori uninstall             # 一覧を出し、確認のうえで消す
```

導入スクリプトと kikitori は、自分が置いたものを 1 行ずつファイルに書き留めます。このファイルを受け取り記録と呼び、場所は `~/.config/kikitori/receipt.txt`（Windows は `%APPDATA%\kikitori\receipt.txt`）です。`kikitori uninstall` は、受け取り記録と既定の置き場を見て消します。

消すもの:

- 実行ファイル（`kikitori-refine` を含む）と、PATH のために足した行（`# added by kikitori` の印が付いた行。Windows は利用者の環境変数）
- モデルと形態素解析の辞書（`~/.cache/kikitori`。Windows は `%USERPROFILE%\.cache\kikitori`）
- エージェント用のスキル
- kikitori が取得した B・K と、kikitori が取得した Ollama のモデル。Ollama が止まっているときは、Ollama のモデルを消すコマンドを案内して残します

残すもの:

- Ollama 本体。要らなければ [Ollama](https://ollama.com) の案内に従って消します。
- 利用者が自分で入れた Ollama のモデル。
- Hugging Face のキャッシュ（`~/.cache/huggingface/hub`。`HF_HOME` や `HF_HUB_CACHE` で場所が変わります）のうち、kikitori が取得していないもの。Python 版やほかの道具が先に取得していた B・K もここに入ります。`--dry-run` の「残すもの」に場所が出るので、要らなければそのフォルダを消します。
- 利用者が書いた辞書や用語集のファイル。

`--keep-models` を付けると、モデル・形態素解析の辞書・B・K・Ollama のモデルを残し、実行ファイル、スキル、PATH の行だけを消します。利用者が既定の置き場の外に手で置いたもので、受け取り記録に無いものは、一覧に出ず、消えません。消す前に `--dry-run` の一覧を確かめてください。

## Python 版 0.2.x からの移行

pip など `uv tool` 以外の方法で入れた Python 版は、導入スクリプトが外しません。先に `pip uninstall kikitori` などで外します。外さないと、PATH で先に見つかる Python 版を表示して、導入スクリプトが終了コード 8 で止まります。

Python 版を `uv tool` で入れてある PC では、導入の 1 行で入れ替えられます。導入スクリプトが Python 版を外してよいかを尋ね、外してから新しい実行ファイルを置きます。端末の無い環境では、`KIKITORI_YES=1` を付けて打ちます（`curl -fsSL …/install.sh | KIKITORI_YES=1 sh`）。文字起こしのモデルは置き場が同じなので、取り直しません。Python 版が取得した B・K も、Hugging Face のキャッシュにあるものをそのまま使います。

主に変わるのは次の点です。全部の一覧は [CHANGELOG.md](../CHANGELOG.md) の移行の表にあります。

- `kikitori-refine` は `kikitori refine` になります。`kikitori-refine` の名前は、0.3.x の間だけ転送する名前として残ります。
- `kikitori --setup` は `kikitori setup` に、`kikitori-refine --setup` は `kikitori refine --setup` になります。
- 失敗の終了コードは、1 から、種類ごとの 2〜11 になります。
- `--device` の値は `auto`・`cpu`・`metal`・`cuda` になります。文字起こしはいつも CPU で動き、GPU を使えるのは補正の B・K だけになります。
- 認識モデルの出力は、公開の短い音声 38 本で Python 版と一致しています。録音全体の出力は完全には一致しません。音声の読み込み方が違うので区切りが変わり、本文と話者の割り当ても変わることがあります。
