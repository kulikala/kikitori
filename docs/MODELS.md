# 精度とモデル

kikitori の文字起こしの精度と速さ、仕組み、使うモデルとそのライセンスです。補正の精度と速さは [USAGE.md](USAGE.md#補正kikitori-refine) にあります。

## 精度

文字起こしの CER（文字誤り率。低いほど正確）です。句読点と空白を除き、漢数字は算用数字にそろえて採点しています（正解と各ツールの出力の両方に同じ変換をしています）。各データ 100 本を、Apple Silicon の CPU で起こしました。

| データ | kikitori | Whisper large-v3-turbo | Whisper large-v3 |
|---|---|---|---|
| FLEURS 日本語（朗読） | 6.1% | **5.2%** | 5.4% |
| ReazonSpeech の評価用データ（放送の自然な発話）※ | **8.4%** | 10.9% | 13.5% |

※ 切り出した音声に、正解の範囲の外の発話が残っているため、この行はどのツールも、前後にはみ出した挿入を誤りに数えずに採点しています。

- 朗読では Whisper に及びません。自然な発話では上回りましたが、kikitori の認識モデル ReazonSpeech は、このデータと同じ系統のデータで学習しているので、このデータは kikitori に有利です。
- 会議の音声そのもので比べた値ではありません。どのツールが正確かを自分の録音で比べるなら、10〜30 分に手で正解を付けて比べるのが確実です。
- 話者分離の精度は測っていません。

## 速さ

68 分の合成の会議（公開の合成音声と朗読音声をつないだもの）を、Apple Silicon の Mac の CPU で、話者つきで最後まで起こしたときの値です。

| | 処理の時間 | 最大メモリ |
|---|---|---|
| kikitori 0.6.0 | 166.3 秒（音声の長さの 0.041 倍） | 3.61GB |

16 コアの Mac で、句読点を付ける処理を含めた値です。話者分離は 3 つの塊に分けて同時に処理し、認識のモデルは 2 つ読み込んで区間を並列に起こしています。2 つ読み込むのは、コアが足りてメモリが 12GB 以上ある機械です。コアの少ない機械では、塊を同時に処理する数と認識のモデルの数が減るので、時間は長くなります。話者の分け方と本文は、コアの数によらず同じです。

精度と速さは [benchmarks/README.md](../benchmarks/README.md) の道具で測っています（この節の会議は `hour`、精度の表は `fleurs` と `rz`）。

## 仕組み

1. 音声を 16 kHz のモノラルにそろえます。
2. Silero VAD（音声区間検出のモデル）で、声のある時間帯を見つけます。
3. pyannote segmentation-3.0 と CAM++（声の特徴を数値にする話者埋め込みのモデル）で、話者ごとの時間帯を求めます。25 分を超える録音は、無音の位置で約 25 分ずつの塊に分けて並列に処理し、塊をまたいで同じ人の声を結び付けます。
4. 声のある時間帯を、話者が交代する位置でだけ切ります。
5. 切った区間ごとに、ReazonSpeech（日本語の音声認識モデル）で文字にします。モデルは sherpa-onnx（ONNX 形式のモデルを動かすライブラリ）で動かします。
6. 形態素解析の辞書 unidic-lite で語の品詞と活用を調べ、句読点を付けます。文が終わる形のあとに名詞が続く所は、語の間に無音があるときだけ句点を付けます（既定の `--model ja` と `--precision fp32` のとき）。
7. 辞書（`--dict`）があれば置き換えます。

認識と話者分離のエンジンは名前で差し替えられます（`--asr`・`--diarizer`、補正は `--engine-b`・`--engine-k`）。使えるエンジンは `kikitori engines list` で分かり、足し方は [ENGINES.md](ENGINES.md) にあります。

## モデルとライセンス

| 用途 | モデル | ライセンス | 配布元 |
|---|---|---|---|
| 認識 | ReazonSpeech zipformer（日本語） | Apache-2.0 | sherpa-onnx の GitHub Release |
| 認識（日英混在、`--model ja-en`。約 440MB。使うときに取得） | ReazonSpeech zipformer 日英版 | Apache-2.0 | sherpa-onnx の GitHub Release |
| 話者ごとの時間帯 | pyannote segmentation-3.0（ONNX 版） | MIT | sherpa-onnx の GitHub Release |
| 話者の特徴 | 3D-Speaker CAM++ | Apache-2.0 | sherpa-onnx の GitHub Release |
| 声のある時間帯 | Silero VAD | MIT | sherpa-onnx の GitHub Release |
| 補正 B | Qwen3-ASR 1.7B 日本語版 | Apache-2.0 | Hugging Face |
| 補正 K | kana-whisper | MIT | Hugging Face |

モデルの重みは配布元のファイルをそのまま読みます。kikitori が重みを変換・量子化して配ることも、手元のキャッシュに変換したものを書くこともありません。Hugging Face 上の pyannote の公式リポジトリは利用条件への同意が要りますが、kikitori が使うのは sherpa-onnx が GitHub Release で配る ONNX 版で、認証なしで取得できます。

補正の LLM のモデルと形態素解析の辞書のライセンスは、それぞれの配布元が定めます。
