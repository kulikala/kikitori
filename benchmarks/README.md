# ベンチマーク

kikitori の文字起こしの精度と速さを、決まった音源で測る道具です。読み手は kikitori を直す開発者と、kikitori の版を出す人です。
精度は CER（文字誤り率。正解の字数に対する、置き換え・抜け・余分な字の数の割合で、低いほど正確）で表します。
`docs/MODELS.md` の精度と速さの表は、この道具で測ります。変更の前と後の kikitori で同じ音源を測ると、変更が精度や速さにどう響いたかが分かります。

## 必要なもの

- Python と [uv](https://docs.astral.sh/uv/)。採点に jiwer、ReazonSpeech の評価用データの読み込みに pyarrow を使い、どちらも uv が実行のたびに用意します。
- ffmpeg。音声をつなぐときと、音声の長さを調べるときに使います。
- 合成会議（`synth`・`hour`）を作るには、macOS の `say` と、その声 Kyoko・Eddy・Reed・Sandy。macOS 以外では、`synth`・`hour` を除いた音源だけを使えます。

以下ではリポジトリの直下で、次の形で呼ぶものとして書きます。

```bash
alias kbench='uv run -q --no-project --with jiwer --with pyarrow python -I benchmarks/bench.py'
```

## 最初に測るまで

次の順に実行します。表示の読み方は「測る」の節にあります。

```bash
cargo build --release -p kikitori          # 測る kikitori を作る
kbench fetch fleurs rz long synth hour     # 公開の音源を取得・生成する（macOS 以外では synth と hour を外す）
kbench run fleurs --bin target/release/kikitori
```

## 公開の音源

| 名前 | 中身 | 向いている測り方 |
|---|---|---|
| `fleurs` | FLEURS 日本語の評価用（test）の先頭 100 本（朗読） | 精度 |
| `rz` | ReazonSpeech の評価用データ（放送の自然な発話）。Hugging Face では 2 つのファイルに分けて置かれていて、その 1 つ目から等間隔に 100 本 | 精度 |
| `long` | FLEURS の先頭 50 本を、間を空けずにつないだ 1 本（約 11 分） | 長い音声の精度と速さ |
| `synth` | macOS の `say` で 4 人の声を合成した会議（約 1 分） | 話者が替わる会議の精度 |
| `hour` | `synth` と FLEURS 10 本を交互につなぎ、67 分を超えるまで続けた 1 本。FLEURS は回ごとに次の 10 本を使う | 1 時間ほどの会議の速さと最大メモリ |

`kbench fetch` は、FLEURS（約 1GB）と ReazonSpeech の評価用データ（約 300MB）を Hugging Face からダウンロードし、残りを手元で作ります（作る音声は合わせて約 150MB）。
置き場は `bench/` で、Git の対象外です。環境変数 `KIKITORI_BENCH_DATA` で置き場を変えられます。

`rz` の切り出した音声には、正解の文の前後に、正解に書かれていない発話が残っています。そのため `rz` では、ふつうの CER に加えて、正解の前後にはみ出した余分な字を誤りに数えない CER（表示では「端の挿入を除く」）も出します。

## 測る

```bash
kbench run synth --bin target/release/kikitori -- --num-speakers 4
```

`--` のあとに書いた引数はそのまま `kikitori transcribe` に渡します（例は話者の数を 4 人と指定しています）。
`run` は次を表示します。

```
synth  kikitori 0.4.0
  本数 1（失敗 0）  音声 64 秒  処理 7.4 秒（0.116 倍）  出力 296 字  最大メモリ 1.18GB
  CER 5.3%（正解のある 1 本）
```

- 失敗は、kikitori が 0 以外の終了コードで終わったか、何も出力しなかった本数です。
- 倍率は処理の時間を音声の長さで割った値です。小さいほど速く、0.1 倍なら 10 分の音声を 1 分で起こします。
- 短い音声の音源（`fleurs`・`rz`）は 1 本ごとに kikitori を起動するので、時間にはモデルの読み込みが含まれます。速さを比べるときは、1 本の録音（`long`・`hour`）を使います。
- 出力の字数は、正解の無い音声を、変更の前と後の kikitori で比べるときの目安です。字数が大きく減ったら、文字起こしが落ちている疑いがあります。

各音声の出力は置き場（既定は `bench/`）の `results/` に、音源の名前と kikitori の版（`kikitori --version` の値）をつないだファイル名で書きます。変更の前と後を測るときは、`--label 名前` で別のファイル名にします。
書いた出力は `kbench score 音源 出力のファイル` で採点し直せます（音源は、`run` と同じく名前かディレクトリのパス）。比べる相手の Whisper は `benchmarks/whisper.py` で同じ音源を起こし、同じ形の出力を書きます（使い方はスクリプトの冒頭にあります）。

## 手元だけで使う音源

公開できない録音（実際の会議など）も、同じ道具で測れます。録音と正解は、リポジトリの外のディレクトリに置きます。
そのディレクトリに次のような `manifest.json` を置き、`kbench run` にディレクトリのパスを渡します。

```json
{"kind": "clips", "items": [
  {"id": "a", "audio": "a.wav", "text_file": "a.truth.txt"},
  {"id": "b", "audio": "b.wav", "text": "正解の本文"},
  {"id": "c", "audio": "c.wav"}
]}
```

- `kind` は、短い音声を並べるなら `clips`、1 本の録音なら `recording` です。1 本の録音は `{"kind": "recording", "audio": "…", "text_file": "…"}` と書きます（`text` も使えます）。
- `audio`・`text_file` は `manifest.json` からの相対パスです。正解は `text_file` のファイルか、`text` に直接書きます。正解の無い音声（上の `c`）は CER に入れず、時間と字数だけに入れます。
- 出力はそのディレクトリの `results/` に書きます。`run` の表示には音声の名前も本文も出しません。
- 手元だけで使う音源のディレクトリが、リポジトリの中にあると `kbench run` は止まります。リポジトリの中で受け付けるのは、公開の音源の置き場（`bench/`）だけです。

```bash
kbench run ~/どこか/手元の音源 --bin target/release/kikitori
```

## 公開に混ざらないようにする仕組み

- 公開の音源と出力の置き場（`bench/`）は、Git の対象外です。
- `scripts/check_public.py`（リポジトリを公開する前の検査）は、公開する履歴と作業ツリーに、音声やデータセットのファイルや、`bench/` の中のファイルがあると失敗します。例外は、音声の読み込みのテストに使う合成音（`crates/kikitori-audio/tests/fixtures/`）と、`tools/truth_edit/` の見本の音声だけです。
