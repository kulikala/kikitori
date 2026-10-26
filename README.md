# kikitori

日本語の会議録音（Web 会議・会議室）を、話者ごとに分けて文字起こしするコマンドラインツールです。Rust 製の 1 本の実行ファイルで、Python などの実行環境は要りません。

- 文字起こしはすべてこの PC の中で完結します。モデルの取得に、Hugging Face などのアカウントは要りません。本文に句読点は付きません。
- `kikitori refine` で、文字起こしの誤り（誤変換・取りこぼし）を、文字起こしの本文（以下、A）とは別の 2 つの認識モデル（以下、B と K）の結果と LLM を使って直せます。これを補正と呼びます（A・B・K の定義は[補正](#補正kikitori-refine)）。補正も既定ではこの PC の中で完結します。本文が外へ出るのは、外部の LLM を指定したときと、Ollama を別の PC に向けたときだけです。
- 辞書（`--dict` に渡す、表記と読みのペアを書いたファイル。[辞書](#辞書)）で、固有名詞の表記を直せます。
- 出力は txt・json・json-v1・yaml・srt・vtt です。
- 生成 AI のエージェントから使うための仕組みを備えています（[エージェントから使う](#エージェントから使う)）。

以前の Python 版（0.2.x）を置き換える版です（[移行](#python-版-02x-からの移行)）。

対応する言語は日本語です。日本語専用が既定で、日本語の会議に英語が混ざる録音は `kikitori transcribe --model ja-en` で起こせます。英語だけの録音や、ほかの言語は認識の対象外です。補正（`kikitori refine`）は日本語専用で、`--model ja-en` を付けると終了コード 2 で止まり、`kikitori transcribe --model ja-en` を案内します。

## 動く環境

この文書では認識モデルを動かす装置を計算機と呼びます。`--device` の値で選び、`cpu`（CPU）、`metal`（Apple Silicon の GPU。Apple の GPU 向けの仕組み）、`cuda`（NVIDIA の GPU。NVIDIA の GPU 向けの仕組み）があります。実行ファイルには通常版と、NVIDIA の GPU 用の CUDA 版（Linux x86_64・Windows x64 向け。導入で選びます）があります。既定は `auto` で、通常版の Mac は Metal、CUDA 版は CUDA を初期化してみて、使えなければ CPU にします。Mac 以外の通常版は CPU です。文字起こしの認識モデル（ReazonSpeech）はいつも CPU で、GPU を使うのは補正の B・K だけです（LLM は、手元で LLM を動かすソフト Ollama が自分で GPU を使います）。

| 環境 | 補正の B・K の計算機 | 確かめたこと |
|---|---|---|
| macOS（Apple Silicon） | B・K は Metal | 起動して認識が通る。導入スクリプトでの導入と削除（素の macOS の VM）。文字起こしの CPU の速さとメモリ、B・K を CPU（fp32。32 ビットの浮動小数点で、[速さとメモリ](#速さとメモリ)に説明があります）で動かしたときの速さと、Metal で動かしたときの速さを測った |
| Linux x86_64 | B・K は CPU | 起動して認識が通る（エミュレーション上）。速さは測っていない |
| Linux arm64 | B・K は CPU | 起動して認識が通る（Docker）。導入スクリプトを端末の無いコンテナで実行した。B・K の CPU の速さを測った（Docker） |
| Windows x64 | B・K は CPU | 起動して認識が通る（arm64 の PC 上のエミュレーション）。速さは測っていない |
| Windows arm64 | B・K は CPU | 起動して認識が通る。`install.ps1` での導入と、`kikitori uninstall` での削除（VM）。文字起こしは 64 秒の音声に 19〜29 秒（4 コア・メモリ 8.6GB の VM での目安）。補正の B・K の速さは測っていない |
| Linux x86_64 の NVIDIA 版（CUDA） | B・K は CUDA（条件を満たすとき。条件は下の箇条書き）、満たさなければ CPU | ビルドと、GPU の無い PC で起動して CPU に切り替わるまで。**NVIDIA の実機では確かめていない** |
| Windows x64 の NVIDIA 版（CUDA） | 同上 | ビルドと、GPU の無い PC（arm64 の PC 上のエミュレーション）で起動して CUDA を使えないと判定するまで。**NVIDIA の実機では確かめていない** |

Intel の Mac 向けの実行ファイルはありません。

- 通常版（CUDA 版ではないほう）は、Mac では B・K を Metal で、Mac 以外では CPU で動かします。GPU が無い PC でも、Mac で `--device cpu` を付けても、同じ使い方で全機能が動きます。
- NVIDIA の PC でも、既定で入るのは通常版で、B・K は CPU で動きます。CUDA 版は導入の `KIKITORI_VARIANT=cuda`（[導入](#導入)の環境変数の表）で選びます。CUDA 版で GPU を使うには、compute capability 8.0 以上の NVIDIA の GPU（GeForce RTX 30 系、A100 以降。それより古い GPU では CPU になります）、そのドライバ、CUDA 12 の cuBLAS と cuRAND（NVIDIA の計算ライブラリ）が要ります。どれかが欠けていれば、CUDA 版も B・K を CPU で動かします。CUDA 版で速くなるのは補正の B・K だけで、文字起こしの本体は CPU のままです。自分の GPU の compute capability は `nvidia-smi --query-gpu=compute_cap --format=csv,noheader` で見られます（例 `8.6`）。CUDA 版を入れたあと、`kikitori doctor` の `cuda:` の行が「利用可能」なら使えます（cuBLAS・cuRAND の読み込みを含めて実際に初期化して確かめます）。補正の LLM のモデルは実行ファイルの版によらず、Apple Silicon は本体のメモリ、NVIDIA は GPU のメモリで選びます（[機器とモデルの選び方](#機器とモデルの選び方)）。
- 自分の PC で何が動くかは `kikitori doctor` で確かめられます（OS、使える計算機と実際に使う計算機（「使う計算機（既定）」の行。既定は文字起こしが cpu、補正の B・K が metal など。JSON の `selected`）、取得済みのモデルと容量、ffmpeg と Ollama の有無、補正がどの直し方（LLM を使うか、LLM を使わない長さの規則かなど。[直し方は環境で決まる](#直し方は環境で決まる)）になる見込みか、空きメモリ）。`kikitori doctor --json` は機械向けです。
- 使える認識エンジンは `kikitori engines list` で見られます。

### 音声の形式

m4a・mp4・mp3・wav・flac・ogg は、ffmpeg なしで読めます（m4a は AAC-LC と HE-AAC の v1・v2）。Opus（Google Meet の webm など）は [ffmpeg](https://ffmpeg.org) が要ります。無いときは、終了コード 4（終了コードの一覧は[エージェントから使う](#エージェントから使う)）と入れ方の案内を出して終わります。

## 導入

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh     # macOS・Linux
```

```powershell
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex            # Windows
```

OS に合うほうの 1 行を打つと、導入スクリプトが次を行います。

1. GitHub の Release から自分の環境の実行ファイルを取り、チェックサムを確かめ、PATH に置く（macOS・Linux は `~/.local/bin`、Windows は `%LOCALAPPDATA%\Programs\kikitori`）。
2. 文字起こしのモデル（約 750MB。認識の ReazonSpeech、話者分離の 2 モデル、声のある時間帯を見つける VAD の合計）と、`--dict` が語の読みを引くための形態素解析の辞書（unidic-lite。取得は約 47MB、展開後は約 250MB）を取得する（文字起こしのモデルと形態素解析の辞書を取得するコマンド `kikitori setup` が行います）。
3. 補正の環境を作る（`kikitori refine --setup` が行います。文字起こしと合わせて、全部で最大約 27GB。最大になるのは、18GB の LLM のモデル（merge）が入る PC です。自分の PC で何が入るかは `kikitori refine --setup --dry-run` で分かります）。[Ollama](https://ollama.com)（手元で LLM を動かすソフト）が無ければ入れ（macOS・Linux は Ollama の公式の install.sh、Windows は公式の install.ps1 を PowerShell で実行）、この PC に合う LLM のモデルを選んで取得する（[機器とモデルの選び方](#機器とモデルの選び方)）。補正の根拠に使う 2 つの認識モデル B・K（Qwen3-ASR と kana-whisper、約 6.8GB）も取得する。すでに手元にあるモデルは取得しません。

取得はどれも、大きさによらず確認を求めます。補正の環境を入れる前には、入れるものと容量を出して確認を求めます。確認は端末（人が対話で打ち込む画面）に直接尋ねる（`/dev/tty`。Windows は `CON`）ので、人が自分の端末で導入の 1 行を打つなら、標準入力がパイプでも確認が出て答えられます。エージェントの実行やコンテナ・CI には端末が無く、確認は答えを待たずに終了コード 6 で止まります。

端末が無いと、導入スクリプトの `kikitori setup`（手順 2）が終了コード 6 で止まり、続く `kikitori refine --setup`（手順 3。`KIKITORI_NO_REFINE=1` なら呼ばれません）も、確認まで進めば終了コード 6 で止まります（その前に空きなどが足りなければ、そのことを出して止まります）。そのとき導入スクリプトは「Note: kikitori setup stopped (exit 6) …」のように Note を出して終了コード 0 で終わります。入っているのは実行ファイルだけで、文字起こしのモデルも補正の環境も入っていません。確認を省くには `KIKITORI_YES=1` を付けて導入の 1 行を打ちます。あとから入れるなら、`kikitori --yes setup` を打ち、補正も使うなら `kikitori refine --setup --yes` も打ちます。補正の環境ごと入れる行では `kikitori setup` と `kikitori refine --setup` が止まり、文字起こしだけの行（`KIKITORI_NO_REFINE=1`）では `kikitori setup` だけが止まります。`kikitori setup` は、取得する大きさ（文字起こしのモデル約 750MB と形態素解析の辞書約 47MB で、合計約 796MB）の 2 倍（約 1.6GB）の空きが無いと、取得の前に終了コード 7 で止まり、空きと要る量を出します。

文字起こしだけを使うなら、補正の環境は要りません。あとから足すときは `kikitori refine --setup` を打ちます。Ollama、LLM のモデル、B・K を取得し、Ollama も起動します。Ollama のモデルと B・K を置くディスクの空きも確かめます。B・K は約 6.8GB（B 約 3.8GB、K 約 3.0GB）で、Ollama のモデルの大きさは別です（[機器とモデルの選び方](#機器とモデルの選び方)の表）。`--no-hf`（B・K はどちらも配布元が Hugging Face です。この旗はそれをこの時点では取得しない指定です）を付けると、B・K は補正を使うときに取得してよいかを確認します（端末が無ければ終了コード 6）。B・K だけなら `kikitori models pull --refine` です。
kikitori 本体の導入に管理者権限は要りません。Ollama の導入に管理者権限が要る環境（主に Linux）では、終了コード 10 で止まり、頼むコマンドを案内します。

環境変数はパイプの先の `sh` に付けます（`curl -fsSL …/install.sh | KIKITORI_NO_REFINE=1 sh`）。Windows では先に `$env:KIKITORI_NO_REFINE = "1"` を打ってから導入の 1 行を打ちます。

| 環境変数 | 働き |
|---|---|
| `KIKITORI_NO_REFINE=1` | 導入で、補正の環境（認識モデル B・K、Ollama、LLM のモデル）をまるごと入れず、文字起こしだけにする |
| `KIKITORI_VARIANT=cuda` | CUDA 版を入れる（Linux x86_64・Windows x64。どちらも NVIDIA の実機では確かめていません。確かめた範囲は[動く環境](#動く環境)の表） |
| `KIKITORI_VERSION` | 入れる版を固定する（例: `0.3.0`。`v` は付けない） |
| `KIKITORI_INSTALL_DIR` | 実行ファイルの置き場を変える |
| `KIKITORI_NO_MODIFY_PATH=1` | PATH の設定を書き換えない |
| `KIKITORI_YES=1` | 確認を省く（端末の無い環境・CI 用） |

### Python 版 0.2.x からの移行

Python 版を `uv tool` で入れてある PC でも、導入の 1 行を打つだけです。導入スクリプトが Python 版を外してから、新しい実行ファイルを置きます。文字起こしのモデルは置き場が同じで、Python 版が取得した B・K も Hugging Face のキャッシュにあるものをそのまま使うので、取り直しません。`kikitori-refine` は 0.3 の間だけ、`kikitori refine` へ転送する名前として残ります。コマンドの形、失敗の終了コード、`--device` の値、文字起こしで GPU を使えなくなること（0.3 の文字起こしはいつも CPU です）、出力が Python 版と完全には一致しないことなど、変わる点は [CHANGELOG.md](CHANGELOG.md) の移行の表にあります。主なものは `kikitori-refine` が `kikitori refine` になること、失敗の終了コードが 1 から種類ごとの 2〜11 になること、`--device` の値が `auto`・`cpu`・`metal`・`cuda` になること、0.2.x の `kikitori --setup`（文字起こしのモデルの取得）が `kikitori setup` に、`kikitori-refine --setup`（補正の環境）が `kikitori refine --setup` になることです。Intel の Mac 向けの実行ファイルは無いので、Intel の Mac では導入の 1 行を打たず、0.2.x を使い続けてください（初めて入れるときは `curl -fsSL https://github.com/kulikala/kikitori/releases/download/v0.2.1/install.sh | sh`）。

### 更新

```bash
kikitori update            # 最新版へ。モデルは取り直さない
kikitori update --dry-run  # 何を更新するかを見るだけ
```

`kikitori update` は、更新した版が起動しないとき、元の版に戻して終わります。`kikitori skill install` で書いたスキル（エージェント用の手順書。[エージェントから使う](#エージェントから使う)）も新しい版の内容に書き換わります。CUDA 版と通常版を替えるときは `--variant cuda`（CUDA 版）か `--variant cpu`（通常版。CUDA 版ではない版を指す名前で、Mac の通常版は B・K に Metal を使います）を付けます。導入の 1 行を打ち直しても更新できます。

### 削除

```bash
kikitori uninstall --dry-run   # 消すものを一覧にするだけ
kikitori uninstall             # 一覧を出して、確認のうえ消す
```

`kikitori uninstall` が消すものは、kikitori が置いた次のものです。何を置いたかは、導入スクリプトと kikitori が、置いたものを 1 行ずつ書き留めるファイル（受け取り記録。`~/.config/kikitori/receipt.txt`、Windows は `%APPDATA%\kikitori\receipt.txt`）に残ります。`uninstall` は、この記録と、既定の置き場を見て消します。

- 実行ファイル（`kikitori-refine` の転送の名前を含む）と、PATH のために `# added by kikitori` の印をつけて足した行（シェルの設定ファイル。Windows は利用者の環境変数）
- モデルの置き場（`~/.cache/kikitori/models`）と、形態素解析の辞書の置き場（`~/.cache/kikitori/dict`）。Windows は `%USERPROFILE%\.cache\kikitori` の下です。利用者が書いた辞書のファイルは消えません。`--keep-models` を付けると、モデル、形態素解析の辞書、Hugging Face のモデル、Ollama のモデルを残し、実行ファイル、スキル、PATH の行だけを消します
- エージェント用のスキル（Claude Code は `~/.claude/skills/kikitori`、Codex は `~/.codex/skills/kikitori`）
- kikitori が取得した Ollama のモデルと、kikitori が取得した補正の認識モデル B・K（約 6.8GB）。Ollama が止まっているときは、消すコマンドを案内して残します

残すものは Ollama 本体（不要なら [Ollama](https://ollama.com) の案内に従って消してください）、利用者が自分で入れた Ollama のモデル、kikitori が取得していない Hugging Face のキャッシュの中身です。B・K は Hugging Face のキャッシュに入っていて、そのうち kikitori が取得したもの（受け取り記録にあるもの）だけが消えます。Python 版やほかの道具が先に取得していたものは残り、`--dry-run` の「残すもの」に場所が出ます。kikitori のモデルの置き場は `kikitori models path` で見られます。Hugging Face のキャッシュの置き場は `~/.cache/huggingface/hub`（`HF_HOME` や `HF_HUB_CACHE` で変わります）で、kikitori が取得していないものを消すなら、`--dry-run` の「残すもの」に出るフォルダを消します。

消す一覧に出るのは、導入スクリプトが記録したものと、既定の置き場にある実行ファイル・モデル・辞書だけです。ほかの場所に手で置いたものは出ず、消えません。必ず `--dry-run` の一覧を確かめてください。

## 使い方

導入の直後に `kikitori` が見つからないときは、新しいシェルを開いてください（PATH の設定が反映されます）。

```bash
kikitori transcribe 会議.m4a                                  # 話者つきのテキスト
kikitori transcribe 会議.m4a --num-speakers 4 --format yaml   # 人数を指定して YAML
kikitori transcribe 会議.m4a --dict 辞書.tsv --format vtt     # 辞書を使って WebVTT の字幕
kikitori refine 会議.m4a --glossary 用語集.tsv                # 補正つき
```

`kikitori 会議.m4a` のように `transcribe` を省いても動きます。次の旗はどのコマンドでも使えます。`--log-format`（診断ログの形式）、`--yes`（確認を省く）、`--dry-run`（実行内容だけを見る）、`--offline`（ネットワークから取得しない。モデルが足りないと終了コード 5）、`-v`（詳しいログ）、`--quiet`（情報ログを出さない）。結果は stdout、進捗と警告は stderr に出ます。モデルの置き場は `~/.cache/kikitori/models`（Windows は `%USERPROFILE%\.cache\kikitori\models`。環境変数 `KIKITORI_MODELS` で変更可）です。実際の場所は `kikitori models path` で出せます。文字起こしのモデルが置き場に無ければ、最初の `kikitori transcribe` の実行で取得します（取得してよいかの確認が出ます）。`kikitori setup` で先に取得できます。

### 出力形式

`--format` で選びます。`kikitori transcribe` と `kikitori refine` で同じです。

| 形式 | 中身 | 向いている使い方 |
|---|---|---|
| `txt`（既定） | 1 行 1 発言。`[分:秒.小数] 話者N: 本文`（時刻は小数つきです。例は下） | 人が読む。LLM に渡すとき、細かい時刻が要らなければこれで足りる |
| `json-v1` | オブジェクト。`schema_version` と、発言の配列 `segments` を持つ（各発言は `start`・`end`（秒）、`speaker`、`text`） | LLM や道具で読む。形に版がある |
| `json` | `start`・`end`、`speaker`、`text` を持つ配列 | 配列だけが欲しい道具（Python 版 0.2.x と同じ形です。Python 版向けに書いたスクリプトのために残してあります） |
| `yaml` | json と同じ項目と値 | LLM や道具で読む。json より短く、日本語をそのまま書く |
| `srt` | SubRip の字幕 | 動画編集の道具 |
| `vtt` | WebVTT の字幕。話者は `<v 話者N>` の声タグ | ブラウザや動画の道具 |

```
[00:00.00] 話者1: それでは定例会議を始めますまず先週の進捗から確認しましょう
[00:06.43] 話者2: 新しい料金プランの試験運用は予定どおり終わりましたユーザーからの問い合わせは先週より二割ほど減っています
```

```yaml
- start: 0.0
  end: 6.426593780517578
  speaker: 0
  text: "それでは定例会議を始めますまず先週の進捗から確認しましょう"
```

json・json-v1・yaml の `speaker` は 0 から数える番号で、txt・srt・vtt の「話者1」は `speaker: 0` にあたります。話者の名前との対応づけは結果を読む側の仕事です。
無音だけの録音は空の結果で終了コード 0 になります（json と yaml は `[]`、json-v1 は空の `segments`、txt と srt は 0 バイト、vtt は `WEBVTT` の行だけ）。

### 辞書

1 行 1 語、`表記<TAB>読み` です。読みはひらがなでもカタカナでもかまいません。

```
負荷試験	ふかしけん
TypeScript	たいぷすくりぷと
```

認識結果の語を形態素解析で読み、その読み（発音）が辞書の読みと一致すると、辞書の表記に置き換えます。発音で一致しない語は綴りどおりの読み（「東京」なら発音は「トーキョー」、綴りは「トウキョウ」）でも照合します。読みが違う語は置き換えません（たとえば「付加価値」の読みは「ふかかち」で、辞書の「ふかしけん」と違うので、「負荷試験」には置き換わりません）。置き換えた区間（文字起こしが区切った発言のひとまとまり。txt の 1 行、json や yaml の 1 要素にあたる）の数は stderr に出ます。
音そのものが違う誤認識（「東京リージョン」が「東京水温」になるなど）は、辞書では直りません。そのときは、用語集を使う `kikitori refine --glossary` を使います。
`--dict` の辞書は認識のモデルに語を覚えさせるものではなく、認識したあとの置換です。語の読みは辞書とは別の形態素解析の辞書（unidic-lite。導入の `kikitori setup` で取得済み）から引きます。`--dict` を付けたときに形態素解析の辞書が無ければ、取得してよいかを確認します（端末が無ければ終了コード 6、`--yes` で取得、`--offline` なら 5）。

### 話者の人数

人数が分かるときは `--num-speakers` で指定してください。指定がないと、話者を分けすぎたりまとめすぎたりします。

## 速さとメモリ

67 分の合成会議（公開の合成音声と朗読音声をつないだもの）を、Apple Silicon の Mac の CPU で、話者つきで最後まで起こしたときの値です。

| | 処理の時間 | 最大メモリ |
|---|---|---|
| kikitori 0.3.0 | 279.7 秒（音声の長さの 0.069 倍） | 3.05GB |
| Python 版 0.2.1（比較用） | 310.9 秒（0.077 倍） | 3.53GB |

上の表の最大メモリ 3.05GB は文字起こし（`kikitori transcribe`）の値で、補正の B・K を含みません。

補正の B と K を CPU（fp32。fp32 は 32 ビットの浮動小数点で、fp16 の 16 ビットよりメモリを多く使います）で動かしたときの時間です。Mac の既定は Metal なので、Mac の行は既定の構成の値ではありません。音声は合計 378.6 秒の 38 本です。

| 環境 | B | K | Python 版 0.2.1 の B・K |
|---|---|---|---|
| Apple Silicon の Mac（`--device cpu`、4 スレッド） | 126.5 秒 | 139.2 秒 | 測っていません |
| Linux arm64（Docker、16 コア。B は 4 スレッド、K は 8 スレッド） | 402.1 秒 | 177.6 秒 | 477.4 秒・244.1 秒 |

Linux arm64 では、スレッドを 12・16 に増やすと K は遅くなりました。

- x86_64 の CPU の速さは測っていません。Windows は起動と認識が通ることを確かめました（Windows arm64 では導入と削除も確かめました）。速さは測っていません。
- Mac の既定の Metal（GPU）では、同じ 38 本で B（fp16）が 23.8 秒、K（fp32）が 30.2 秒でした（本文は CPU の fp32 と同じ）。
- `kikitori refine` は、B・K を読み込む直前にメモリを見積もります（`--plan`・`--evidence` や入力の誤りのときは見積もりません）。B を fp32 にすると約 9.7GB、B を fp16 にすると約 5.6GB です（K はいつも fp32 です。B と K は順に読み込むので、大きい B で決まります）。`--dtype` の既定は `auto` で、CPU は fp32、Metal は B が fp16・K が fp32 です。CPU でも `--dtype fp16` で B を fp16 にできます。使えるメモリが足りなければ、終了コード 11 で止まり、`--dtype fp16` か `--force`（見積もりを無視して続ける）を案内します。CUDA では見積もりません。`kikitori doctor` で空きメモリを見られます。

## 補正（kikitori refine）

`kikitori refine` は、文字起こしの誤りを直すコマンドです。音声を受け取り、文字起こしからやり直します。`transcribe` と同じ `--num-speakers`・`--dict` などが使え、`--model` は `ja`（既定）だけです。文字起こしが区切った発言のひとまとまり（以下、区間）ごとに、
同じ音声を別の 2 つの認識でも文字にし、その結果を根拠に本文を直します。区間は txt の 1 行、json や yaml の 1 要素にあたります。`kikitori transcribe` は補正をしないので、動作と出力は補正の影響を受けません。

```bash
kikitori refine --plan --glossary 用語集.tsv                       # どの直し方になるかだけを見る（音声は省ける）
kikitori refine 会議.m4a --glossary 用語集.tsv --format json-v1    # 手元で直す
kikitori refine 会議.m4a --glossary 用語集.tsv --external claude   # claude コマンド（Haiku）で直す
```

`--plan` は、どの直し方になるか（次の節の 5 通り）と使うモデルを出して終わります。`--glossary` などの旗は本番と同じものを付け、本番は同じ旗から `--plan` を外して打ちます。

直す対象の本文を A、根拠にする 2 つの認識を B と K と呼びます（A・B・K は記号です。A は kikitori の文字起こしの本文だけを指し、K の出力は音をカタカナで書いたものです）。

| 記号 | モデル | 出すもの | ライセンス |
|---|---|---|---|
| A | 文字起こしの本文（出すのは ReazonSpeech） | 漢字かな交じり | Apache-2.0 |
| B | Qwen3-ASR 1.7B 日本語版（`neosophie/Qwen3-ASR-1.7B-JA`） | 漢字かな交じり。A が落とした発話を拾うが、話されていない文を作ることがある | Apache-2.0 |
| K | kana-whisper（`sbintuitions/kana-whisper`） | 音をカタカナで書いたもの | MIT |

用語集（`--glossary`）は 1 行 1 語、`表記<TAB>補足` です。補足（読みや説明）は省けます。会議に出る社名・地名・製品名を書くと、LLM がその表記に直します。
辞書（`--dict`）が読みの一致で機械的に置き換えるのに対し、用語集は LLM に渡して判断させます。`kikitori refine` では両方を使えます。

### 直し方は環境で決まる

起動時に、B・K のモデルの有無、指定した旗、Ollama にあるモデルを調べ、次の上から順に当てはまるものを選びます（`--external` は利用者の指定ですが、この表では 2 行目に並びます）。選んだ直し方と理由は stderr に出ます。
CER は文字誤り率で、低いほど正確です。幅は同じ条件で 2〜3 回測った値の幅です。
merge は突き合わせ用の大きな LLM のモデル、fix は用語の置き換え用の小さなモデル（tev1。Ollama で動く、選択肢から 1 つを選ぶ小さなモデル、0.8GB）の呼び名です。どのモデルかは次の節の表の merge の列と fix の列にあります。

| 状況 | 直し方 | 正解つき録音 1 本での CER |
|---|---|---|
| B・K のモデルが無く、取得しない（端末が無ければ確認で終了コード 6、`--yes` で取得、`--offline` なら 5。この行は `--external` より先に当たる） | 文字起こしの本文のまま | 28.4% |
| `--external` を指定した | A・B・K と用語集を外部の LLM に渡して直させる（`--external bundle` では呼び出し元の LLM が直す） | 19.1〜20.4% |
| merge のモデルが Ollama にある | 区間ごとに、そのモデルに A・B・K を突き合わせて本文を書かせる | 21.6〜22.4% |
| fix のモデルが Ollama にあり、`--glossary` を指定した | 長さの規則（次の行）で A か B を選んだあと、用語集の語に置き換えるかをそのモデルに選ばせる | 24.6〜25.0% |
| 上の 4 行のどれにも当たらない（Ollama が答えない、`--glossary` が無いなど） | 長さの規則: B の文字数が A の 1.6 倍＋3 文字を超える区間だけ B を採る（A が落とした発話を B で補う） | 24.8〜25.2% |

- CER は 5 分・1126 字の会議録音 1 本を Apple Silicon で測った値で、外部 LLM は Haiku です。ほかの録音で同じ差が出るとは限りません。
- この CER は同じ録音 1 本で直し方どうしを比べる値です。次の「精度」の節の値（朗読や放送音声 100 本での文字起こしの CER）とは、データも測り方も違うので、並べて比べられません。
- Ollama の向き先は `KIKITORI_OLLAMA`、無ければ `OLLAMA_HOST`、どちらも無ければ `localhost:11434` です。
- 普段の `kikitori refine` は、Ollama が止まっていても Ollama を起動しません。長さの規則に下がり、Ollama が答えないので長さの規則で直したことを stderr に出します（終了コードは 0）。Ollama を起動するのは `kikitori refine --setup` です。
- Ollama のモデルは `--merge-model <名前>`（別名 `--llm`、環境変数 `KIKITORI_LLM`）で指定できます。

### 機器とモデルの選び方

`kikitori refine --setup` と、補正の直し方を選ぶときの両方が、同じ表と、下の「モデルの大きさ＋8GB」の条件でモデルを選びます。ここでの機器（`--hardware` の値）は、補正の LLM のモデルを選ぶための区分で、認識モデルを動かす計算機（`--device`）とは別です。GPU があっても、AMD・Intel の GPU は `cpu` の区分です。

| 機器（`--hardware` の値） | merge（突き合わせ） | fix（用語の置き換え） |
|---|---|---|
| Apple Silicon（`apple`） | `qwen3.8:27b-mlx`（18GB） | `tev1:0.8b-mxfp8`（0.8GB） |
| NVIDIA の GPU（`nvidia`） | `qwen3.8:27b-q4_K_M`（18GB） | `tev1:0.8b-q8_0`（0.8GB） |
| 上の 2 行以外（`cpu`。CPU だけ、AMD・Intel の GPU） | 使わない | `tev1:0.8b-q8_0`（0.8GB） |

- Apple Silicon では Ollama の MLX エンジンで動く MLX 版を、それ以外では GGUF 版（Ollama で動くモデルのファイル形式。`q4_K_M`・`q8_0` は量子化の種類）を選びます。MLX 版は Apple Silicon でしか動かないためです。
- モデルを使うのは、モデルを置くメモリの大きさが「モデルの大きさ＋8GB」以上のときだけです。merge にも fix にも当てます（fix は 0.8GB なので約 9GB 以上）。モデルを置くメモリは、Apple Silicon は本体のメモリ（CPU と GPU で共有）、NVIDIA は最も大きい GPU 1 枚の GPU メモリ、それ以外は本体のメモリです。27B（モデル名の `27b`。パラメータ数 270 億）なら 26GB 以上が要るので、Apple Silicon の 16GB や NVIDIA の 24GB では fix だけになります。
- `cpu` の機器で 27B を使わないのは、CPU で回すと 1 区間に数十秒以上かかると見込むためです（測っていません）。
- 補正の精度を測ったのは、Apple Silicon の MLX 版だけです。GGUF 版（`q4_K_M`・`q8_0`）、NVIDIA、Windows での精度と速さは測っていません。
- Ollama は 0.40.0 以上が要ります。`--setup` は古い版を見つけると止まり、更新の仕方を出します。勝手に更新はしません。
- 機器の自動判定は `--hardware apple|nvidia|cpu` か環境変数 `KIKITORI_HARDWARE` で上書きできます。向き先の Ollama が別の PC にあるとき、`--setup` はその PC の機器を知らないので、`--hardware` も `KIKITORI_HARDWARE` も指定が無ければ `cpu` の区分として扱います。
- `kikitori refine --setup --dry-run` で、何を入れるかを入れずに確かめられます。2 回目以降の `kikitori refine --setup` は既にあるものを入れ直しません。

### 外部 LLM に渡す

`--external` を指定すると、本文が PC の外へ出ます。

- `--external claude`：`claude` コマンド（Claude Code の CLI。Claude Code を入れると使えます）に Haiku で渡します。
- `--external bundle`：指示（`system`）と問い（`chunks[].user`）をまとめた JSON（束）を stdout に出します。呼び出し元の LLM（kikitori を呼んだエージェントなどの LLM）が問いに答え、
  答えの「`[番号] 本文`」の行（番号は束の `chunks[].ids` にある区間の番号）をつないで `kikitori refine --apply 束.json --reply 答え.txt` に渡すと、直した結果が出ます。
  答えが無い区間は A のまま残り、stderr に警告が出ます。

`--save-evidence 根拠.json` で A・B・K の認識結果を保存しておくと、`--evidence 根拠.json` で認識を省いて直し方だけを替えられます。

## エージェントから使う

生成 AI のエージェント（Claude Code・Codex など）に kikitori を任せる人向けの仕組みです。

- スキル：エージェントが決まった置き場にある手順書を読んで使う仕組みです。`kikitori skill install` で、使い方の手順書（SKILL.md）をエージェントのスキル置き場に書きます。`kikitori skill path` で置き場を、`kikitori skill uninstall` で削除を行います。同じ内容が kikitori のリポジトリ（github.com/kulikala/kikitori）の `skills/kikitori/` にもあります。
- 機械向けの出力：`--log-format json`（環境変数 `KIKITORI_LOG_FORMAT`）を付けると、stderr が 1 行 1 件の JSON になります。各行の `type` の値は `progress`・`info`・`warning`・`error`・`summary` です。`error` は `code`（終了コードの名前）・`message`・`hint`（次に打つコマンド）を持ちます。stdout は選んだ出力形式の結果だけです。
- JSON の形：`kikitori schema <名前>` が JSON Schema を出します。名前は `transcript`・`transcript-v1`・`bundle`・`doctor`・`engines` です。
- 確認：取得（大きさによらずどれも）、削除、更新は確認が出ます。`--dry-run` で中身と容量を先に出せ、`--yes` で確認を省けます。端末が無いときは確認を待たず、終了コード 6 で終わり、`--yes` を案内します。

終了コードは次のとおりです（`kikitori --help` にも出ます）。

| コード | `code`（JSON の失敗の行） | 意味 |
|---|---|---|
| 0 | `success` | 正常終了 |
| 2 | `usage` | 引数の誤り |
| 3 | `input` | 入力がない、読めない、または形式が不正 |
| 4 | `dependency` | 必要な依存ソフトがない（ffmpeg が要る形式など） |
| 5 | `offline_model` | オフラインでモデルを利用できない |
| 6 | `needs_confirmation` | 確認が必要（`--yes` を指定） |
| 7 | `download` | ダウンロード失敗（ディスクの空き不足を含む） |
| 8 | `processing` | 処理失敗 |
| 9 | `refine` | `--external claude` を指定したのに `claude` コマンドが見つからない（Ollama が答えない、モデルが無いときは、別の直し方に下がって 0 のまま） |
| 10 | `privilege` | 管理者権限が必要 |
| 11 | `memory` | メモリ不足の見込み（`kikitori refine` の `--dtype fp16` か `--force` で続行） |

想定外の異常（panic）は、`code` が `internal` で終了コード 8 です。

## 仕組み

1. 音声を 16 kHz モノラルに変換する（ffmpeg が要るのは Opus などの一部の形式だけ）。
2. Silero VAD（音声区間検出のモデル）で、声のある時間帯を見つける。
3. pyannote segmentation-3.0 と CAM++（声の特徴を数値にする話者埋め込みのモデル）で、話者ごとの時間帯を求める。
4. 声のある時間帯をもとに、話者が交代する位置でだけ切る。
5. 4 で切った発言のひとまとまり（区間）ごとに、ReazonSpeech（日本語専用の音声認識モデル）で認識する。モデルの実行には sherpa-onnx（ONNX 形式のモデルを動かすライブラリ）を使う。
6. 辞書があれば置換する。

モデルとライセンスは次のとおりです。

| 用途 | モデル | ライセンス | 配布元 |
|---|---|---|---|
| 認識 | ReazonSpeech zipformer（日本語専用） | Apache-2.0 | sherpa-onnx の GitHub Releases |
| 認識（日英混在、`--model ja-en`。約 440MB。使うときに取得） | ReazonSpeech zipformer 日英版 | Apache-2.0 | sherpa-onnx の GitHub Releases |
| 話者ごとの時間帯 | pyannote segmentation-3.0（ONNX 版） | MIT | sherpa-onnx の GitHub Releases |
| 話者の特徴 | 3D-Speaker CAM++ | Apache-2.0 | sherpa-onnx の GitHub Releases |
| 声のある時間帯 | Silero VAD | MIT | sherpa-onnx の GitHub Releases |
| 補正 B | Qwen3-ASR 1.7B 日本語版 | Apache-2.0 | Hugging Face |
| 補正 K | kana-whisper | MIT | Hugging Face |

重みは配布元のファイルをそのまま読みます。kikitori が重みを変換・量子化して配ることも、手元のキャッシュに派生物を書くこともありません。
Hugging Face 上の pyannote の公式リポジトリは利用条件への同意が要りますが、kikitori が使うのは sherpa-onnx が GitHub Releases で配布している ONNX 版で、認証なしで取得できます。
取得するファイルと配布元は `kikitori models list` で見られます。

### 認識エンジンの差し替え

認識と話者分離のエンジンは共通のインターフェースの裏にあり、名前で選べます（`--asr`、`--diarizer`、補正は `--engine-b`・`--engine-k`）。足し方は [docs/ENGINES.md](docs/ENGINES.md) にあります。

## 精度

CER（文字誤り率）です。句読点と空白を除き、漢数字は算用数字にそろえて（正解と各ツールの出力の両方に同じ変換）採点しています。
各 100 本、Apple Silicon の CPU です。

| データ | kikitori | Whisper large-v3-turbo | Whisper large-v3 |
|---|---|---|---|
| FLEURS 日本語（朗読） | 5.9% | **5.2%** | 5.4% |
| ReazonSpeech の評価用データ（放送の自然な発話）※ | **7.1%** | 10.9% | 13.5% |

※ 切り出した音声に、正解の範囲の外の発話が残っているため、この行は全ツールとも、前後にはみ出した挿入を誤りに数えずに採点しています。
ReazonSpeech は学習データが同系統のため、このデータは kikitori に有利です。
表の kikitori は `kikitori transcribe` の結果です。認識モデルの出力は公開の短い音声 38 本で Python 版 0.2.x と一致しています。録音全体では、音声の読み込みの差で区切りが変わり、本文と話者の割り当てが Python 版と完全には一致しません（[CHANGELOG.md](CHANGELOG.md)）。

**朗読では Whisper に及びません**（0.7 ポイント差）。自然な発話では上回りましたが、有利なデータでの結果です。
会議音声そのもので比べた値ではありません。
どのツールが正確かをお使いの録音で比べるときは、10〜30 分に手で正解を付けるのが確実です。

話者分離の精度は測っていません。

## ライブラリとして使う

認識・音声の読み込み・出力形式などは Rust のクレートに分かれています。自分のプログラムからは `kikitori-core` と使うエンジンのクレート（Rust のライブラリの単位）に、git またはパスで依存します。クレートの一覧と役割は [docs/DESIGN.md](docs/DESIGN.md) にあります。crates.io には公開していません。

## リリースで配るもの

GitHub の Release ごとに、5 つの環境（macOS arm64、Linux x86_64・arm64、Windows x64・arm64）の実行ファイルと、Linux x86_64・Windows x64 の CUDA 版、チェックサム、導入スクリプトを載せます。一覧と手順は [docs/RELEASING.md](docs/RELEASING.md) にあります。

変更点は [CHANGELOG.md](CHANGELOG.md) にあります。

## 開発

```bash
cargo build --release                  # 実行ファイルを作る
cargo test --workspace                 # 通常のテスト（モデルが要るテストは除く）
cargo clippy --workspace --all-targets -- -D warnings
```

ビルドの最初に sherpa-onnx の配布物をネットワークから取得します。取得済みの置き場は環境変数 `SHERPA_ONNX_ARCHIVE_DIR` で指せます。Windows arm64 でビルドするには Visual Studio 2026 と clang が要ります。

設計の考え方は [docs/DESIGN.md](docs/DESIGN.md)、認識エンジンの足し方は [docs/ENGINES.md](docs/ENGINES.md)、リリースの手順は [docs/RELEASING.md](docs/RELEASING.md)、文書の書き方は [docs/WRITING.md](docs/WRITING.md) にあります。

## ライセンス

kikitori のコードは [Apache License 2.0](LICENSE) です。認識モデルのライセンスは上の「モデルとライセンス」の表のとおりです。補正の LLM のモデルと形態素解析の辞書のライセンスは、この文書では扱いません。
