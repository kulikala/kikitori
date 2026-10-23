---
name: kikitori
description: 日本語の会議録音（Web 会議・会議室）を、話者ごとに分けてローカルで文字起こしする CLI「kikitori」の使い方。音声や動画のファイル（m4a/mp3/wav/mp4 など）を文字に起こしたい、発言を話者別に分けたい、固有名詞の表記を辞書で直したいときに使う。Hugging Face の認証は不要。kikitori 自体は LLM による後処理を含まず、別コマンド kikitori-refine で、別の認識結果と LLM（手元の Ollama か、呼び出し元の LLM）を使って本文を直せる。
---

# kikitori：日本語の録音を話者つきで文字起こしする

音声ファイルを、話者ごとの発言（開始時刻・話者番号・本文）に起こします。処理はこの PC の中で完結し、
Hugging Face のトークンは要りません。要約や整形は含みません。要約や整形は、結果を受け取ったあなた（エージェント）が行ってください。

対応する言語は日本語です。英語が混ざる会議は `--model ja-en` で起こせますが、`kikitori-refine` の補正は日本語の会議だけです。

## 導入

`kikitori` が無ければ、利用者の了承を得てから次の 1 行で入れます。uv・kikitori・モデルに加え、補正用の Ollama と LLM のモデル（PC に合わせて最大約 27GB）が入ります。

```bash
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh   # macOS・Linux
curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | KIKITORI_NO_REFINE=1 sh   # 文字起こしだけ
irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex   # Windows（PowerShell）
```

入れたあと、補正の環境で何が足りないかは `kikitori-refine --setup --dry-run` で見られます。環境変数はパイプの先の `sh` に付けます。

`kikitori-refine --setup` は、入れる前に容量を出して端末で確認を求めます。あなたが打つときは端末が無いので止まります。入れるものと容量を利用者に伝えて了承を得てから、`KIKITORI_YES=1`（導入の 1 行の `sh` の前）か `--yes`（`kikitori-refine --setup`）を付けます。

## 実行

```bash
kikitori 会議.m4a --num-speakers 4 --format yaml > 会議.yaml   # LLM で読むなら yaml か json
kikitori 会議.m4a --dict 辞書.tsv                              # 辞書つき（テキスト出力）
kikitori 会議.m4a --format srt > 会議.srt
kikitori --setup                                               # モデルだけ先に取得する
```

- 結果は **stdout**、進捗と警告は **stderr** です。`-v` で、選ばれた計算機と話者数が見えます。
- 入力の変換に `ffmpeg` を使います。PATH に無ければ案内を出して終了します。
- モデル（約 750 MB）が無ければ、最初の実行で取得します。置き場は `~/.cache/kikitori/models`（`KIKITORI_MODELS` で変更可）です。

## 出力形式（`--format`）

| 形式 | 使うとき |
|---|---|
| `yaml`・`json` | 時刻と話者を機械で扱う、LLM でそのまま読む。yaml は日本語をそのまま書き、json より短い |
| `txt`（既定） | 細かい時刻が要らない。1 行 1 発言の `[分:秒] 話者N: 本文` |
| `srt`・`vtt` | 字幕の道具に渡す。時刻の行と本文の行が分かれていて、発言ごとの対応を読み取りにくいので、LLM で読むときは使わない |

yaml と json は発言ごとに次の 4 つを持ちます。

- `start`・`end`：秒
- `speaker`：0 から数える話者番号（txt・srt・vtt の「話者1」が `0`）。人物名ではありません。名前との対応づけは結果を読む側の仕事です。
- `text`：本文。句読点は付きません。

## 精度を上げる

- **`--num-speakers N`**：参加者の数が分かるなら指定してください。分からなければ省けますが、指定がないと、話者を分けすぎたりまとめすぎたりします。
  指定の人数に分けられなかったときは、stderr に警告が出ます。
- **`--dict`**：1 行 1 語、`表記<TAB>読み`（読みはひらがなでもカタカナでも可）。誤認識された固有名詞を、読みの一致で表記に直します。
  読みが違う語は置き換わらないので、辞書は広めに書いてかまいません。モデルへの語彙の注入ではなく、後段の置換です。ARM の Windows では使えません。
- **Zoom の「参加者ごとに個別のオーディオファイルを記録」** を使うと、話者分離が要らなくなり、いちばん正確です。
  その場合はファイルごとに実行し、`start` の順に並べます。
- **`--model ja-en`**：英語の発言が多い会議で試します。既定の `ja`（日本語専用）は、話されていない英語の語が紛れ込みにくい代わりに、英語を認識しません。

## 計算機の選び方

`--device auto`（既定）は、実際の音声の先頭で試走し、GPU がはっきり速いときだけ GPU を使います。
Apple Silicon では CoreML が CPU と同等かやや遅いため、CPU が選ばれます。これは正常です。
GPU を指定して使えない場合は、理由を stderr に出して CPU に戻ります。

NVIDIA（CUDA）、Windows の DirectML、AMD の MIGraphX/ROCm は実装されていますが、実機では検証されていません。
動かないときは `-v` で理由を確認し、`onnxruntime` の種類（`[cuda]` / `[directml]`）が合っているかを見てください。

## 本文を直す（kikitori-refine）

`kikitori-refine` は、kikitori の本文（A）の誤変換・取りこぼしを直す別のコマンドです。発言の区切り（区間）ごとに、同じ音声を別の 2 つの認識
（B: Qwen3-ASR、K: 音をカタカナで書く kana-whisper）でも文字にし、それを根拠に直します。導入の 1 行で入ります（Intel の Mac と ARM の Windows には入りません）。

```bash
kikitori-refine --plan --glossary 用語集.tsv                     # どの直し方になるかを stderr に出すだけ
kikitori-refine 会議.m4a --glossary 用語集.tsv --format json > 会議.json
```

- 用語集は 1 行 1 語、`表記<TAB>補足`（補足は読みや説明。省ける）です。会議に出る社名・地名・製品名を書きます。
- 直し方は環境で決まります。Ollama（手元で LLM を動かすソフト）に大きなモデルがあれば、区間ごとに A・B・K を突き合わせて書かせます。
  小さな判定用モデル tev1 しか無ければ、用語集の語に置き換えるかを選ばせます（`--glossary` が要る）。Ollama が答えなければ、
  B が A より十分に長い区間だけ B を採る規則（長さの規則）だけで直します。選ばれた直し方と理由は stderr に出ます。
- Ollama やモデルが無いと言われたら、`kikitori-refine --setup` で入れます（入れる前に容量を出して確認を求めます）。
- `--model ja-en` は受け付けません（補正は日本語だけ）。

### 呼び出し元の LLM（あなた）が直す

あなたが直すと、本文があなたの動いている LLM のサービスへ渡ります。利用者がそれでよいと言っているときは、Ollama で直すより、あなたが直したほうが正確になる見込みです。

```bash
kikitori-refine 会議.m4a --glossary 用語集.tsv --external bundle --save-evidence 根拠.json > 束.json
```

1. `束.json` の `system` を指示として読み、`chunks` の各 `user` に答えます。答えは区間ごとに「`[番号] 本文`」の 1 行です。
   要約・言い換え・敬語の整形をせず、指示に書かれた規則に従います。
2. あなたの答えを `chunks` の順につないでファイルに書き、`kikitori-refine --apply 束.json --reply 答え.txt --format json` を打ちます。
   `--apply` は、答えの各行を `束.json` の区間に戻し、直した結果を出します。答えの無い区間は元の本文のまま残り、stderr に警告が出ます。

`--external claude` を付けると、手順 1 と 2 を `claude` コマンド（Haiku）が行います。このときも本文は Anthropic のサービスへ渡ります。

## 結果を使うときの注意

- 認識は完全ではありません。固有名詞・数字・同音異義語は前後の文脈で確かめてください。
- 重なった発言と、短い相づちは取りこぼします。
- 朗読のような明瞭な音声では、Whisper large-v3-turbo のほうが正確なことがあります（FLEURS で CER 5.9% 対 5.2%）。
  自然な会話では kikitori が上回る傾向があります（放送音声で 7.1% 対 10.9%）が、会議音声そのものでの検証ではありません。
- 1 時間の録音は Apple Silicon で 6 分ほどかかります。先に `ffmpeg -t 300 -i 会議.m4a 先頭.m4a` で 5 分だけ切って試すと、設定を確かめられます。
