---
name: kikitori
description: 日本語の会議録音（Web 会議・会議室）を、話者ごとに分けてこの PC の中で文字起こしする CLI「kikitori」（Rust 製の 1 本の実行ファイル）の使い方。音声や動画のファイル（m4a/mp4/mp3/wav/flac/ogg など）を文字に起こしたい、発言を話者別に分けたい、固有名詞の表記を辞書で直したい、文字起こしの誤りを補正（kikitori refine）したい、kikitori を入れる・診断する・更新する・削除するときに使う。Hugging Face の認証は不要。kikitori transcribe は LLM を使わず、kikitori refine が別の認識結果と LLM（手元の Ollama、呼び出し元の LLM、claude）で本文を直す。
---

# kikitori：日本語の録音を話者つきで文字起こしする

音声ファイルを、話者ごとの発言（開始時刻・終了時刻・話者番号・本文）に起こします。処理はこの PC の中で完結し、Hugging Face のトークンは要りません。
補正（`kikitori refine`、後述）は、文字起こしの誤りを直す別のコマンドです。文字起こしの本文を A、根拠にする別の 2 つの認識モデルを B（Qwen3-ASR）・K（音をカタカナで書く kana-whisper）と呼びます。直す単位は区間（文字起こしが区切った発言のひとまとまり。txt の 1 行、json・yaml の 1 要素）です。用語集（会議に出る社名・地名・製品名を書いた一覧）は補正の節にあります。
`kikitori transcribe` は要約や整形をしません。要約や整形が要るときは、利用者に頼まれてから、結果を受け取ったあなた（エージェント）が行ってください。

対応する言語は日本語です。英語が混ざる会議は `--model ja-en` で起こせます。`kikitori refine` の補正は日本語専用です。

## 打ち方の決まり

- 文字起こしはいつも `kikitori transcribe <音声>` と打ってください。`kikitori <音声>` でも動きますが、音声のファイル名が `doctor` のようなコマンド名と同じとき、コマンドとして読まれます。
- 結果は **stdout**、進み具合と警告は **stderr** です。
- 機械で読むときは `--log-format json` を付けます。stderr が 1 行 1 件の JSON になり、種類は `progress`・`info`・`warning`・`error`・`summary` です。stdout は選んだ出力形式だけのままです。
- エージェントの実行は、端末（人が対話で打ち込む TTY）が無いものとして扱ってください。確認が要る操作は、取得（大きさによらずどれも）、削除、更新です。端末が無いと、確認を待たずに終了コード 6 で終わります。入れるものと容量を利用者に伝えて了承を得てから、`--yes` を付けて打ち直します。
- 実行内容だけを見たいときは `--dry-run` を付けます。何も変更しません。
- ネットワークを使わせたくないときは `--offline`（`HF_HUB_OFFLINE=1` も効きます。モデルが足りないと終了コード 5）。`-v` で詳しいログが出ます。

## 終了コードと hint

| コード | `code` | 意味 | 次の手 |
|---|---|---|---|
| 0 | `success` | 正常終了 | 補正が長さの規則（LLM を使わず、B の文字数が A の 1.6 倍＋3 文字を超える区間だけ B を採る直し方。補正の節）に下がったときも 0 です。stderr の警告を読みます |
| 2 | `usage` | 引数の誤り | コマンドと旗を `--help` で確かめます |
| 3 | `input` | 入力がない、読めない、形式が不正 | パスと形式を確かめます |
| 4 | `dependency` | 必要な依存ソフトがない | 多くは ffmpeg です。入れ方は `hint` に出ます |
| 5 | `offline_model` | オフラインでモデルを使えない | `--offline` を外すか、先に `kikitori setup` を打ちます |
| 6 | `needs_confirmation` | 確認が必要 | 利用者の了承を得て `--yes` を付けます |
| 7 | `download` | ダウンロード失敗 | 通信の失敗なら、通信を確かめて打ち直します（途中からの再開は自動です）。空き不足（`hint` が「ディスクを空けてから…」）なら、利用者にディスクを空けてもらってから打ち直します。取得の前の空きの確認でも、保存中に空きが尽きたときでも 7 です |
| 8 | `processing` | 処理失敗 | `-v` を付けて理由を見ます。直らなければ `kikitori doctor` の出力を利用者に見せます |
| 9 | `refine` | `--external claude` を指定したのに `claude` コマンドが見つからない | `--external claude` は利用者が外部の LLM を使うと言ったときだけ打つので、その範囲で、`hint` のとおり Claude Code を入れる（了承を得てから）か、`--external bundle` に替えます（束はファイルに書かれるだけで、どこにも送られません） |
| 10 | `privilege` | 管理者権限が必要（主に Linux で Ollama を入れるとき） | `hint` のコマンドを利用者に頼みます。あなたが権限を上げて打たないでください |
| 11 | `memory` | メモリ不足の見込み | `kikitori refine` で出ます。`hint` のとおり `--dtype fp16`（B を fp16 にして約 5.6GB。CPU でも指定できます）を試すか、利用者に了承を得て `--force`（見積もりを無視して続ける）を付けます |

`--log-format json` のとき、失敗は `type` が `error` の 1 行で出ます。`code`（終了コードの名前）・`message`・`hint` を持ちます。分岐は終了コードか `code` の名前（上の表）で行います。想定外の異常（panic）は、`code` が `internal` で終了コード 8 です。`hint` は次に打つコマンドです（人向けの出力では、失敗の表示に添えられます）。利用者の了承が要る操作（導入、取得、削除、更新）でなければ、そのまま打ってかまいません。表の次の手のうち、取得（コード 5 の `kikitori setup`、7 の打ち直し）と導入（9 の Claude Code を入れる）は了承を得てから打ちます。

## 導入

まず `kikitori --version` を打ちます。0.3.0 以上が返れば導入済みです。`kikitori` コマンドが無いときは、導入済みでも PATH が今のシェルに反映されていないだけのことがあるので、`~/.local/bin/kikitori --version`（Windows は `%LOCALAPPDATA%\Programs\kikitori\kikitori.exe`）を絶対パスで打ちます。それでも無いか、0.2.x（Python 版。`kikitori transcribe` が無い）が返るときは、利用者に入るもの（実行ファイル、文字起こしのモデル約 750MB と形態素解析の辞書約 47MB、補正の環境を入れるならその容量）をまとめて伝え、1 回の了承を得てから、下の 3 行のうち OS と目的に合う 1 行で入れます（0.2.x は置き換わります）。macOS・Linux で補正の環境ごと入れるなら 1 行目、文字起こしだけなら 2 行目、Windows は 3 行目です（補正を入れるかは、下の「入るもの」の項で決めます）。kikitori 本体の導入に管理者権限は要りません。
**Intel の Mac では打たないでください。** macOS では先に `uname -sm` を打ち、`Darwin x86_64` なら Intel の Mac、`Darwin arm64` なら Apple Silicon です。0.3 は Intel の Mac に対応せず、打つと止まります。何も入っていないときは、利用者に「Intel の Mac は 0.2.x（Python 版）を使う」と伝え、了承を得て `curl -fsSL https://github.com/kulikala/kikitori/releases/download/v0.2.1/install.sh | sh` で入れます。0.2.x には `transcribe` が無く、`kikitori <音声>` と打ちます。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh                          # macOS・Linux
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | KIKITORI_NO_REFINE=1 sh    # 文字起こしだけ
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex                              # Windows（PowerShell）
```

- 入るもの：実行ファイルと、文字起こしのモデル（約 750MB）。既定では補正の環境（認識モデル B・K 約 6.8GB、[Ollama](https://ollama.com)（手元で LLM を動かすソフト）、PC に合う LLM のモデル）も入り、全部で最大約 27GB です。利用者が補正を使うと言っているときだけ、補正の環境ごと入れます。補正を使うか依頼から読めないときは、補正の節のとおり確かめ、使うと分かったときだけ補正の環境ごと入れます。誤変換を直したいと言われただけなら、先に `--dict`（下の「精度を上げる」）を試し、辞書で直らなかったときに、補正の環境を入れる了承を得ます。それ以外は `KIKITORI_NO_REFINE=1` を付けて文字起こしだけを入れ、補正が要るときに `kikitori refine --setup` を打ちます。
- 環境変数はパイプの先の `sh` に付けます。Windows は先に `$env:KIKITORI_NO_REFINE = "1"` を打ちます。
- Python 版 0.2.x が入っているときは、導入の 1 行がそれを外してから入れます。モデルは取り直しません。
- CUDA 版は、利用者が補正の B・K を NVIDIA の GPU で動かしたいと言っている Linux x86_64・Windows x64 でだけ、`KIKITORI_VARIANT=cuda` を付けて入れます。速くなるのは補正の B・K だけで、文字起こしの本体は CPU のままです。付ける前に、`nvidia-smi --query-gpu=compute_cap --format=csv,noheader` を打ちます（`nvidia-smi` が動かなければ NVIDIA の GPU は無い、または使えません）。出た値（例 `8.6`）が 8.0 未満なら、GPU では動かないので `cuda` は付けません。8.0 以上なら、GPU で動くにはさらに、その GPU のドライバと CUDA 12 の cuBLAS・cuRAND が要ります。これらがそろっているかは、入れたあと `kikitori doctor` の `cuda:` の行で分かります（「利用可能」なら使えます。cuBLAS・cuRAND の読み込みを含めて実際に初期化して確かめます）。使えなければ B・K は CPU で動くだけです。Linux x86_64 の CUDA 版は GPU の無い PC で CPU に切り替わるところまで、Windows x64 の CUDA 版は GPU の無い PC で起動して CUDA を使えないと判定するところまで確かめてあり、どちらも NVIDIA の実機では確かめていません。そのため、利用者が NVIDIA の GPU で動かしたいと言い、値が 8.0 以上のときだけ付け、実機で確かめていない版であることを利用者に伝えます。それ以外では付けません。
- 端末が無い環境では、導入スクリプトの `kikitori setup`（文字起こしのモデルの取得）が確認を待たずに終了コード 6 で止まります。補正の環境ごと入れる行では、続く `kikitori refine --setup` も、確認まで進めば終了コード 6 で止まります（その前に空きなどが足りなければ、そのことを出して止まります）（文字起こしだけの行では、止まるのは `kikitori setup` だけです）。そのとき導入スクリプトは「Note: kikitori setup stopped (exit 6) …」のように Note を出して終了コード 0 で終わります。入っているのは実行ファイルだけで、文字起こしのモデルも補正の環境も入っていません。上の了承を得たうえで、最初から `KIKITORI_YES=1` を付けて導入の 1 行を打つか、あとで `kikitori --yes setup` を打ち、補正も使うなら `kikitori refine --setup --yes` も打ちます。`--yes` はどのコマンドにも付けられる、確認を省く旗です。
- 入れた直後は、PATH が今のシェルに反映されていないことがあります。新しいシェルを開くか、`~/.local/bin/kikitori`（Windows は `%LOCALAPPDATA%\Programs\kikitori\kikitori.exe`）を絶対パスで打ちます。
- 補正の環境ごと入れる Linux では、Ollama の導入に管理者権限が要ることがあります。そのときは終了コード 10 で止まります。`hint` のコマンドを利用者が打ったら、導入の 1 行を打ち直します（補正の環境だけなら `kikitori refine --setup`）。
- 取得の前に、空き容量が確かめられます。`kikitori setup` は、取得する大きさ（文字起こしのモデル約 750MB と形態素解析の辞書約 47MB で、合計約 796MB）の 2 倍（約 1.6GB）の空きを、`kikitori refine --setup` は Ollama のモデルと B・K の分の空きを確かめます。B・K は約 6.8GB（B 約 3.8GB、K 約 3.0GB）で、Ollama のモデルは別です（突き合わせ用の大きなモデルは 18GB、tev1 は 0.8GB。どれが入るかは `kikitori refine --setup --dry-run` で、入れるものと容量が出ます）。足りなければ終了コード 7 で止まり、空きと要る量が出ます（ディスクを空けてから打ち直します）。
- 入れたあとの確かめは `kikitori doctor` です（後述の「診断」）。

モデルを個別に取得するコマンドは次のとおりです。

| 取得したいもの | コマンド |
|---|---|
| 文字起こしのモデル | `kikitori setup`（`kikitori models pull --core` と同じ） |
| 補正の認識モデル B・K | `kikitori models pull --refine` |
| 補正の Ollama と LLM のモデル | `kikitori refine --setup`（`--dry-run` で入れるものと容量を先に見られる） |

## 文字起こし

```bash
kikitori transcribe 会議.m4a --num-speakers 4 --format json-v1 > 会議.json   # LLM で読むなら json-v1
kikitori transcribe 会議.m4a --dict 辞書.tsv                                 # 辞書つき（テキスト出力）
kikitori transcribe 会議.m4a --format srt > 会議.srt
kikitori transcribe 会議.m4a --model ja-en                                   # 日英が混ざる会議
```

- 音声の形式：m4a・mp4・mp3・wav・flac・ogg は ffmpeg なしで読めます。Opus（Google Meet の webm など）は ffmpeg が要ります。無ければ終了コード 4 で、入れ方が `hint` に出ます。
- モデルが無ければ、最初の実行で取得します（約 750MB）。端末が無い環境で終了コード 6 になるのを避けるには、利用者の了承を得て、先に `kikitori --yes setup` で取得しておきます。
- 録音が長いときは先に `ffmpeg -t 300 -i 会議.m4a 先頭.m4a` で 5 分だけ切って試すと（ffmpeg が無ければ、切らずに実行するか、利用者の了承を得て入れます）、人数・辞書・出力形式の指定を確かめられます。67 分の合成会議を Apple Silicon の Mac の CPU で話者つきで起こすと 279.7 秒でした。
- 無音だけの録音は空の結果で終了コード 0 です。

### 出力形式（`--format`）

| 形式 | 使うとき |
|---|---|
| `json-v1` | **LLM や道具で読むときの第一候補**。`schema_version` と `segments` を持つオブジェクト |
| `json`・`yaml` | 発言の配列。互換のために残してあります。yaml は日本語をそのまま書き、短い |
| `txt`（既定） | 細かい時刻が要らない。1 行 1 発言の `[分:秒.小数] 話者N: 本文`（例 `[00:06.43] 話者2: …`） |
| `srt`・`vtt` | 字幕の道具に渡す。時刻の行と本文の行が分かれていて、発言ごとの対応を読み取りにくいので、LLM で読むときは使わない |

各発言は次の 4 つを持ちます。

- `start`・`end`：秒
- `speaker`：0 から数える話者番号（txt・srt・vtt の「話者1」が `0`）。人物名ではありません。名前との対応づけは結果を読む側の仕事です。
- `text`：本文。句読点は付きません。

形の定義は `kikitori schema transcript-v1` で JSON Schema として出せます（ほかに `transcript`・`bundle`・`doctor`・`engines`）。

### 精度を上げる

- `--num-speakers N`：参加者の数が分かるなら指定してください。指定がないと、話者を分けすぎたりまとめすぎたりします。指定の人数に分けられなかったときは、stderr に警告が出ます。
- `--dict`（利用者が書く辞書のファイル）：1 行 1 語、`表記<TAB>読み`（読みはひらがなでもカタカナでも可）。誤認識された固有名詞を、認識結果の語の読み（発音。一致しなければ綴りの読み）が辞書の読みと一致したときに、表記へ直します。置き換えた区間の数は stderr に出ます。読みを引く形態素解析の辞書（unidic-lite。`--dict` のファイルとは別物で、取得は約 47MB、展開後は約 250MB）は、`kikitori setup` が取得します。これが無いまま `--dict` を付けると、大きさによらず取得の確認が出て、端末が無ければ終了コード 6 で止まるので、利用者の了承を得て `--yes` を付けます（`--offline` なら 5）。読みが違う語は置き換わらないので、会議に出そうな語を多めに書いてかまいません。モデルへの語彙の注入ではなく、後段の置換です。辞書で直らなかったときは（stderr が「0 区間を置き換えました」など）、辞書の読みを誤認識の語の読みに合わせて書き換えることはせず、辞書の限界を利用者に報告し、補正を使ってよいかを確かめてから、用語集（補正の節）と `refine` で直します。補正の環境が入っているかは `kikitori doctor` で分かります。未導入なら、導入（最大約 27GB）の了承も同じときに得ます。**音そのものが違う誤認識（「東京リージョン」が「東京水温」になるなど）は辞書では直りません。** その語を用語集に書いて `kikitori refine --glossary` を使います。
- **Zoom の「参加者ごとに個別のオーディオファイルを記録」** を使うと、話者分離が要らなくなり、いちばん正確です。話者分離を省く旗はないので、`--num-speakers 1` を付けてファイルごとに実行します。録音の開始が同じ時刻にそろっているかは利用者に確かめ、そろっているときだけ `start` の順に並べます。分からなければ、ファイルごとの結果をそのまま渡します。話者番号はどのファイルも `0` になるので、人物はファイル名から対応づけます。
- `--model ja-en`：英語の発言が多い会議で試します。補正（`kikitori refine`）は日本語専用なので、補正と日英混在の両方が要るときは、補正をあきらめて `transcribe --model ja-en` の結果を渡し、補正できないことを利用者に伝えます。既定の `ja`（日本語専用）は、話されていない英語の語が紛れ込みにくい代わりに、英語を認識しません。

### 計算機

計算機は認識モデルを動かす装置のことです。`--device` の値で選び、`cpu`（CPU）、`metal`（Apple Silicon の GPU）、`cuda`（NVIDIA の GPU）があります。`--device auto`（既定）は使える計算機を調べ、使えなければ CPU に戻ります。文字起こし本体はいつも CPU で動き、GPU を使うのは補正の B・K だけです。通常版では、Mac（Apple Silicon）向けの配布物で B・K が Metal を使い、Mac 以外では CPU です。CUDA 版では、B・K は CUDA を使い、使えなければ CPU です。選ばれた計算機と理由は `-v` か `kikitori doctor` で見られます。

## 補正（kikitori refine）

補正を使うか、人数（分からなければ省く）、英語の発言が多いか（多ければ `ja-en`）、用語集（会議の資料などにある社名・地名・製品名）は、利用者の意図なので、依頼から読めなければ確かめます。外部の LLM（`--external claude`、`--external bundle`、あなたが直す手順）は、本文（A・B・K）と用語集を PC の外へ渡します。渡してよいかは利用者が決めることで、利用者が外部の LLM を使うと言っていなければ使いません。

`kikitori refine` は、文字起こしの本文（A）の誤変換・取りこぼしを直します。区間ごとに、同じ音声を B と K でも文字にし、それを根拠に直します。日本語の会議だけに対応します。

```bash
kikitori refine --plan --glossary 用語集.tsv                                  # どの直し方になるかを出すだけ（音声は省ける）
kikitori refine 会議.m4a --glossary 用語集.tsv --format json-v1 > 会議.json   # 手元で直す
```

- 用語集は 1 行 1 語、`表記<TAB>補足`（補足は読みや説明。省ける）です。会議に出る社名・地名・製品名を書きます。`--dict` の辞書（`表記<TAB>読み`）も同じ形なので、そのまま用語集に使えます。
- **refine は文字起こしをやり直します。** `transcribe` と同じ `--num-speakers`・`--dict` などを付けてください（`--model` は `ja` だけです。`ja-en` を付けると終了コード 2 で止まり、`kikitori transcribe --model ja-en` を案内します）。付けないと、話者番号や区間の数が `transcribe` の結果と食い違います。
- 直し方は環境（B・K の有無、Ollama にあるモデル、指定した旗）で決まります。`--plan` は、音声を省いて打てます。`--glossary` などの旗は本番と同じものを付けます。`--plan` は、直し方（kikitori の本文のまま／長さの規則／長さの規則＋tev1 で用語の置き換え／Ollama の大きなモデルで突き合わせ／外部 LLM）と使うモデルを出して終わります。tev1 は Ollama で動く、選択肢から 1 つを選ぶ小さなモデル（0.8GB）です。出た直し方が利用者の望みに足りるか分からなければ利用者に確かめ、よければ同じ旗から `--plan` を外して本番を打ちます。直し方を変えたいときは、下の項目の `kikitori refine --setup`（Ollama と LLM のモデルを入れる。導入なので了承が要ります）か、外部 LLM（`--external`。利用者が使うと言っているときだけ）に進みます。Ollama に突き合わせ用の大きなモデル（Apple Silicon は `qwen3.8:27b-mlx`、NVIDIA は `qwen3.8:27b-q4_K_M`、どちらも 18GB。モデルを置くメモリ（LLM のモデルを載せるメモリ。Apple Silicon は本体のメモリ、NVIDIA は最も大きい GPU 1 枚の GPU メモリ、それ以外は本体のメモリ）が「モデルの大きさ＋8GB」以上、つまり 26GB 以上のときだけ使われ、Apple Silicon の 16GB や NVIDIA の 24GB では tev1 だけになります。CPU だけの機器では使われません）があれば、区間ごとに A・B・K を突き合わせて書かせます。tev1 しか無ければ、用語集の語に置き換えるかを選ばせます（`--glossary` が要る）。この PC で突き合わせ用のモデルが使えるかは、`--plan` が出す直し方（「Ollama の大きなモデルで突き合わせ」になるか）で分かります。Ollama が答えないだけなら、終了コード 0 のまま警告を出して、長さの規則で直します。選ばれた直し方と理由は stderr に出ます。
- `kikitori refine` は Ollama を起動しません。Ollama が止まっている、または答えない、LLM のモデルが無いときは、エラーにならず、長さの規則などに切り替えて終了コード 0 で終わります。Ollama が答えないので長さの規則で直したことは stderr に出るので、利用者に伝えます。どの直し方になったかと理由は stderr（`--log-format json` では `info` / `warning`）に出ます。LLM を使いたいときは `kikitori refine --setup` で入れます（Ollama を入れて起動します）。入れる前に `--dry-run` で容量を見て、利用者に了承を得ます。Ollama を使わずに済ませるなら、長さの規則か、次の外部 LLM を使います。
- B・K のモデルが無いと、取得するかを確かめます。端末が無ければ終了コード 6 で止まるので、利用者の了承を得て `--yes` を付けます（先に `kikitori models pull --refine` でも取得できます）。`--offline` では終了コード 5 です。人が端末で取得を断ったときは、補正せず本文のまま（終了コード 0）です。エージェントの実行では `--yes` で取得するか、取得せず終了コード 6（`--offline` なら 5）で止まるかのどちらかです。
- Ollama は 0.40.0 以上が要ります。古いと `--setup` が止まり、更新の仕方を出します。
- `--external claude` には `claude` コマンドが PATH に要ります。
- 補正は文字起こしより時間がかかります。B・K の分だけで、CPU では音声の長さの約 1.5 倍（Linux arm64 で 378.6 秒の音声に B・K 合わせて約 580 秒）、Mac の Metal では約 0.15 倍（同じ音声に約 54 秒）でした。これに文字起こしの時間が加わります。使う計算機は `kikitori doctor` の「使う計算機（既定）」の行で分かります。コマンドの実行時間に上限がある環境では、この目安で足りるかを先に見積もります。
- B・K を読み込む直前にメモリを見積もります（B を fp32 にすると約 9.7GB、B を fp16 にすると約 5.6GB。K はいつも fp32 で、大きい B で決まります。Metal の既定は B が fp16、CPU でも `--dtype fp16` で B を fp16 にできます。CUDA では見積もりません）。使えるメモリが足りないと終了コード 11 で止まります。
- Ollama のモデルは `--merge-model <名前>`（別名 `--llm`、環境変数 `KIKITORI_LLM`）で指定できます。

### 呼び出し元の LLM（あなた）が直す

あなたが直すと、本文があなたの動いている LLM のサービスへ渡ります。利用者がそれでよいと言っているときだけ行います（言っていなければ、打つ前に確かめます）。Ollama で直すより、あなたが直したほうが正確になる見込みです。

```bash
kikitori refine 会議.m4a --glossary 用語集.tsv --external bundle --save-evidence 根拠.json > 束.json   # --save-evidence は手順の後に説明
```

1. `束.json` の `system` を指示として読み、`chunks` の各 `user` に答えます。答えは区間ごとに「`[番号] 本文`」の 1 行です。行数と番号が `chunks[].ids` と一致することを確かめます。区間の数と番号は、`束.json` の `chunks[].ids` に従います（配列の添字ではありません）。形は `kikitori schema bundle` で確かめられます。要約・言い換え・敬語の整形をせず、指示に書かれた規則に従います。
2. あなたの答えを `chunks` の順につないでファイルに書き、`kikitori refine --apply 束.json --reply 答え.txt --format json-v1` を打ちます（手順 2 の旗は、この 1 行のとおりです。`--save-evidence` は付けません）。`--apply` は、答えの各行を `束.json` の区間に戻し、直した結果を出します。答えの無い区間は元の本文のまま残り、stderr に警告が出ます。

`--external claude` を付けると、手順 1 と 2 を `claude` コマンド（Haiku）が行います。このときも本文は Anthropic のサービスへ渡るので、利用者が外部の LLM を使うと言っているときだけ打ちます。

上の例の `--save-evidence 根拠.json` は手順 1・2 の答えには関わらず、A・B・K の認識結果を保存する旗です。保存しておくと、あとで `--evidence 根拠.json` を付けて認識を省き、直し方だけを替えられます。

## 診断（kikitori doctor）

```bash
kikitori doctor                 # 人向け
kikitori doctor --json          # 機械向け。形は kikitori schema doctor
```

OS・CPU・使える計算機（Metal や CUDA は実際に初期化を試す）と、実際に使う計算機（「使う計算機（既定）」の行。既定は文字起こしが cpu、補正の B・K が metal など。JSON の `selected`）・取得済みのモデル（補正の B・K の取得状況を含む）と容量・ffmpeg と Ollama の有無・補正がどの直し方になる見込みか・空きメモリを出します。補正を打つ前に見ると、B・K の取得が要るかが分かります。動かないときや、導入の直後は、まずこれを打ちます。使えるエンジンは `kikitori engines list` で見られます。

## 更新

Python 版 0.2.x が入っている PC には `kikitori update` がありません。導入の 1 行を打ち直します。ただし Intel の Mac に 0.2.x が入っているときは、0.3 の導入の 1 行を打たず、0.2.x の更新の手順（`v0.2.1` の `install.sh` の打ち直し）で行います。

```bash
kikitori update --dry-run       # 何を更新するかを見る
kikitori update --yes           # 最新版へ。モデルは取り直さない
```

新しい版が起動しなければ、元の版に戻って終了コード 8 で終わります。CUDA 版と通常版を替えるときは `--variant cuda|cpu` を付けます。

## 削除

```bash
kikitori uninstall --dry-run             # 消すものを一覧にするだけ
kikitori uninstall --yes                 # 消す
kikitori uninstall --yes --keep-models   # モデルは残す
```

消すのは、導入スクリプトが記録したものと、既定の置き場にあるもの（実行ファイル、PATH に足した行、モデルと、`--dict` が語の読みを引くために kikitori が取得した形態素解析の辞書（unidic-lite）の置き場、スキル、kikitori が取得した Ollama のモデルと B・K）だけです。B・K は、kikitori が取得したものだけが消えます（導入スクリプトと kikitori が、置いたものを書き留める「受け取り記録」にあるもの）。Python 版やほかの道具が先に取得していた Hugging Face のキャッシュ（置き場は `~/.cache/huggingface/hub`。`HF_HOME` や `HF_HUB_CACHE` で変わります）は残り、`--dry-run` の「残すもの」にフォルダの場所が出ます。利用者がそれも消したいと言うときは、そのフォルダを消します。利用者が書いた `--dict` のファイルは消えません。ほかの場所に手で置いたものは一覧に出ず、消えません。Ollama 本体と、利用者が自分で入れたものは消しません。Ollama 本体を消したいと言われたら、`kikitori uninstall` では消えないことを伝え、Ollama の案内に従うよう利用者に頼みます。kikitori が取得した Ollama のモデルは、Ollama が止まっていると消せずに残り、`ollama rm <名前>` が案内されます。Ollama を起動してから（`ollama serve` か Ollama のアプリ）その行を打てば消えます。消すのは削除なので、利用者の了承を得てから打ちます。`--keep-models` は、モデルを残したいと言われたときに付けます（モデル、形態素解析の辞書、Hugging Face のモデル、Ollama のモデルが残り、実行ファイル・スキル・PATH の行だけが消えます）。言われていなければ付けずに、`--dry-run` の一覧を利用者に見せて了承を得てから `--yes` を付けます。

## スキルとして入れる

利用者に頼まれたときに、`kikitori skill install` で、この SKILL.md をエージェントのスキル置き場に書きます（Claude Code は `~/.claude/skills/kikitori`、Codex は `~/.codex/skills/kikitori`）。`kikitori skill path` で置き場、`kikitori skill uninstall` で削除です。

## 結果を使うときの注意

- 認識は完全ではありません。固有名詞・数字・同音異義語は前後の文脈で確かめてください。
- 重なった発言と、短い相づちは取りこぼします。
- 朗読のような明瞭な音声では、Whisper large-v3-turbo（OpenAI の認識モデル）のほうが正確なことがあります（FLEURS は朗読の公開データ。文字誤り率 CER は低いほど正確で、5.9% 対 5.2%）。自然な発話では kikitori が上回りました（ReazonSpeech の評価用データ、放送の自然な発話で 7.1% 対 10.9%）が、ReazonSpeech は学習データが同系統のため、このデータは kikitori に有利です。会議音声そのものでの検証ではありません。
- 話者分離の精度は測っていません。
