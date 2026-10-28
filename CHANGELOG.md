# Changelog

この形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版の付け方は [Semantic Versioning](https://semver.org/lang/ja/) に従います。
0.x の間は、利用者の手順や出力が変わる変更で minor を上げます。

## [Unreleased]

## [0.6.0] - 2026-10-29

### Changed

- 25 分を超える録音では、話者分離を無音の位置で約 25 分ずつの塊に分けて並列に処理し、塊をまたいで同じ人の声を結び付ける。68 分の合成の会議を 16 コアの Apple Silicon の Mac の CPU で起こすと、同じ機械・同じ既定の設定の 0.5.0 と比べて、処理の時間が約 40% 短くなり（276.6 秒から 166.3 秒）、最大メモリが 4.1GB から 3.6GB に減る。この会議の本文の文字誤り率は、0.5.0 が 9.26%、0.6.0 が 9.34% でほぼ同じ。`--num-speakers` と `--threshold` は、塊をまたいで結び付けるときに効く。25 分以下の録音の出力は 0.5.0 と変わらない。
- 塊の数と区切りの位置は録音の長さと無音の位置だけで決まる。コアの数が違う機械でも、同じ録音からは同じ話者の分け方が出る。コアの数はいくつの塊を同時に処理するかだけを決める。
- 25 分を超える録音では、話者の分け方と発言の区切りが 0.5.0 の出力とは変わる。話者の分け方の正確さが上がったか下がったかは、測っていない。`kikitori refine`（補正）が直す発言の単位も変わる。
- `kikitori transcribe` の `--threads` の既定を 4 から 3 にした。`kikitori refine` の既定は 4 のまま。ただし、既定の数がコアの数の 4 分の 3 を超える機械では、どちらもそこまで減らす（4 コアなら 3、2 コアなら 1）。
- 認識のスレッドが仕事を待つあいだ CPU を回し続けないようにした。出力は変わらず、CPU の使用時間が減る。
- `-v` を付けると、発話区間の検出・話者分離・認識のそれぞれにかかった時間を出す。
- 音声を 16 kHz にそろえるとき、元の標本化周波数の音声全体をメモリに持たないようにした。長い録音でメモリが減る。出力は変わらない。
- モデルのダウンロードに、ファイル全体での時間の上限を設けない。データが 60 秒届かなければ途切れたものとして続きから取り直し、進んでいるあいだは取り直しの回数を数えない。数 GB ある補正のモデルを、遅い回線でも取れる。
- `kikitori refine --setup` が Ollama に LLM のモデルを取らせるときも、全体での時間の上限をなくし、進みが 120 秒止まったときだけ失敗にする。
- `kikitori update` は curl・shasum・certutil が無い機械でも動く。これらを使わずに、kikitori の中でダウンロードと SHA-256 の照合を行う。最新の版はインストールスクリプトと同じく、GitHub の `releases/latest` の転送先から決める。

### Fixed

- `kikitori refine` が根拠にする 2 つの音声認識（Qwen3-ASR と kana-whisper）が、`--threads` を無視して 4 スレッドで動いていた。`--threads` の数で動かす。
- `OLLAMA_HOST`（または `KIKITORI_OLLAMA`）に `[::1]:11434` のような IPv6 の接続先を指定すると、手元の Ollama を別の機械のものとして扱い、補正の準備で手元の機械の GPU とメモリを調べずに進めていた。
- 補正の準備で空き容量を調べるとき、`HF_HUB_CACHE` と `OLLAMA_MODELS` を見ていなかった。Linux で Ollama を systemd のサービスとして動かしているときは、サービスのモデルの置き場を見る。
- インストールスクリプトで kikitori をアンインストールするとき、補正の LLM のモデルを Ollama から消す処理が、Ollama の接続先を `localhost:11434` に決め打ちしていたため、ほかの接続先の Ollama ではモデルが消えずに残っていた。`KIKITORI_OLLAMA`、`OLLAMA_HOST`、`localhost:11434` の順に決める。macOS・Linux のスクリプトは、モデルの置き場を `KIKITORI_MODELS` と `XDG_CACHE_HOME` に従って決める。
- Windows で、npm で入れた Claude Code（`claude.cmd`）を `--external claude` が見つけられなかった。`PATHEXT` の拡張子を付けて探す。
- `--external claude` で `claude` の出力が多いと、出力を読み出さないまま待ち続け、時間切れまで止まることがあった。
- Windows の PowerShell 5.1 で、インストールスクリプトのダウンロードが進捗の表示のために遅くなっていた。TLS 1.2 も有効にする。

## [0.5.0] - 2026-10-27

### Changed

- 文字起こしは、コアが足りてメモリが 12GB 以上ある機械では、認識器を 2 つ読み込んで区間を並列に起こす。出力は 1 つで起こしたときと同じ。68 分の会議を Apple Silicon の Mac の CPU で起こすと、処理の時間が約 15% 短くなり（300.8 秒から 256.8 秒）、最大メモリが約 1GB 増える（3.06GB から 4.05GB）。`kikitori refine` の中の文字起こしは、補正のモデルもメモリに載せるので 1 つのまま。
- 話者の数を指定したとき（`--num-speakers`）の話者の特徴の計算を、並列に行う。出力は変わらない。
- 文字起こしの本文に句読点を付ける。形態素解析の辞書（unidic-lite）で品詞と活用を調べ、文が終わる形に句点を、つなぎの語のあとに読点を付ける。文が終わる形のあとに名詞が続く所は、音声に 0.15 秒以上の無音があるときだけ句点を付ける（既定の `--model ja`・`--precision fp32` のとき）。`--no-punctuation` で付けない。`kikitori refine` の中の文字起こしには付けない。辞書は `kikitori setup` で取得され、無いときは句読点を付けずに起こして警告を出す。`--dict` の置き換えは、句読点を付けた後に行う。句読点を付ける処理で、68 分の会議の処理の時間は約 2 秒、最大メモリは約 0.2GB 増える。

## [0.4.0] - 2026-10-27

### Changed

- `kikitori skill install` は、SKILL.md に加えて `references/` の手順書（インストール・文字起こし・補正）も書く。SKILL.md には判断に必要なことだけを残した。
- `kikitori refine --setup`（インストールスクリプトの補正の段を含む）は、Ollama をインストールも起動もしない。Ollama が無いか止まっているときは案内を出し、補正の 2 つの音声認識モデル（Qwen3-ASR と kana-whisper）だけをダウンロードする。Ollama をインストールするかどうかは利用者が決める。
- README を短い入口にし、細部を `docs/INSTALL.md`・`docs/USAGE.md`・`docs/DISCLOSURES.md`・`docs/MODELS.md` に分けた。README には、特長、kikitori が受け持つ範囲、対応環境の表、必要なディスク容量、別途必要なソフトウェアを書いた。

### Fixed

- 文字起こしで、長い区間の前半が落ちていた。1 人が 20 秒以上話し続けると、区間の末尾の数文だけが残ることがあった。区間の頭の文も落ちやすかった。0.3.0 からの不具合。認識に渡す区間が 20 秒を超えたら無音の位置で分け（25 秒を超えて無音が無ければ、いちばん静かな位置で分ける）、区間の前後に足す余白を 0.5 秒から 1 秒に延ばした。
- 補正の容量の表示と見積もりが 1024 単位（GiB）で、文字起こしや `kikitori doctor` の 1000 単位（GB）と混ざっていた。どれも 1000 単位にそろえる。補正の 2 つの音声認識モデルは約 6.8GB と出ていたが、実際は約 7.3GB。LLM のモデルの大きさは、Ollama の表示と同じ数え方になる。
- `kikitori update` が、更新前の版の持つ古い SKILL.md を書き戻していた。置き換えた新しい版にスキルを書き直させる。0.3.0 から更新するときは、動くのが 0.3.0 の処理なので、スキルは古いまま残る。更新したあとで `kikitori skill install` を実行し直す。
- macOS・Linux のインストールスクリプトが、`uv tool` でインストールした Python 版を外してよいかを標準入力に尋ねていたため、`curl … | sh` の形では答えられず止まっていた。TTY（`/dev/tty`）に直接尋ねる。
- `kikitori setup`・`kikitori models pull`・`kikitori uninstall` などの確認が標準入力を読んでいたため、`curl … | sh` でインストールすると、文字起こしのモデルのダウンロードの確認に答えられず止まっていた。標準入力がパイプのときは、TTY に直接尋ねる（Windows では尋ねず、`--yes` を求めて終了コード 6 で終わる）。確認の質問の文が表示されないこともあったので、表示されるようにした。
- `kikitori transcribe --dry-run`（`kikitori 音声 --dry-run` も）と `kikitori refine --dry-run` が、`--dry-run` を無視して文字起こしと補正を始めていた。入力、使うエンジン、モデルが取得済みかを出して終える（`refine` は `--plan` と同じく直し方を出して終える）。
- `kikitori update --dry-run` が `--dry-run` を無視して更新していた。更新先の版と取得するファイルを出すだけにする。
- `kikitori uninstall` が、補正の 2 つの音声認識モデルの Hugging Face のキャッシュで、参照（snapshots）だけを消し、実体（blobs、約 7.3GB）を残していた。ほかのスナップショットが使っていない実体も消す。
- `kikitori doctor` と `kikitori uninstall` が、Ollama の接続先を `localhost:11434`（`uninstall` は `OLLAMA_HOST` だけ）に決め打ちしていた。補正と同じく `KIKITORI_OLLAMA`、`OLLAMA_HOST`、`localhost:11434` の順に決める。

## [0.3.0] - 2026-10-26

Python 版から Rust の 1 本の実行ファイルに置き換わりました。Python と uv は要りません。インストールの 1 行（[README](README.md#インストール)）は、Python 版 0.2.x が入っていれば外してから入れます。モデルは取り直しません。

### Python 版 0.2.x からの移行

| 0.2.x | 0.3.0 |
|---|---|
| `kikitori 会議.m4a` | `kikitori transcribe 会議.m4a`（`kikitori 会議.m4a` も動く） |
| `kikitori-refine …` | `kikitori refine …`（`kikitori-refine` は 0.3 の間だけ、転送する名前として残る） |
| 話者分離だけが見つけた発話区間が重なって残る（0.2.x の動作） | 修正済み。0.2.x と同じ動作は `--python-compat` |
| `kikitori --setup` | `kikitori setup`（旧い形も動き、警告で新しい書き方を案内する） |
| `kikitori-refine --setup` | `kikitori refine --setup` |
| `--device coreml`・`directml`・`migraphx`・`rocm`・`sherpa` | `--device auto`・`cpu`・`metal`・`cuda`。旧い値は警告を出して読み替える（`coreml`・`directml`・`migraphx`・`rocm` は `auto`、`sherpa` は `cpu`）。旧い `--models DIR` は、値をそのまま使い（そこを置き場にする）、警告で環境変数 `KIKITORI_MODELS` を案内する |
| 失敗の終了コードは 1 | 種類ごとに 2〜11（`kikitori --help` に一覧。使い方の誤りは 2） |
| 入力なしの `kikitori` は 0 | 2 |
| 更新はインストールの 1 行に `KIKITORI_UPGRADE=1` | `kikitori update`（`KIKITORI_UPGRADE` は受け付けて無視する） |
| 取り外しは `uv tool uninstall kikitori` と、モデルを手で削除 | `kikitori uninstall`（`--dry-run` で一覧、`--keep-models`） |
| `KIKITORI_NO_HF=1` | `KIKITORI_NO_REFINE=1` と同じ扱い |
| モデルの置き場 `~/.cache/kikitori/models` | そのまま |
| `--format` は txt・json・yaml・srt・vtt | `json-v1` を追加。`json` の形はそのまま |
| ONNX Runtime の GPU（CoreML・CUDA・DirectML・ROCm）で文字起こし | 文字起こしの本体は CPU。GPU は補正の B・K が Metal と CUDA で使う。Apple Silicon の CoreML は CPU と同等かやや遅かった |

### Added

- 0.2.x にあった旗は同じ名前のまま使える。`kikitori transcribe` は `--model`・`--dict`・`--num-speakers` など、`kikitori refine` は `--glossary`・`--plan`・`--hardware`・`--external`・`--save-evidence` などに加えて `--model`・`--dict`・`--num-speakers` も受ける。
- `kikitori` が Rust の 1 本の実行ファイルになった。macOS（Apple Silicon）、Linux（x86_64・arm64）、Windows（x64・arm64）に対応する。Linux x86_64 と Windows x64 には CUDA 版もある（GPU は compute capability 8.0 以上、CUDA 12 の cuBLAS と cuRAND が要る。使えなければ CPU で動く。NVIDIA の実機では確かめていない）。
- 補正の B・K（Qwen3-ASR と kana-whisper の 2 つの認識モデル）が、Python と torch なしで、CPU・Metal・CUDA で動く。Windows arm64 でも補正が使える。
- `kikitori doctor`：使える計算機、取得済みのモデルと容量、ffmpeg と Ollama、補正の直し方の見込み、空きメモリを出す。`--json` で機械向け。
- `kikitori update`：最新版への更新。起動しない版は元に戻す。モデルは触らない。
- `kikitori uninstall`：実行ファイル、PATH の行、モデル、スキル、kikitori が取得した Ollama のモデルを、一覧を出してから消す。
- `kikitori models list|pull|remove|path`、`kikitori engines list`、`kikitori schema <名前>`、`kikitori skill install|uninstall|path`。
- `--log-format json`：stderr を 1 行 1 件の JSON にする。失敗の行は `code`・`message`・`hint` を持つ。
- 終了コードを種類ごとに分けた（2〜11）。
- `--yes`・`--dry-run`・`--offline`。端末が無いときは確認を待たず、終了コード 6 で終わる。取得の前にディスクの空きを確かめ、足りなければ終了コード 7 で空きと要る量を出す。
- `--format json-v1`：`schema_version` と `segments` を持つ出力。
- `--asr`・`--diarizer`・`--engine-b`・`--engine-k`：認識と話者分離のエンジンを名前で選ぶ。足し方は `docs/ENGINES.md`。
- `kikitori refine` は、B・K の読み込み前にメモリを見積もり、足りなければ終了コード 11 で止まる（`--dtype fp16` か `--force` で続ける）。
- m4a・mp4・mp3・wav・flac・ogg を、ffmpeg なしで読む（m4a は AAC-LC と HE-AAC の v1・v2。Opus は ffmpeg が要る）。
- Windows arm64 でも `--dict` が使える。

### Changed

- 補正の B・K が CPU で Python 版より速い。Linux arm64 の CPU で、378.6 秒の音声 38 本について B が 477.4 秒から 402.1 秒、K が 244.1 秒から 177.6 秒になった。
- 処理が少し速くなり、メモリが減った。公開の合成音声と朗読音声をつないだ 67 分の会議を Apple Silicon の Mac の CPU で処理して、Python 版の 310.9 秒・3.53GB から 279.7 秒・3.05GB になった。
- 発話区間の検出では見つからず、話者分離でだけ見つかった区間が、ほかの区間と重なったまま残る不具合を直した。直す前の動作（Python 版と同じ）は `--python-compat` で選べる。
- 音声の読み込みが変わったため、本文と話者の割り当てが Python 版と完全には一致しない。上と同じ 67 分の会議で、Python 版の出力を正解とみなすと、本文の食い違い（句読点と空白を除いた文字誤り率）が 4.82%、話者の一致（話者番号の対応を最もよく取ったときに、一致する時間の割合）が 80.5% だった。正解の文章に対する文字誤り率は Python 版 11.26%、0.3.0 が 11.00% だった（句読点と空白を除いて採点）。
- インストールは、`uv tool` 経由の Python 版から、Release の実行ファイルをチェックサムで照合して置く方式になった。管理者権限は要らない。

### Removed

- Python 版（`uv tool` で入る kikitori）。インストールの 1 行が外す。pip など別の方法で入れたものは、`pip uninstall kikitori` などで自分で外す。外さずにインストールの 1 行を打つと、PATH で先に見つかる `kikitori` が新しいものでないとき、その場所を表示して終了コード 8 で終わる。
- Intel の Mac 向けの実行ファイル。Intel の Mac でインストールの 1 行を打つと「Intel Macs are not supported by kikitori 0.3; keep using 0.2.x」と出て止まる。0.2.x を使い続ける。
- `KIKITORI_UPGRADE`（受け付けるが働かない）。
- ONNX Runtime の GPU を使う文字起こし（CoreML・CUDA・DirectML・MIGraphX・ROCm）。

## [0.2.1] - 2026-10-23

### Fixed

- Windows arm64 でインストールが止まらないようにした。CPU 版の onnxruntime を入れ、補正は入れない（補正が使う部品に ARM 版 Windows 用の配布物が無い）。辞書の置換（`--dict`）に使う fugashi（形態素解析のライブラリ）も入れず、`--dict` は案内を出して終わる。
- Windows PowerShell 5.1 で `irm … | iex` したとき、インストールスクリプトの日本語が化けないようにした。`install.ps1` は ASCII だけの入口になり、本体の `install-windows.ps1` を UTF-8 として読んで実行する。
- Windows のインストールスクリプトが、終わったあとに利用者の PowerShell を閉じないようにした。
- macOS で、Ollama の公式のインストールスクリプトが入れた直後の起動に失敗しても、`kikitori-refine --setup` が止まらず、Ollama を起動して続けるようにした。

## [0.2.0] - 2026-10-23

最初の公開版です。

### Added

- `kikitori`: 日本語の会議録音を、話者ごとに分けて文字起こしする。ReazonSpeech（認識）、Silero VAD（発話区間）、pyannote segmentation-3.0 と CAM++（話者）を使い、処理はすべて手元で完結する。
- `kikitori-refine`: 同じ区間を Qwen3-ASR と kana-whisper でも認識し、それを根拠に LLM で本文を直す。外部 LLM（呼び出し元の LLM か claude CLI）、Ollama の 27B での突き合わせ、tev1（判定用の小さなモデル）での用語の置き換え、長さの規則（B が A より十分に長い区間だけ B を採る）の中から、環境に合うものを選ぶ。
- `kikitori-refine --setup`: Ollama が無ければ入れて起こし、PC の機器（Apple Silicon・NVIDIA・それ以外）とメモリに合うモデルを選んで取得する。
- `install.sh`・`install.ps1`: 1 行で kikitori を入れ、環境を作る。
- 出力形式 txt・json・yaml・srt・vtt。
- `--version`。
