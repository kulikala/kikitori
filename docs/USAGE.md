# 使い方

kikitori の使い方の詳しい説明です。入れ方は [INSTALL.md](INSTALL.md)、概要は [README](../README.md) にあります。

## 文字起こし

```bash
kikitori transcribe 会議.m4a                                  # 話者つきのテキスト
kikitori transcribe 会議.m4a --num-speakers 4 --format yaml   # 人数を決めて YAML
kikitori transcribe 会議.m4a --dict 辞書.tsv --format vtt     # 辞書を使って WebVTT の字幕
```

`kikitori 会議.m4a` のように `transcribe` を省いても動きます。結果は stdout に、進み具合と警告は stderr に出ます。文字起こしのモデルが無ければ、最初の `kikitori transcribe` が取得してよいかを尋ねます。先に取得しておくには `kikitori setup` を実行します。

次のオプションはどのコマンドにも付けられます。コマンド名の前にも後にも置けます。

| オプション | 働き |
|---|---|
| `--yes` | 確認を省く |
| `--dry-run` | 何をするかを出すだけで、変更しない |
| `--offline` | ネットワークから取得しない。モデルが足りなければ終了コード 5 で止まる |
| `--log-format json` | stderr を 1 行 1 件の JSON にする |
| `-v`（`--verbose`）・`--quiet` | ログを詳しくする・情報のログを出さない |

### 話者の人数

人数が分かるときは `--num-speakers` で渡してください。渡さないと kikitori が人数を推し量るので、1 人を 2 人に分けたり、2 人を 1 人にまとめたりすることがあります。

### 出力形式

`--format` で選びます。`kikitori transcribe` と `kikitori refine` で同じです。

| 形式 | 中身 | 向く使い方 |
|---|---|---|
| `txt`（既定） | 1 行 1 発言。`[分:秒.小数] 話者N: 本文` | 人が読む。細かい時刻が要らなければ、LLM に渡すのもこれで足りる |
| `json-v1` | `schema_version` と、発言の配列 `segments` を持つオブジェクト | LLM や道具で読む。形に版がある |
| `json` | 発言の配列 | 0.2.x と同じ形。0.2.x 向けに書いたスクリプトのために残してある |
| `yaml` | json と同じ項目と値 | LLM や道具で読む。json より短い |
| `srt` | SubRip の字幕 | 動画編集の道具 |
| `vtt` | WebVTT の字幕。話者は `<v 話者N>` の声タグ | ブラウザや動画の道具 |

json・json-v1・yaml の各発言は、`start`・`end`（秒）、`speaker`、`text` を持ちます。

```yaml
- start: 0.0
  end: 6.426593780517578
  speaker: 0
  text: "それでは定例会議を始めますまず先週の進捗から確認しましょう"
```

`speaker` は 0 から数える番号で、txt・srt・vtt の「話者1」が `speaker: 0` にあたります。無音だけの録音は空の結果で終了コード 0 になります（json と yaml は `[]`、json-v1 は空の `segments`、txt と srt は 0 バイト、vtt は `WEBVTT` の行だけ）。

### 辞書（--dict）

社名や製品名などの固有名詞を、決まった表記に直します。辞書は 1 行 1 語で、`表記<TAB>読み` と書きます。読みはひらがなでもカタカナでもかまいません。

```
負荷試験	ふかしけん
TypeScript	たいぷすくりぷと
```

kikitori は認識した語を形態素解析で読み、その読みが辞書の読みと一致したら、辞書の表記に置き換えます。読みは発音（「東京」なら「トーキョー」）で比べ、合わなければ綴りどおりの読み（「トウキョウ」）でも比べます。読みが違う語は置き換えません。たとえば「付加価値」の読みは「ふかかち」なので、「負荷試験」には置き換わりません。置き換えた発言の数は stderr に出ます。

辞書は認識のあとで文字を置き換えるだけで、認識そのものは変えません。そのため、音そのものを聞き違えた誤り（「東京リージョン」が「東京水温」になるなど）は直りません。そうした誤りは補正の用語集（`kikitori refine --glossary`）で直せます。

形態素解析の辞書（unidic-lite）は、インストールの `kikitori setup` で取得されます。無いまま `--dict` を付けると、取得してよいかを尋ねます。

## 補正（kikitori refine）

`kikitori refine` は、文字起こしの誤りを直すコマンドです。音声を受け取って文字起こしからやり直し、話者が交代する位置などで区切った発言のひとまとまり（以下、区間）ごとに直します。区間は txt の 1 行、json や yaml の 1 要素にあたります。`kikitori transcribe` の結果は補正の影響を受けません。

```bash
kikitori refine --plan --glossary 用語集.tsv                       # どの直し方になるかだけを見る（音声は要らない）
kikitori refine 会議.m4a --glossary 用語集.tsv --format json-v1    # 手元で直す
kikitori refine 会議.m4a --glossary 用語集.tsv --external claude   # claude コマンドで直す
```

補正は文字起こしの本文を、別の 2 つの音声認識の結果と照らし合わせて直します。この文書では直す対象の本文を A、根拠にする 2 つの認識を B と K と呼びます。

| 記号 | モデル | 出すもの |
|---|---|---|
| A | 文字起こしの本文（ReazonSpeech） | 漢字かな交じりの文 |
| B | Qwen3-ASR 1.7B 日本語版（Hugging Face の `neosophie/Qwen3-ASR-1.7B-JA`） | 漢字かな交じりの文。A が落とした発話を拾う一方、話されていない文を作ることがある |
| K | kana-whisper（Hugging Face の `sbintuitions/kana-whisper`） | 音をカタカナで書いたもの |

`--glossary` で渡す用語集は 1 行 1 語で `表記<TAB>補足` と書きます。補足（読みや説明）は省けます。会議に出る社名・地名・製品名を書くと、LLM がその表記に直します。辞書（`--dict`）が読みの一致で機械的に置き換えるのに対し、用語集は LLM が文脈を見て判断します。`kikitori refine` では両方を使えます。`--model` は `ja`（既定）だけで、`ja-en` を付けると終了コード 2 で止まります。

### 直し方は環境で決まる

`kikitori refine` は、B・K のモデルの有無、付けたオプション、Ollama にあるモデルを調べ、次の表の上から順に、最初に当てはまる直し方を選びます。選んだ直し方と理由は stderr に出ます。`--plan` を付けると、直し方と使うモデルを出すだけで終わります。`--plan` には本番と同じオプションを付け、本番では同じオプションから `--plan` を外して実行します。

表の merge は突き合わせに使う大きな LLM のモデル（18GB）、fix は用語集の語に置き換えるかを選ぶ小さな LLM のモデル（0.8GB）を指します。どのモデルかは次の節の表にあります。CER は文字誤り率で、低いほど正確です。

| 当てはまる状況 | 直し方 | CER |
|---|---|---|
| B・K のモデルが無く、取得もしない | 文字起こしの本文のまま | 28.4% |
| `--external` を付けた | A・B・K と用語集を外部の LLM に渡して直させる | 19.1〜20.4%（`--external claude`） |
| merge のモデルが Ollama にある | 区間ごとに、merge のモデルに A・B・K を突き合わせて本文を書かせる | 21.6〜22.4% |
| fix のモデルが Ollama にあり、`--glossary` を付けた | 長さの規則（次の行）で A か B を選んだあと、用語集の語に置き換えるかを fix のモデルに選ばせる | 24.6〜25.0% |
| 上のどれにも当たらない（Ollama が応答しない、`--glossary` が無いなど） | 長さの規則。B の文字数が A の 1.6 倍＋3 文字を超える区間だけ B を採り、A が落とした発話を補う | 24.8〜25.2% |

- CER は 5 分・1126 字の会議録音 1 本を Apple Silicon で測った値です。`--external claude` のモデルは Claude Haiku です。幅は同じ条件で 2〜3 回測った値の幅です。直し方どうしを同じ録音で比べるための値で、ほかの録音で同じ差が出るとは限りません。[MODELS.md](MODELS.md) の文字起こしの CER とは、データも採点の仕方も違うので比べられません。
- 表の 1 行目は、`--external` より先に当たります。B・K が無いとき、`kikitori refine` は取得してよいかを尋ね、断ると文字起こしの本文のまま出します。ターミナルが無ければ終了コード 6 で、`--offline` なら終了コード 5 で止まります。
- 普段の `kikitori refine` は、Ollama が止まっていても Ollama を起動しません。長さの規則に切り替え、そのことを stderr に出して、終了コード 0 で終わります。
- Ollama の接続先は、環境変数 `KIKITORI_OLLAMA`、それが無ければ `OLLAMA_HOST`、どちらも無ければ `localhost:11434` です。
- merge に使うモデルは `--merge-model <名前>`（環境変数 `KIKITORI_LLM`）で決められます。

### 機器とモデルの選び方

`kikitori refine --setup` が取得するモデルと、`kikitori refine` が使うモデルは、どちらも次の表で決まります。ここでいう機器（`--hardware` の値）は、LLM のモデルを選ぶための PC の区分で、B・K を動かす計算機（`--device`）とは別のものです。

| 機器（`--hardware` の値） | merge | fix |
|---|---|---|
| Apple Silicon（`apple`） | `qwen3.8:27b-mlx`（18GB） | `tev1:0.8b-mxfp8`（0.8GB） |
| NVIDIA の GPU（`nvidia`） | `qwen3.8:27b-q4_K_M`（18GB） | `tev1:0.8b-q8_0`（0.8GB） |
| それ以外（`cpu`。AMD・Intel の GPU を含む） | 使わない | `tev1:0.8b-q8_0`（0.8GB） |

- モデルを使うのは、モデルを置くメモリが「モデルの大きさ＋8GB」以上あるときだけです。モデルを置くメモリは、Apple Silicon では本体のメモリ（CPU と GPU で共有）、NVIDIA では最も大きい GPU 1 枚のメモリ、それ以外では本体のメモリです。merge には 26GB 以上が要るので、16GB の Apple Silicon や 24GB の NVIDIA の GPU では fix だけを使います。
- `cpu` の機器で merge を使わないのは、CPU では 1 区間に数十秒以上かかると見込むためです（測ってはいません）。
- Apple Silicon では MLX 版（Apple Silicon 向けの形式）を、それ以外では GGUF 版を選びます。補正の精度を測ったのは Apple Silicon の MLX 版だけで、GGUF 版、NVIDIA、Windows での精度と速さは測っていません。
- Ollama は 0.40.0 以上が要ります。`kikitori refine --setup` は古い版を見つけると、更新の仕方を出して止まります。Ollama を勝手に更新はしません。
- 機器の判定は、`kikitori refine` と `kikitori refine --setup` に付ける `--hardware apple|nvidia|cpu` か、環境変数 `KIKITORI_HARDWARE` で上書きできます。Ollama の接続先を別の PC にしたときは、その PC の機器が分からないので、上書きが無ければ `cpu` として扱います。

### 外部の LLM で直す

`--external` を付けると、本文が PC の外へ出るか、呼び出し元に渡ります。手元の PC で動かす LLM は Ollama だけに対応していて、LM Studio・llama.cpp・vLLM のような OpenAI 互換の API で動かす LLM には、今はつなぐ手段がありません（対応は [issue #1](https://github.com/kulikala/kikitori/issues/1) に積んであります）。`--external bundle` は、kikitori を呼んだエージェントが自分で直すための形です。

- `--external claude` は、`claude` コマンド（Claude Code の CLI）に、モデル Claude Haiku で直させます。利用料は、その `claude` コマンドにログインしているアカウントに付きます。
- `--external bundle` は、LLM への指示（`system`）と、区間ごとの修正の依頼（`chunks[].user`）をまとめた JSON（以下、プロンプトの JSON）を stdout に出します。kikitori を呼んだエージェントなど、呼び出し元の LLM がこのプロンプトに応答します。応答の `[番号] 本文` の行（番号はプロンプトの JSON の `chunks[].ids` にある区間の番号）をつなぎ、`kikitori refine --apply プロンプト.json --reply 応答.txt` に渡すと、直した結果が出ます。応答の無い区間は A のまま残り、stderr に警告が出ます。プロンプトの JSON の形は `kikitori schema bundle` で分かります。

`--save-evidence 根拠.json` で A・B・K の認識結果を保存しておくと、次からは `--evidence 根拠.json` で認識を省き、直し方だけを替えて試せます。

### 補正の速さとメモリ

B と K を CPU で動かしたときの時間です。音声は、公開の短い音声 38 本、合計 378.6 秒です。

| 環境 | B | K |
|---|---|---|
| Apple Silicon の Mac（`--device cpu`、4 スレッド） | 126.5 秒 | 139.2 秒 |
| Linux arm64（Docker、B は 4 スレッド、K は 8 スレッド） | 402.1 秒 | 177.6 秒 |

- Mac の既定の Metal では、同じ 38 本で B が 23.8 秒、K が 30.2 秒でした。
- x86_64 の CPU と Windows での速さは測っていません。
- Linux arm64 では、K のスレッドを 12・16 に増やすと遅くなりました。スレッド数は `--threads` で変えられます（既定 4）。
- B・K を読み込む前に、`kikitori refine` は要るメモリを見積もります。B を 32 ビットの浮動小数点（fp32）で動かすと約 9.7GB、16 ビット（fp16）で動かすと約 5.6GB です。K はいつも fp32 で、B と K は順に読み込むので、要るメモリは B で決まります。
- `--dtype` の既定は `auto` で、CPU では B を fp32、Metal では B を fp16 で動かします。CPU でも `--dtype fp16` で B を fp16 にできます。
- メモリが足りない見込みなら、終了コード 11 で止まります。`--dtype fp16` を付けるか、`--force` で見積もりを無視して続けられます。CUDA では見積もりません。

## エージェントから使う

Claude Code や Codex のような生成 AI のエージェントに kikitori を任せるための仕組みです。

- **スキル。** `kikitori skill install` が、kikitori の使い方を書いた手順書（SKILL.md）をエージェントのスキルの置き場に書きます。書き込む前の確認はありません。既定は Claude Code（`~/.claude/skills/kikitori`）で、`--agent codex` なら Codex（`~/.codex/skills/kikitori`）です。置き場は `kikitori skill path` で分かり、`kikitori skill uninstall` で消せます。同じ手順書が、このリポジトリの `skills/kikitori/` にもあります。
- **機械が読むログ。** `--log-format json`（環境変数 `KIKITORI_LOG_FORMAT`）を付けると、stderr が 1 行 1 件の JSON になります。各行の `type` は `progress`・`info`・`warning`・`error`・`summary` のどれかです。`error` の行は、`code`（下の表の名前）、`message`、`hint`（次に実行するとよいコマンド）を持ちます。stdout には、選んだ形式の結果だけが出ます。
- **JSON の形。** `kikitori schema <名前>` が JSON Schema を出します。名前は `transcript`（json・yaml の形）・`transcript-v1`（json-v1 の形）・`bundle`・`doctor`・`engines` です。
- **確認。** モデルの取得、補正の環境のインストール、削除、更新の前には確認が出ます。ターミナルが無いときは答えを待たずに終了コード 6 で止まり、`--yes` を案内します。`--dry-run` で、何をするかと容量を先に見られます。

### 終了コード

`kikitori --help` にも出ます。

| コード | `code` | 意味 |
|---|---|---|
| 0 | `success` | 正常に終わった |
| 2 | `usage` | 引数の誤り |
| 3 | `input` | 入力が無い、読めない、または形式が正しくない |
| 4 | `dependency` | 要るソフトが無い（Opus の音声で ffmpeg が無いなど） |
| 5 | `offline_model` | `--offline` のときに、要るモデルが手元に無い |
| 6 | `needs_confirmation` | 確認が要る（`--yes` を付ける） |
| 7 | `download` | 取得に失敗した（ディスクの空きが足りないときを含む） |
| 8 | `processing`・`internal` | 処理に失敗した。`internal` は想定外の異常 |
| 9 | `refine` | `--external claude` を付けたのに `claude` コマンドが見つからない |
| 10 | `privilege` | 管理者権限が要る |
| 11 | `memory` | メモリが足りない見込み |

## 困ったとき

| 起きたこと | どうするか |
|---|---|
| インストールの直後に `kikitori` が見つからない | 新しいターミナルを開きます。PATH の変更は、新しく開いたターミナルから効きます |
| インストールスクリプトが「Note: kikitori setup stopped (exit …)」と出した | 実行ファイルだけが入り、モデルも補正の環境も入っていません。出た終了コードの行を下の表で見て対処し、`kikitori setup` を実行し直します。補正も使うなら、続けて `kikitori refine --setup` を実行します（[インストール](INSTALL.md#インストール)） |
| インストールスクリプトが「Note: kikitori refine --setup stopped (exit …)」と出した | 文字起こしは使えます。補正の環境が途中までです。出た終了コードの行を下の表で見て対処し、`kikitori refine --setup` を実行し直します |
| インストールスクリプトが「another kikitori shadows the installed binary」と出して終了コード 8 で止まった | pip など `uv tool` 以外で入れた Python 版が、PATH で先に見つかっています。`pip uninstall kikitori` などで外してから、インストールのコマンドを実行し直します |
| インストールスクリプトが「Python kikitori found in uv; rerun with KIKITORI_YES=1」と出した | `uv tool` で入れた Python 版を外してよいかを尋ねたいのに、ターミナルがありません（エージェントや CI での実行）。外してよければ `curl -fsSL …/install.sh \| KIKITORI_YES=1 sh` と実行し直します（[旧版 0.2.x から移行する](INSTALL.md#旧版-02x-から移行する)） |
| インストールスクリプトが「installation cancelled」と出した | Python 版を外す問いに「いいえ」と答えたので止まりました。外してよければ実行し直して「y」と答えます |
| 終了コード 2 で止まった | オプションや値の誤りです。`kikitori <コマンド> --help` で書き方を確かめます |
| 終了コード 3 で止まった | 音声のファイルが無いか、読めない形式です。パスと形式（[ffmpeg](INSTALL.md#ffmpeg)）を確かめます |
| 終了コード 4 で止まった | 音声が Opus なので ffmpeg が要ります。ffmpeg をインストールして（[ffmpeg](INSTALL.md#ffmpeg)）、実行し直します |
| 終了コード 5 で止まった | `--offline` を外すか、ネットワークにつないで取得します。文字起こしのモデルは `kikitori setup`、補正の B・K は `kikitori models pull --refine` です |
| 終了コード 6 で止まった | 確認が要ります。人がターミナルで実行し直すか、`--yes` を付けて実行し直します |
| 終了コード 7 で止まった | ディスクの空きが足りないか、取得に失敗しました。空きは取得の前に確かめます。`kikitori setup` は約 1.6GB（取得する約 796MB の 2 倍）、`kikitori refine --setup` は B・K と LLM のモデルの分（最大約 26GB）を求めます。stderr に出る、空きと要る量を見て空きを作ります。通信の失敗なら、実行し直すと続きから取得します |
| 終了コード 8 で止まった | 処理の途中で失敗しました。`-v` を付けて実行し直し、stderr の内容を見ます（インストールスクリプトの 8 は、上の「another kikitori shadows」の行です） |
| 終了コード 9 で止まった | `--external claude` を付けたのに `claude` コマンドがありません。Claude Code を入れるか、`--external` を外して手元で直します |
| 終了コード 11 で止まった | `--dtype fp16` を付けて実行し直します（[補正の速さとメモリ](#補正の速さとメモリ)） |
| 補正が「長さの規則」で直したと出た | Ollama が無いか、応答していません。Ollama をインストールして起動し（[Ollama](INSTALL.md#ollama)）、`kikitori refine --setup` で LLM のモデルを取得します |
| `kikitori refine --setup` が「Ollama が見つかりません」「Ollama が動いていません」と出した | B・K だけを取得しました。LLM で直すには、Ollama をインストールして起動し（[Ollama](INSTALL.md#ollama)）、`kikitori refine --setup` を実行し直します |
| CUDA 版なのに GPU を使わない | `kikitori doctor` の `cuda:` の行を見ます。足りないものは [NVIDIA の GPU を使う](INSTALL.md#nvidia-の-gpu-を使う) の 3 つのどれかです |
| 話者が分かれすぎる、まとまりすぎる | `--num-speakers` で人数を渡します |
