#!/bin/sh
# kikitori を入れ、文字起こしと補正の環境を作る（macOS・Linux）。
#   curl -fsSL https://github.com/kulikala/kikitori/releases/latest/download/install.sh | sh
# 環境変数:
#   KIKITORI_NO_REFINE=1  補正（kikitori-refine の Qwen3-ASR・kana-whisper・Ollama）を入れない
#   KIKITORI_NO_HF=1      Hugging Face のモデル（約 6.8GB）を先に取得しない
#   KIKITORI_YES=1        入れる前の確認を省く（端末の無い環境・CI 用）
#   KIKITORI_UPGRADE=1    既に入っている kikitori を、この版で入れ直す
#   KIKITORI_WHEEL=...    入れる wheel（パスか URL）。既定はこのスクリプトと同じ Release の wheel
set -eu

WHEEL="${KIKITORI_WHEEL:-__WHEEL_URL__}"
say() { printf '%s\n' "$*" >&2; }
die() { say "kikitori: $*"; exit 1; }

case "$WHEEL" in
  __WHEEL*) die "wheel の場所が決まっていません。Release の install.sh を使うか、KIKITORI_WHEEL を指定してください" ;;
  http://* | https://* | file://*) ;;
  /*) WHEEL="file://$WHEEL" ;;
  *) WHEEL="file://$(pwd)/$WHEEL" ;;
esac

OS="$(uname -s)"
EXTRAS=""
TORCH=""
REFINE=1
[ "${KIKITORI_NO_REFINE:-}" = "1" ] && REFINE=0
case "$OS" in
  Darwin)
    if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" != "1" ]; then
      say "Intel の Mac では補正（torch）が入らないため、文字起こしだけを入れます"
      REFINE=0
    fi
    ;;
  Linux)
    if command -v nvidia-smi >/dev/null 2>&1; then EXTRAS="cuda"; else EXTRAS="cpu"; fi
    TORCH="--torch-backend auto"
    ;;
  *) die "この OS（$OS）には install.sh は対応していません。Windows は install.ps1 を使ってください" ;;
esac
[ "$REFINE" = "1" ] && EXTRAS="${EXTRAS:+$EXTRAS,}refine"

# uv（Python と道具の管理）。入れたばかりの uv は今のシェルの PATH に無いので、絶対パスで呼ぶ
if command -v uv >/dev/null 2>&1; then
  UV="$(command -v uv)"
else
  say "uv を入れます（https://astral.sh/uv）"
  curl -LsSf https://astral.sh/uv/install.sh | sh >&2
  UV="${XDG_BIN_HOME:-$HOME/.local/bin}/uv"
  [ -x "$UV" ] || die "uv を入れられませんでした"
fi

SPEC="kikitori${EXTRAS:+[$EXTRAS]} @ $WHEEL"
FORCE=""
[ "${KIKITORI_UPGRADE:-}" = "1" ] && FORCE="--force"
say "kikitori を入れます: $SPEC"
# shellcheck disable=SC2086  # FORCE と TORCH は空か 1 語以上の引数
"$UV" tool install --python 3.12 $FORCE $TORCH "$SPEC" >&2

BIN="$("$UV" tool dir --bin)"
"$BIN/kikitori" --setup

STATUS=0
if [ "$REFINE" = "1" ]; then
  set -- --setup
  [ "${KIKITORI_YES:-}" = "1" ] && set -- "$@" --yes
  [ "${KIKITORI_NO_HF:-}" = "1" ] && set -- "$@" --no-hf
  "$BIN/kikitori-refine" "$@" || STATUS=$?
fi

say ""
say "== kikitori の導入の結果 =="
if [ "$STATUS" = "0" ]; then say "済み: kikitori$( [ "$REFINE" = "1" ] && echo ' と kikitori-refine の環境' )"
else say "未完了: 補正の環境づくりが終わっていません。上の案内のあと、$BIN/kikitori-refine --setup を打ち直してください"; fi
if ! command -v ffmpeg >/dev/null 2>&1; then
  if [ "$OS" = "Darwin" ]; then HOW="brew install ffmpeg"; else HOW="sudo apt-get install -y ffmpeg（など、OS のパッケージ管理で）"; fi
  say "未完了: ffmpeg がありません。入れるまで文字起こしできません: $HOW"
fi
case ":$PATH:" in
  *":$BIN:"*) ;;
  *) say "PATH に $BIN がありません。$UV tool update-shell を打ち、端末を開き直してください" ;;
esac
exit "$STATUS"
