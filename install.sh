#!/bin/sh
# Install or uninstall the kikitori release binary (macOS/Linux).
set -eu
say() { printf '%s\n' "$*" >&2; }
die() { say "kikitori: $*"; exit "${2:-1}"; }
HOME_DIR=${HOME:?HOME must be set}
CONFIG=${XDG_CONFIG_HOME:-$HOME_DIR/.config}/kikitori
RECEIPT=$CONFIG/receipt.txt
BIN_DIR=${KIKITORI_INSTALL_DIR:-$HOME_DIR/.local/bin}

remove_receipt() {
  [ -f "$RECEIPT" ] || return 0
  while IFS="$(printf '\t')" read -r kind place; do
    [ -n "${place:-}" ] || continue
    case "$kind" in
      path-line) [ -f "$place" ] && sed '/# added by kikitori/d' "$place" > "$place.kikitori-tmp" && mv "$place.kikitori-tmp" "$place" ;;
      bin|skill|dict) rm -f "$place" ;;
      models-dir|hf-repo) rm -rf "$place" ;;
      ollama-model)
        if command -v curl >/dev/null 2>&1 && curl -fsS -X DELETE http://localhost:11434/api/delete -H 'Content-Type: application/json' -d "{\"name\":\"$place\"}" >/dev/null 2>&1; then :;
        else say "Ollama is not running; kept $place. Remove it with: ollama rm $place"; fi ;;
      *) ;;
    esac
  done < "$RECEIPT"
  rm -rf "${KIKITORI_MODELS:-$HOME_DIR/.cache/kikitori/models}"
  rm -f "$RECEIPT"
}
if [ "${1:-}" = --uninstall ]; then remove_receipt; exit 0; fi

OS=$(uname -s); ARCH=$(uname -m); CUDA=${KIKITORI_VARIANT:-cpu}
case "$OS:$ARCH" in
  Darwin:arm64|Darwin:aarch64) TRIPLE=aarch64-apple-darwin; CUDA=cpu ;;
  Linux:x86_64|Linux:amd64) TRIPLE=x86_64-unknown-linux-gnu ;;
  Linux:aarch64|Linux:arm64) TRIPLE=aarch64-unknown-linux-gnu; CUDA=cpu ;;
  Darwin:x86_64) die "Intel Macs are not supported by kikitori 0.3; keep using 0.2.x (see the README)" ;;
  *) die "unsupported platform: $OS $ARCH" ;;
esac
SUFFIX=; [ "$CUDA" = cuda ] && { [ "$OS:$ARCH" = Linux:x86_64 ] || [ "$OS:$ARCH" = Linux:amd64 ] || die 'CUDA is supported only on Linux x86_64'; SUFFIX=-cuda; }
[ "$CUDA" = cpu ] || [ "$CUDA" = cuda ] || die 'KIKITORI_VARIANT must be cpu or cuda'
[ -z "${KIKITORI_UPGRADE:-}" ] || say 'KIKITORI_UPGRADE is ignored; installs replace the existing binary.'
[ -z "${KIKITORI_NO_HF:-}" ] || { say 'KIKITORI_NO_HF is treated as KIKITORI_NO_REFINE.'; [ "${KIKITORI_NO_HF}" != 1 ] || KIKITORI_NO_REFINE=1; }
VERSION=${KIKITORI_VERSION:-latest}
if [ -n "${KIKITORI_BASE_URL:-}" ]; then BASE=${KIKITORI_BASE_URL%/}; else
  REPO=${KIKITORI_REPO:-kulikala/kikitori}
  if [ "$VERSION" = latest ]; then
    # Resolve the latest version by following the redirect of /releases/latest
    # (the redirect target ends in /tag/vX.Y.Z); avoids the rate-limited API.
    REDIRECT=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") \
      || die 'could not resolve the latest version (network or repository problem)'
    VERSION=${REDIRECT##*/tag/v}
    [ "$VERSION" != "$REDIRECT" ] || die "could not resolve the latest version; redirect target: $REDIRECT"
    case "$VERSION" in *[!0-9.]*) die "unexpected latest tag: v$VERSION" ;; esac
    BASE="https://github.com/$REPO/releases/download/v$VERSION"
  else BASE="https://github.com/$REPO/releases/download/v$VERSION"; fi
fi
case "$BASE" in http://*|https://*) fetch() { curl -fsSL "$BASE/$1" -o "$2" || die "download failed: $1"; };; *) fetch() { cp "$BASE/$1" "$2" || die "missing release asset: $1"; };; esac
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT HUP INT TERM
if [ "$VERSION" = latest ]; then
  # A base URL without a version (a local dir most likely): pick the newest
  # matching asset and read the version from its name.
  ASSET=$(find "$BASE" -maxdepth 1 -name "kikitori-*-${TRIPLE}${SUFFIX}.tar.gz" -print | sort | tail -n 1 | sed "s|.*/||")
  [ -n "$ASSET" ] || die 'no matching asset; put kikitori-<version>-<target>.tar.gz and SHA256SUMS in the base URL'
  VERSION=$(printf '%s' "$ASSET" | sed "s/^kikitori-//;s/-${TRIPLE}${SUFFIX}.tar.gz$//")
else ASSET="kikitori-${VERSION}-${TRIPLE}${SUFFIX}.tar.gz"; fi
fetch "$ASSET" "$TMP/$ASSET"; fetch SHA256SUMS "$TMP/SHA256SUMS"
EXPECTED=$(awk -v f="$ASSET" '$2==f || $2=="*"f {print $1}' "$TMP/SHA256SUMS"); [ -n "$EXPECTED" ] || die 'asset missing from SHA256SUMS'
if command -v sha256sum >/dev/null 2>&1; then ACTUAL=$(sha256sum "$TMP/$ASSET" | awk '{print $1}'); else ACTUAL=$(shasum -a 256 "$TMP/$ASSET" | awk '{print $1}'); fi
[ "$EXPECTED" = "$ACTUAL" ] || die 'SHA-256 mismatch'
tar -xzf "$TMP/$ASSET" -C "$TMP" || die 'invalid archive'
chmod +x "$TMP/kikitori"; "$TMP/kikitori" --version >/dev/null || die 'downloaded binary failed --version'

# Python migration: inspect uv before replacing its symlink.
if command -v uv >/dev/null 2>&1 && uv tool list 2>/dev/null | grep -q '^kikitori '; then
  if [ "${KIKITORI_YES:-0}" != 1 ]; then [ -t 0 ] || die 'Python kikitori found in uv; rerun with KIKITORI_YES=1'; printf 'Remove Python kikitori from uv? [y/N] ' >&2; read answer; case "$answer" in y|Y|yes|YES) ;; *) die 'installation cancelled' ;; esac; fi
  uv tool uninstall kikitori || die 'uv tool uninstall failed'
fi
mkdir -p "$BIN_DIR" "$CONFIG"; DEST=$BIN_DIR/kikitori; OLD=$DEST.old
[ ! -L "$DEST" ] || rm -f "$DEST"
if [ -e "$DEST" ]; then rm -f "$OLD"; mv "$DEST" "$OLD"; fi
mv "$TMP/kikitori" "$DEST"
if ! "$DEST" --version >/dev/null 2>&1; then rm -f "$DEST"; [ ! -e "$OLD" ] || mv "$OLD" "$DEST"; die 'installed binary failed; previous version restored' 8; fi
if [ -e "$OLD" ]; then rm -f "$OLD"; fi
case ":$PATH:" in *":$BIN_DIR:"*) :;; *)
  RC=${KIKITORI_SHELL_RC:-}; [ -n "$RC" ] || { SH_NAME=${SHELL:-sh}; case "${SH_NAME##*/}" in zsh) RC=$HOME_DIR/.zshrc;; bash) RC=$HOME_DIR/.bashrc;; *) RC=$HOME_DIR/.profile;; esac; }
  if [ "${KIKITORI_NO_MODIFY_PATH:-0}" != 1 ]; then mkdir -p "$(dirname "$RC")"; printf '\nexport PATH="%s:$PATH" # added by kikitori\n' "$BIN_DIR" >> "$RC"; printf 'path-line\t%s\nbin\t%s\n' "$RC" "$DEST" >> "$RECEIPT"; fi;; esac
ln -sf kikitori "$BIN_DIR/kikitori-refine"; printf 'bin\t%s\nvariant\t%s\n' "$BIN_DIR/kikitori-refine" "$CUDA" >> "$RECEIPT"
printf 'bin\t%s\n' "$DEST" >> "$RECEIPT"
PATH="$BIN_DIR:$PATH"; export PATH
command -v kikitori >/dev/null 2>&1 || die "kikitori is not on PATH; add $BIN_DIR and retry" 8
FOUND=$(command -v kikitori); [ "$FOUND" = "$DEST" ] || die "another kikitori shadows the installed binary: $FOUND" 8
# The binary is in place; a stopped setup (no terminal, no space, offline) is reported, not fatal.
if [ "${KIKITORI_YES:-0}" = 1 ]; then SETUP_YES=--yes; else SETUP_YES=; fi
"$DEST" $SETUP_YES setup || say "Note: kikitori setup stopped (exit $?). Run 'kikitori setup' later to fetch the models."
if [ "${KIKITORI_NO_REFINE:-0}" != 1 ]; then
  "$DEST" refine --setup || say "Note: kikitori refine --setup stopped (exit $?). Run it later to prepare refine."
fi
