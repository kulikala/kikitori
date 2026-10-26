# 通信と書き込むもの

kikitori が PC の外とやり取りすること、起動するプログラム、書き込むファイルの一覧です。

kikitori は利用状況を送りません。アカウントも使いません。録音の音声は PC の外へ出ません。録音の文字が PC の外へ出るのは、`--external claude` を付けたときと、Ollama の接続先を別の PC にしたときだけです。`--external bundle` は補正のプロンプトをまとめた JSON を stdout に出すだけで、その JSON をどこへ渡すかは呼び出し元が決めます。

## 通信する先

| 先 | 何のために | いつ |
|---|---|---|
| GitHub（`github.com/kulikala/kikitori` の Release、`api.github.com`） | 実行ファイルとチェックサムの取得、最新版の確認 | インストール、`kikitori update` |
| sherpa-onnx の GitHub Release | 文字起こしのモデルの取得 | `kikitori setup`、モデルが無いときの最初の文字起こし |
| PyPI（`files.pythonhosted.org`） | 形態素解析の辞書 unidic-lite の取得 | `kikitori setup`、辞書が無いときの `--dict` |
| Hugging Face | 補正の B・K の取得（認証なし） | `kikitori refine --setup`、B・K が無いときの `kikitori refine` |
| Ollama（既定は `localhost:11434`） | LLM のモデルの取得、補正の本文の送信 | `kikitori refine --setup`、`kikitori refine` |
| `claude` コマンド（Anthropic） | 補正の本文の送信 | `--external claude` を付けたとき |

kikitori は Ollama を入れも起動もせず、利用者が入れて動かしている Ollama にだけつなぎます。LLM のモデルは、kikitori が Ollama に取得を頼み、Ollama が自分の取得元（Ollama のモデルのレジストリ）から取得します。`--external claude` の `claude` コマンドはツールを使わせず、会話を手元に保存しない設定で、OS の一時フォルダから起動します。送った本文を Anthropic がどう扱うかは、そのアカウントの契約で決まります。

## 起動するプログラム

- ffmpeg：Opus などの音声を読むとき。
- `claude`：`--external claude` を付けたとき。
- `nvidia-smi`：LLM のモデルを選ぶために、NVIDIA の GPU とメモリを調べるとき。
- `curl`・`shasum`（Windows は `certutil`）：`kikitori update` が新しい版を取得し、チェックサムを確かめるとき。
- `uv`：インストールスクリプトが `uv tool` で入れた Python 版を外すとき。

## 書き込むもの

| もの | 場所（macOS・Linux） | 場所（Windows） |
|---|---|---|
| 実行ファイル | `~/.local/bin/kikitori`、`kikitori-refine` | `%LOCALAPPDATA%\Programs\kikitori` |
| PATH の行 | シェルの設定ファイル（zsh は `~/.zshrc`、bash は `~/.bashrc`、ほかは `~/.profile`） | 利用者の環境変数 `Path` |
| インストール記録（削除のために、置いたものを書き留めるファイル） | `~/.config/kikitori/receipt.txt` | `%APPDATA%\kikitori\receipt.txt` |
| 文字起こしのモデル | `~/.cache/kikitori/models`（`KIKITORI_MODELS` で変更可） | `%USERPROFILE%\.cache\kikitori\models` |
| 形態素解析の辞書 | `~/.cache/kikitori/dict` | `%USERPROFILE%\.cache\kikitori\dict` |
| B・K | Hugging Face のキャッシュ（`~/.cache/huggingface/hub`） | 同じ |
| LLM のモデル | Ollama の置き場（Ollama が決める） | 同じ |
| スキル（`kikitori skill install`） | `~/.claude/skills/kikitori`・`~/.codex/skills/kikitori` | `%USERPROFILE%\.claude\skills\kikitori`・`%USERPROFILE%\.codex\skills\kikitori` |
| `kikitori update` が取得した新しい版 | OS の一時フォルダ（置き換えたあとは使わない） | 同じ |

モデルの置き場は `kikitori models path` で分かります。取得するファイルと配布元は `kikitori models list` で見られます。消し方は [INSTALL.md の削除](INSTALL.md#削除) にあります。
