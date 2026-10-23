# kikitori

日本語の会議録音（Web 会議・会議室）を、話者ごとに分けて文字起こしするコマンドラインツールです。

- 文字起こしはすべてこの PC の中で完結します。モデルの取得に、Hugging Face などのアカウントは要りません。本文に句読点は付きません。
- 別のコマンド `kikitori-refine` で、文字起こしの誤り（誤変換・取りこぼし）を、別の 2 つの認識結果と LLM を使って直せます。これを補正と呼びます（[補正](#補正kikitori-refine)）。補正も既定ではこの PC の中で完結します。本文が外へ出るのは、外部の LLM を指定したときと、Ollama を別の PC に向けたときだけです。
- 辞書（表記と読みのペア）で、固有名詞の表記を直せます。
- 出力は txt・json・yaml・srt・vtt です。
- LLM のエージェントから使うためのスキルを同梱しています（`skills/kikitori/`）。

対応する言語は日本語です。英語が混ざる会議は `kikitori --model ja-en` で起こせますが、`kikitori-refine` の補正は日本語の会議だけに対応します。

## 導入

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh     # macOS・Linux
```

```powershell
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex            # Windows
```

OS に合うほうの 1 行を打つと、導入スクリプトが次を順に行います。

1. [uv](https://docs.astral.sh/uv/)（Python と道具の管理）が無ければ入れる。
2. kikitori を `uv tool install` で入れる（Python 3.12 も uv が用意する）。
3. `kikitori --setup` で、文字起こしのモデル（約 750MB）を取得する。
4. `kikitori-refine --setup` で、補正の環境を作る。[Ollama](https://ollama.com)（手元で LLM を動かすソフト）が無ければ公式の導入スクリプトで入れて起動し、この PC に合う LLM のモデルを選んで取得する（[計算機とモデルの選び方](#計算機とモデルの選び方)）。補正の根拠に使う 2 つの認識モデル（Qwen3-ASR と kana-whisper、約 6.8GB）も取得する。

`kikitori-refine --setup` は、入れる前に入れるものと容量を出し、端末で確認を求めます。導入スクリプトは最後に、足りないもの（ffmpeg など）と、PATH の設定が要るかを出します。

環境変数はパイプの先の `sh` に付けます（`curl -fsSL …/install.sh | KIKITORI_NO_REFINE=1 sh`）。`curl` の前に付けても `sh` には届きません。Windows では、先に `$env:KIKITORI_NO_REFINE = "1"` を打ってから導入の 1 行を打ちます。

| 環境変数 | 働き |
|---|---|
| `KIKITORI_NO_REFINE=1` | 補正を入れない（torch・Ollama・LLM のモデルを入れず、文字起こしだけにする） |
| `KIKITORI_NO_HF=1` | 補正の 2 つの認識モデルを先に取得しない（初めて補正するときに取得される） |
| `KIKITORI_YES=1` | 確認を省く（端末の無い環境・CI 用） |
| `KIKITORI_UPGRADE=1` | 既に入っている kikitori を入れ直す。上の 1 行の URL は `releases/latest` なので、最新の Release の版になる |

入力の変換に `ffmpeg` を使います。導入では入れないので、OS のパッケージ管理で入れてください（macOS は `brew install ffmpeg`、Ubuntu は `sudo apt-get install -y ffmpeg`、Windows は `winget install ffmpeg`）。

- 更新: `KIKITORI_UPGRADE=1` を付けて導入の 1 行を打ち直します。
- 取り外し: `uv tool uninstall kikitori`（Windows も同じ。`kikitori` と `kikitori-refine` の両方が消える）。モデルは別に消します。文字起こしのモデルは `~/.cache/kikitori/models` にあります。補正の認識モデルは `~/.cache/huggingface/hub` の `models--neosophie--Qwen3-ASR-1.7B-JA` と `models--sbintuitions--kana-whisper` です（同じフォルダにほかの道具のモデルもあるので、フォルダごとは消さない）。Ollama のモデルは[計算機とモデルの選び方](#計算機とモデルの選び方)の表の名前を `ollama rm qwen3.8:27b-mlx` のように指定して消せます。Ollama 本体と uv はほかの道具も使うので、導入スクリプトは消しません。消すときは、それぞれの説明に従ってください。
- Intel の Mac には補正（torch）が入らないので、文字起こしだけが入ります。
- Windows on ARM にも、文字起こしだけが入ります。DirectML 版の onnxruntime、補正が使う qwen-asr の依存（nagisa の dynet38）、辞書の置換（`--dict`）が使う fugashi に、ARM 版 Windows 用の配布物が無いためです。`--dict` を付けると、案内を出して終わります。

## 使い方

```bash
kikitori 会議.m4a                                  # 話者つきのテキスト
kikitori 会議.m4a --num-speakers 4 --format yaml   # 人数を指定して YAML
kikitori 会議.m4a --dict 辞書.tsv --format vtt     # 辞書を使って WebVTT の字幕
kikitori-refine 会議.m4a --glossary 用語集.tsv     # 補正つき
```

結果は stdout、進捗と警告は stderr に出ます。モデルの置き場は `~/.cache/kikitori/models`（環境変数 `KIKITORI_MODELS` で変更可）です。無ければ、最初の実行で取得します。

### 出力形式

`--format` で選びます。`kikitori` と `kikitori-refine` で同じです。

| 形式 | 中身 | 向いている使い方 |
|---|---|---|
| `txt`（既定） | 1 行 1 発言。`[分:秒] 話者N: 本文` | 人が読む。LLM に渡すとき、細かい時刻が要らなければこれで足りる |
| `json` | `start`・`end`（秒）、`speaker`、`text` を持つ配列 | LLM や道具で読む |
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

json と yaml の `speaker` は 0 から数える番号で、txt・srt・vtt の「話者1」は `speaker: 0` にあたります。話者の名前との対応づけは結果を読む側の仕事です。

### 辞書

1 行 1 語、`表記<TAB>読み` です。読みはひらがなでもカタカナでもかまいません。

```
負荷試験	ふかしけん
TypeScript	たいぷすくりぷと
```

認識結果を形態素に分け、連続する形態素の読みが辞書の読みと一致したときだけ置き換えます。
読みが違う語（「付加価値」など）は置き換えません。モデルへの語彙の注入ではなく、後段の置換です。

### 話者の人数

人数が分かるときは `--num-speakers` で指定してください。指定がないと、話者を分けすぎたりまとめすぎたりします。
指定した人数になるように、区間ごとの話者の特徴を取り直して併合します。

## 補正（kikitori-refine）

`kikitori-refine` は、`kikitori` の文字起こしの誤りを直すコマンドです。`kikitori` が区切った発言のひとまとまり（出力の 1 行。以下、区間）ごとに、
同じ音声を別の 2 つの認識でも文字にし、その結果を根拠に本文を直します。`kikitori` の動作と出力は変わりません。

```bash
kikitori-refine --plan --glossary 用語集.tsv                       # どの直し方になるかだけを見る
kikitori-refine 会議.m4a --glossary 用語集.tsv --format json      # 手元で直す
kikitori-refine 会議.m4a --glossary 用語集.tsv --external claude  # claude CLI（Haiku）で直す
```

直す対象の本文を A、根拠にする 2 つの認識を B と K と呼びます（K はカタカナの K）。

| 記号 | モデル | 出すもの | ライセンス |
|---|---|---|---|
| A | ReazonSpeech（`kikitori` の本文） | 漢字かな交じり | Apache-2.0 |
| B | Qwen3-ASR 1.7B 日本語版（`neosophie/Qwen3-ASR-1.7B-JA`） | 漢字かな交じり。A が落とした発話を拾うが、話されていない文を作ることがある | Apache-2.0 |
| K | kana-whisper（`sbintuitions/kana-whisper`） | 音をカタカナで書いたもの | MIT |

用語集（`--glossary`）は 1 行 1 語、`表記<TAB>補足` です。補足（読みや説明）は省けます。会議に出る社名・地名・製品名を書くと、LLM がその表記に直します。
辞書（`--dict`）が読みの一致で機械的に置き換えるのに対し、用語集は LLM に渡して判断させます。`kikitori-refine` では両方を使えます。

### 直し方は環境で決まる

起動時に環境を調べ、次の上から順に当てはまるものを選びます。選んだ直し方と理由は stderr に出ます。
CER は文字誤り率で、低いほど正確です。幅は同じ条件で 2〜3 回測った値の幅です。

| 環境 | 直し方 | 正解つき録音 1 本での CER |
|---|---|---|
| B・K を動かせない（補正を入れていない、Intel の Mac） | `kikitori` の本文のまま | 28.4% |
| `--external` を指定した | A・B・K と用語集を外部の LLM に渡して直させる（Ollama のモデルで直してから渡すことはしない） | 19.1〜20.4% |
| 突き合わせ用の大きなモデル（次の節の表の merge 列）が Ollama にある | 区間ごとに、そのモデルに A・B・K を突き合わせて本文を書かせる | 21.6〜22.4% |
| 用語の置き換え用の小さなモデル（次の節の表の fix 列）が Ollama にあり、`--glossary` を指定した | 長さの規則（次の行）で A か B を選んだあと、用語集の語に置き換えるかをそのモデルに選ばせる | 24.6〜25.0% |
| それ以外（Ollama が答えない、`--glossary` が無いなど） | 長さの規則: B が A の 1.6 倍＋3 字より長い区間だけ B を採る（A が落とした発話を B で補う） | 24.8〜25.2% |

fix 列の tev1 は決められた選択肢から 1 つを選ぶ判定用の小さなモデルで、Ollama の選択の機能（`/v1/systemone`、0.40.0 から）で使います。

- CER は 5 分・1126 字の会議録音 1 本を Apple Silicon で測った値です。外部 LLM への指示はこの録音を見ながら書いたので、ほかの録音で同じ差が出るとは限りません。外部 LLM の値は Haiku で測りました。
- 5 分の録音で、B と K の認識に約 2 分半かかり、メモリは最大 7.2GB 使いました（Apple Silicon、B は GPU、K は CPU）。
- Ollama の向き先は `KIKITORI_OLLAMA`、無ければ `OLLAMA_HOST`、どちらも無ければ `localhost:11434` です。
- 普段の `kikitori-refine` は、Ollama が止まっていても起動しません。長さの規則に下がり、その旨を出します。Ollama を起動するのは `kikitori-refine --setup` です。

### 計算機とモデルの選び方

`kikitori-refine --setup` と、補正の直し方を選ぶときの両方が、同じ表と同じ式でモデルを選びます。

| 機器（`--hardware` の値） | 判定のしかた | merge（突き合わせ） | fix（用語の置き換え） |
|---|---|---|---|
| Apple Silicon（`apple`） | macOS で `sysctl hw.optional.arm64` が 1。Intel 用の Python を Apple Silicon で動かしている場合も Apple Silicon と判定する | `qwen3.8:27b-mlx`（18GB） | `tev1:0.8b-mxfp8`（0.8GB） |
| NVIDIA（`nvidia`） | `nvidia-smi` が答える | `qwen3.8:27b-q4_K_M`（18GB） | `tev1:0.8b-q8_0`（0.8GB） |
| それ以外（`cpu`。CPU だけ、AMD・Intel の GPU） | Apple Silicon でも NVIDIA でもない | 使わない | `tev1:0.8b-q8_0`（0.8GB） |

Intel の Mac は「それ以外」にあたりますが、補正そのものが入らないので、この表のモデルは使いません。

- Apple Silicon では Ollama の MLX エンジンで動く MLX 版を、それ以外では GGUF 版を選びます。MLX 版は Apple Silicon でしか動かないためです。
- モデルを使うのは、モデルを置くメモリの大きさが「モデルの大きさ＋8GB」以上のときだけです。モデルを置くメモリは、Apple Silicon は本体のメモリ（CPU と GPU で共有）、NVIDIA は最も大きい GPU 1 枚の GPU メモリ、それ以外は本体のメモリです。27B なら 26GB 以上が要るので、Apple Silicon の 16GB や NVIDIA の 24GB では fix だけになります。
- それ以外の機器で 27B を使わないのは、CPU で回すと 1 区間に数十秒以上かかると見込むためです（測っていません）。
- 補正の精度を測ったのは、Apple Silicon の MLX 版だけです。GGUF 版（`q4_K_M`・`q8_0`）、NVIDIA、Windows での精度と速さは測っていません。
- Linux（arm64、CPU だけ）では、Docker の Ubuntu 24.04 で、導入の 1 行から補正（`tev1:0.8b-q8_0` での用語の置き換え）までが通ることを確かめました。64 秒の合成音声の補正に 260 秒かかりました（B と K の認識を CPU で行うため）。
- macOS（Apple Silicon の仮想マシン、macOS 26、メモリ 24GB、Ollama の無い状態から）では、導入スクリプトで Ollama と `tev1:0.8b-mxfp8` を入れ、補正（用語の置き換え）までが通ることを確かめました。仮想マシンの GPU では torch の MPS が動かないため、B と K の認識は CPU に替えて試し、64 秒の合成音声に 92 秒かかりました。
- Windows 11 on ARM（仮想マシン、メモリ 8GB）では、導入の 1 行から文字起こしまでが通ることを確かめました。64 秒の合成音声に 50 秒かかりました（CPU）。x64 の Windows と DirectML は確かめていません。
- Ollama は 0.40.0 以上が要ります（tev1 の選択に使う `/v1/systemone` が 0.40.0 からのため）。`--setup` は古い版を見つけると止まり、更新の仕方を出します。勝手に更新はしません。
- 判定は `--hardware apple|nvidia|cpu` か環境変数 `KIKITORI_HARDWARE` で上書きできます。向き先の Ollama が別の PC にあるとき、`--setup` はその PC の機器を知らないので、指定が無ければ「それ以外」として扱います。
- `kikitori-refine --setup --dry-run` で、何を入れるかを入れずに確かめられます。2 回目以降の `kikitori-refine --setup` は既にあるものを入れ直しません。

### 外部 LLM に渡す

`--external` を指定すると、本文が PC の外へ出ます。

- `--external claude`：`claude` コマンドに Haiku で渡します。60 区間ずつに分けて送ります。
- `--external bundle`：指示（`system`）と問い（`chunks[].user`）を JSON で stdout に出します。呼び出し元の LLM が問いに答え、
  答えの「`[番号] 本文`」の行をつないで `kikitori-refine --apply 束.json --reply 答え.txt` に渡すと、直した結果が出ます。
  答えが無い区間は A のまま残り、stderr に警告が出ます。

`--save-evidence A.json` で A・B・K を保存しておくと、`--evidence A.json` で認識を省いて直し方だけを替えられます。

## 仕組み

1. `ffmpeg` で 16 kHz モノラルに変換する。
2. Silero VAD で発話区間を見つける。
3. pyannote segmentation-3.0 と CAM++ の話者埋め込みで、話者の区間を求める。
4. VAD の発話区間をもとに、話者が交代する位置でだけ切る。
5. ReazonSpeech（zipformer transducer、日本語専用）で区間ごとに認識する。
6. 辞書があれば置換する。

モデルとライセンスは次のとおりです。

| 用途 | モデル | ライセンス | 配布元 |
|---|---|---|---|
| 認識 | ReazonSpeech zipformer（日本語専用 / 日英混在は `--model ja-en`） | Apache-2.0 | sherpa-onnx の GitHub Releases |
| 話者の区間 | pyannote segmentation-3.0（ONNX 版） | MIT | sherpa-onnx の GitHub Releases |
| 話者の特徴 | 3D-Speaker CAM++ | Apache-2.0 | sherpa-onnx の GitHub Releases |
| 発話区間 | Silero VAD | MIT | sherpa-onnx の GitHub Releases |

Hugging Face 上の pyannote の公式リポジトリは利用条件への同意が要りますが、kikitori が使うのは
sherpa-onnx が GitHub Releases で配布している ONNX 版で、認証なしで取得できます
（`pytest -m network` で、各 URL が認証なしで取れることを確認できます）。

## 文字起こしの計算機（GPU）の対応状況

認識は 2 つの経路を持ちます。ONNX Runtime（既定。CPU でも GPU でも使う）と、C++ の sherpa-onnx（onnxruntime が無いとき、
または日英混在モデル・int8 のときの CPU）です。ONNX Runtime 経路のほうが探索が精密で、CPU でも少し正確です。
`--device auto`（既定）は、まず CPU を測り、GPU が実際の音声の先頭 8 秒で CPU より 10% 以上速いときだけ GPU を使います。
provider が黙って CPU に戻る場合は、起動時に例外にして、GPU として扱いません。

| 計算機 | provider | 状態 |
|---|---|---|
| Apple Silicon（CPU） | ONNX Runtime / sherpa-onnx | **実測済み**。録音全体でリアルタイムの約 10 倍（56 分で 325 秒） |
| Apple Silicon（GPU / ANE） | CoreML | **実測済み**。動くが CPU と同等かやや遅い。auto は CPU を選ぶ |
| NVIDIA | CUDA | 実装のみ（実機で未検証） |
| AMD / Intel / NVIDIA（Windows） | DirectML | 実装のみ（実機で未検証） |
| AMD（Linux） | MIGraphX / ROCm | 実装のみ（実機で未検証） |
| AMD の iGPU | 上の DirectML / MIGraphX | 実装のみ（実機で未検証） |
| Vulkan、MLX | — | 未対応 |

導入スクリプトは、Linux で NVIDIA の GPU があれば CUDA 版、無ければ CPU 版の onnxruntime を、Windows では DirectML 版（Windows on ARM では CPU 版）を入れます。
話者分離（segmentation と埋め込み）は CPU で動きます。処理時間の大半を占めます（64 秒の会議で、話者分離 4.2 秒、それ以外 1.9 秒）。

## 精度

CER（文字誤り率）です。句読点と空白を除き、漢数字は算用数字にそろえて（正解と各ツールの出力の両方に同じ変換）採点しています。
各 100 本、Apple Silicon の CPU です。

| データ | kikitori（既定） | kikitori（sherpa-onnx 経路） | Whisper large-v3-turbo | Whisper large-v3 |
|---|---|---|---|---|
| FLEURS 日本語（朗読） | 5.9% | 6.4% | **5.2%** | 5.4% |
| ReazonSpeech テスト（放送の自然な発話）※ | **7.1%** | 7.0% | 10.9% | 13.5% |

※ 切り出し音声に、正解の範囲の外の発話が残っているため、前後にはみ出した挿入を数えない値です。
ReazonSpeech は学習データが同系統のため、このデータは kikitori に有利です。

**朗読では Whisper に及びません**（0.7 ポイント差）。自然な発話では上回りましたが、有利なデータでの結果です。
会議音声そのもので比べた値ではありません（認証なしで取れる、話者つきの日本語会議データを見つけられませんでした）。
どのツールが正確かをお使いの録音で比べるときは、10〜30 分に手で正解を付けるのが確実です。

話者分離は macOS の `say` で合成した 4 人の会議で動作を確かめました（`scripts/make_synth_meeting.py`）。
合成音声なので、実際の会議での精度を示すものではありません。

## 長い録音

約 56 分の録音（m4a、64 kbps）を Apple Silicon の CPU で処理して、325 秒（リアルタイムの約 10 倍）、最大メモリ 4.1 GB でした（sherpa-onnx 経路では 281 秒、4.4 GB）。
既定の構成の値です。認識は長さの近い順に小さなバッチで行います。まとめて処理すると、11 分の録音でもメモリが 19 GB に達します。

## エージェントから使う

`skills/kikitori/` が、LLM のエージェント向けの使い方です。使うエージェントの skills ディレクトリにコピーするか、リンクを張ってください。

## リリースで配るもの

GitHub の Release ごとに、次の 5 つを載せます。

- `kikitori-<版>-py3-none-any.whl`：kikitori のコード（モデルは含まない。モデルと依存は、それぞれのライセンスで別に取得される）
- `kikitori-<版>.tar.gz`：ソース
- `install.sh`：macOS・Linux 用の導入スクリプト。その Release の wheel を入れる
- `install.ps1`：Windows 用の導入の入口。ASCII だけで書いてあり、`install-windows.ps1` を取得して UTF-8 として読み、実行する（Windows PowerShell 5.1 の `irm` は、日本語を含むスクリプトを UTF-8 として読まないため）
- `install-windows.ps1`：Windows 用の導入スクリプトの本体。その Release の wheel を入れる

変更点は [CHANGELOG.md](CHANGELOG.md) にあります。

## 開発

```bash
uv sync --group dev                       # .venv に開発用の環境を作る
uv run pytest                             # 通常のテスト
uv run pytest -m network                  # モデルが認証なしで取得できることの確認
uv sync --group dev --extra refine        # kikitori-refine の認識（torch など）も入れる
```

`scripts/` に、FLEURS と ReazonSpeech テストでの CER 計測、合成会議の作成と採点、provider ごとの速さの計測があります。
リリースの手順は [docs/RELEASING.md](docs/RELEASING.md) にあります。

## ライセンス

kikitori のコードは [Apache License 2.0](LICENSE) です。使うモデルのライセンスは、上の表のとおりです。
