# インストール・更新・削除

取得・インストール・更新・削除はどれも利用者の了承を得てから実行します。了承を得る前に、何を入れるか・消すかと容量を伝えます。kikitori が入っていれば `--dry-run` の出力で、まだ入っていなければ下の「何を入れるかを決める」の表の容量で伝えます。

## 入っているかを確かめる

`kikitori --version` を実行します。0.3.0 以上が返れば入っています。

`kikitori` が見つからないときは、入れた直後で PATH が今のシェルに反映されていないだけのことがあります。`~/.local/bin/kikitori --version`（Windows は `%LOCALAPPDATA%\Programs\kikitori\kikitori.exe --version`）を絶対パスで実行します。それでも無いか、0.2.x（Python 版。`kikitori transcribe` が無い）が返るときは、下の手順で入れます。0.2.x はインストールのコマンドで置き換わり、モデルは取り直しません。

## Intel の Mac を見分ける

macOS では、インストールする前に `uname -sm` を実行します。`Darwin x86_64` なら Intel の Mac で、kikitori は対応していません。インストールせずに、そのことを利用者に伝えます。

## 何を入れるかを決める

| 入れ方 | 入るもの | 容量 |
|---|---|---|
| 文字起こしだけ | 実行ファイル、文字起こしのモデル、形態素解析の辞書（`--dict` が読みを引く） | 約 800MB |
| 補正の環境ごと | 上に加え、補正の認識モデル B・K と、Ollama が動いていれば PC に合う LLM のモデル | 最大約 27GB |

補正の環境は、利用者が補正を使うと言ったときだけ入れます。容量が大きく、文字起こしだけなら要らないためです。誤変換を直したいと言われただけなら、先に文字起こしだけを入れて `--dict` を試し、辞書で直らなかったときに補正の環境を入れる了承を得ます。補正の環境はあとから `kikitori refine --setup` で足せます。kikitori は Ollama（手元で LLM を動かすソフト）を入れも起動もしません。入れ方は利用者ごとに違うので、Ollama を入れるかどうかと入れ方は利用者が決めます。LLM で直したいのに Ollama が無ければ、利用者に入れて起動してもらってから、`kikitori refine --setup` を実行し直します。

LLM のモデルの大きさは PC で変わります。どの PC でも小さなモデル（0.8GB）を取得し、Apple Silicon で本体のメモリが、NVIDIA で GPU のメモリが 26GB 以上あれば、大きなモデル（18GB）も取得します。入れる前に正確な量を知るには、実行ファイルを入れたあとで `kikitori refine --setup --dry-run` を実行します。

## 入れる

了承を得たら、OS と入れ方に合う 1 行を実行します。あなたの実行にはターミナルが無く、確認で止まるので、`KIKITORI_YES=1` を付けます。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | KIKITORI_YES=1 sh                        # macOS・Linux、補正の環境ごと
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | KIKITORI_YES=1 KIKITORI_NO_REFINE=1 sh   # macOS・Linux、文字起こしだけ
```

```powershell
$env:KIKITORI_YES = "1"; irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex   # Windows。文字起こしだけなら先に $env:KIKITORI_NO_REFINE = "1"
```

- kikitori のインストールに管理者権限は要りません。
- インストールスクリプトは、途中の取得が止まっても終了コード 0 で終わり、「Note: kikitori setup stopped (exit 6) …」のように止まった手順と終了コードを出します。そのときは実行ファイルだけが入っています。済んだかはインストールスクリプトの終了コードでは分からないので、入れたあとは `kikitori doctor` で確かめます。止まった手順は `kikitori --yes setup` と `kikitori refine --setup --yes` で打ち直せます。
- 空きが足りないと終了コード 7 で止まり、空きと要る量が出ます。`kikitori setup` は約 1.6GB の空きを求めます。
- 個別に取得するコマンドは、文字起こしのモデルが `kikitori setup`、B・K が `kikitori models pull --refine`、LLM のモデル（動いている Ollama に取得させる）が `kikitori refine --setup` です。

## NVIDIA の GPU（CUDA 版）

CUDA 版は、利用者が補正を NVIDIA の GPU で動かしたいと言ったときだけ、Linux x86_64・Windows x64 で入れます。速くなるのは補正の B・K だけで、文字起こしは CPU のままです。また、CUDA 版は NVIDIA の GPU を積んだ PC では確かめていないので、そのことを利用者に伝えます。

1. `nvidia-smi --query-gpu=compute_cap --format=csv,noheader` を実行します。動かないか、出た値が 8.0 未満なら、GPU では動かないので CUDA 版は入れません。
2. 8.0 以上なら、インストールのコマンドに `KIKITORI_VARIANT=cuda` を足して入れます。入れたあとで替えるなら `kikitori update --variant cuda --yes` です。
3. `kikitori doctor` の `cuda:` の行が「利用可能」なら GPU を使えます。使えないとき（ドライバや CUDA 12 の cuBLAS・cuRAND が無いとき）は、B・K は CPU で動きます。

## 更新

```bash
kikitori update --dry-run       # 何を更新するかを見る
kikitori update --yes           # 最新版へ。モデルは取り直さない
```

新しい版が起動しなければ、元の版に戻して終了コード 8 で終わります。`kikitori skill install` で書いたスキルも新しい版の内容に書き換わります。0.3.0 から更新したときだけはスキルが古いまま残るので、更新のあとで `kikitori skill install`（Codex なら `--agent codex`）を実行し直します。0.2.x には `kikitori update` が無いので、インストールのコマンドを実行し直します。実行する行は「何を入れるかを決める」と同じ考えで選び、利用者が 0.2.x で補正を使っていなかったなら、文字起こしだけの行にします。Intel の Mac では 0.2.1 が最新なので、更新するものはありません。

## 削除

```bash
kikitori uninstall --dry-run             # 消すものと残すものを一覧にする
kikitori uninstall --yes                 # 消す
kikitori uninstall --yes --keep-models   # モデルを残して消す
```

`--dry-run` の一覧を利用者に見せて了承を得てから、`--yes` を付けて実行します。`--keep-models` は、利用者がモデルを残したいと言ったときに付けます。

- 消えるのは、kikitori が置いたものです。実行ファイル、PATH に足した行、文字起こしのモデルと形態素解析の辞書、スキル、kikitori が取得した B・K と Ollama のモデルが当たります。
- 残るのは、Ollama 本体（kikitori は入れていない）、利用者が自分で入れた Ollama のモデル、kikitori が取得していない Hugging Face のキャッシュ、利用者が書いた辞書と用語集です。利用者がそれも消したいと言ったときは、`--dry-run` の「残すもの」に出るフォルダを消すか、Ollama の案内に従ってもらいます。
- Ollama が止まっていると、kikitori が取得した Ollama のモデルは消せずに残り、`ollama rm <名前>` が案内されます。Ollama を起動してから、その行を実行すれば消えます。

0.2.x には `kikitori uninstall` がありません。0.2.x は `uv tool uninstall kikitori` で外します（`kikitori` と `kikitori-refine` の両方が消えます）。モデルは残るので、利用者が要らないと言えば、文字起こしのモデルの `~/.cache/kikitori/models` と、Hugging Face のキャッシュ（`~/.cache/huggingface/hub`）の中の `models--neosophie--Qwen3-ASR-1.7B-JA` と `models--sbintuitions--kana-whisper` を消します。Hugging Face のキャッシュにはほかの道具のモデルもあるので、フォルダごとは消しません。

## スキルを入れる

利用者に頼まれたときに、`kikitori skill install`（Codex なら `--agent codex`）で、このスキルをエージェントのスキルの置き場に書きます。置き場は `kikitori skill path` で分かり、`kikitori skill uninstall` で消せます。
