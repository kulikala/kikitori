#!/bin/sh
# Build and package one release binary of kikitori (macOS/Linux host).
#
# Usage: scripts/build-release.sh <target> [--cuda]
#
# Produces in dist/:
#   kikitori-<version>-<target>[-cuda].tar.gz
#       containing the binary, LICENSE, README.md and THIRD-PARTY-NOTICES
#   THIRD-PARTY-NOTICES (also left in dist/)
#   one line appended to dist/SHA256SUMS
#
# A smoke test (kikitori --version, kikitori doctor --json) runs on the
# unpacked archive with HOME and the model/cache directories pointed at a
# temp dir, so packaging and testing never touch the real HOME.
#
# Feature policy (D-4 of plan/02-設計.md):
#   aarch64-apple-darwin        -> metal,accelerate
#   x86_64-unknown-linux-gnu    -> none (CPU)
#   aarch64-unknown-linux-gnu   -> none (CPU)
#   x86_64-pc-windows-msvc      -> none (CPU); use build-release.ps1 on Windows
#   aarch64-pc-windows-msvc     -> none (CPU); use build-release.ps1 on Windows
#   --cuda on x86_64 Linux      -> cuda (cudarc loads libcuda at runtime)
set -eu
cd "$(dirname "$0")/.."

say() { printf '%s\n' "$*" >&2; }
die() { say "build-release: $*"; exit 1; }

usage() {
  say 'usage: scripts/build-release.sh <target> [--cuda]'
  say 'targets: aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu'
  say '         (windows-msvc targets are built with scripts/build-release.ps1)'
}

TARGET=
CUDA=
for arg in ${1:+"$@"}; do
  case "$arg" in
    --cuda) CUDA=1 ;;
    -h|--help) usage; exit 0 ;;
    -*) die "unknown argument: $arg" ;;
    *) [ -z "$TARGET" ] || die 'exactly one target is expected'; TARGET=$arg ;;
  esac
done
[ -n "$TARGET" ] || { usage; exit 2; }

is_unix_target() {
  case "$1" in
    aarch64-apple-darwin|x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) return 0 ;;
    *) return 1 ;;
  esac
}
if ! is_unix_target "$TARGET"; then
  case "$TARGET" in
    x86_64-pc-windows-msvc|aarch64-pc-windows-msvc)
      die "$TARGET is built with scripts/build-release.ps1 on a Windows host" ;;
    *) die "unsupported target: $TARGET" ;;
  esac
fi

FEATURES=
case "$TARGET" in
  aarch64-apple-darwin) FEATURES='metal accelerate' ;;
  *) : ;;
esac
if [ -n "$CUDA" ]; then
  [ "$TARGET" = x86_64-unknown-linux-gnu ] || die '--cuda is only offered for x86_64 Linux and x86_64 Windows'
  FEATURES="$FEATURES cuda"
  # The candle kernels are compiled as PTX for this compute capability; the driver
  # JIT-compiles PTX for newer GPUs. The kernels need 8.0 (Ampere) or newer.
  : "${CUDA_COMPUTE_CAP:=80}"
  export CUDA_COMPUTE_CAP
  # Link the CUDA runtime statically (libcudart_static.a) so the binary starts on
  # machines without the toolkit; cudarc opens libcuda, cuBLAS and cuRAND at run time.
  CUDA_ROOT=${CUDA_HOME:-/usr/local/cuda}
  [ -f "$CUDA_ROOT/lib64/libcudart_static.a" ] || die "libcudart_static.a not found under $CUDA_ROOT/lib64"
  CUDART_DIR=$(mktemp -d) || die 'mktemp failed'
  ln -s "$CUDA_ROOT/lib64/libcudart_static.a" "$CUDART_DIR/libcudart.a"
  RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-L native=$CUDART_DIR"
  export RUSTFLAGS
fi

command -v cargo >/dev/null 2>&1 || die 'cargo is not on PATH'
command -v jq >/dev/null 2>&1 || die 'jq is not on PATH'
METADATA=$(cargo metadata --format-version 1) || die 'cargo metadata failed'

VERSION=$(printf '%s\n' "$METADATA" | jq -r '.packages[] | select(.name == "kikitori") | .version')
[ -n "$VERSION" ] || die 'cannot determine the version of the kikitori package'
DECLARED=$(printf '%s\n' "$METADATA" | jq -r '[.packages[] | select(.name == "kikitori")][0].features | keys | join(" ")')

for f in $FEATURES; do
  case " $DECLARED " in
    *" $f "*) ;;
    *) die "the kikitori crate does not declare the feature $f" ;;
  esac
done

FLAGS=
[ -z "$FEATURES" ] || FLAGS="--features $(printf '%s' "$FEATURES" | tr ' ' ',')"
cargo build --release --target "$TARGET" -p kikitori $FLAGS \
  || die "cargo build for $TARGET failed"

BIN="${CARGO_TARGET_DIR:-target}/$TARGET/release/kikitori"
[ -f "$BIN" ] || die "binary not found: $BIN"

case "$CUDA" in
  1) ASSET="kikitori-$VERSION-$TARGET-cuda.tar.gz" ;;
  *) ASSET="kikitori-$VERSION-$TARGET.tar.gz" ;;
esac

DIST=dist
mkdir -p "$DIST"
STAGE=$(mktemp -d) || die 'mktemp failed'
SMOKE=$(mktemp -d) || die 'mktemp failed'
trap 'rm -rf "$STAGE" "$SMOKE" ${CUDART_DIR:+"$CUDART_DIR"}' EXIT HUP INT TERM

{
  printf 'kikitori %s (%s)\n' "$VERSION" "$ASSET"
  printf 'kikitori is under the Apache License 2.0 (LICENSE in this archive).\n\n'
  printf 'Statically linked third-party components:\n'
  printf '  - sherpa-onnx: Apache-2.0, https://github.com/k2-fsa/sherpa-onnx\n'
  printf '    (embeds ONNX Runtime, MIT, https://github.com/microsoft/onnxruntime)\n'
  if [ -n "$CUDA" ]; then
    printf '  - NVIDIA CUDA runtime (libcudart_static): CUDA Toolkit EULA,\n'
    printf '    https://docs.nvidia.com/cuda/eula/ (redistributable runtime library)\n'
  fi
  printf '  - every Rust dependency of this build, listed below as\n'
  printf '    name version<TAB>licence, taken from the licence fields of\n'
  printf '    the cargo metadata of this build.\n\n'
  printf '%s\n' "$METADATA" | jq -r '
    .packages[]
    | select(.name != "kikitori")
    | .name + " " + .version + "\t" + (.license // "license not specified")
  ' | sort -f -u
} > "$STAGE/THIRD-PARTY-NOTICES"

cp "$BIN" "$STAGE/kikitori"
cp LICENSE README.md "$STAGE/"
chmod +x "$STAGE/kikitori"
tar -czf "$DIST/$ASSET" -C "$STAGE" . || die "packaging $ASSET failed"

if command -v sha256sum >/dev/null 2>&1; then
  HASH=$(sha256sum "$DIST/$ASSET" | awk '{print $1}')
else
  HASH=$(shasum -a 256 "$DIST/$ASSET" | awk '{print $1}')
fi
[ -n "$HASH" ] || die "hashing $ASSET failed"
# Replace any previous entry for this asset, then append the fresh hash.
[ -f "$DIST/SHA256SUMS" ] || : > "$DIST/SHA256SUMS"
awk -v f="$ASSET" '!(NF>=2 && ($2==f || $2=="*"f))' "$DIST/SHA256SUMS" > "$DIST/SHA256SUMS.part"
mv "$DIST/SHA256SUMS.part" "$DIST/SHA256SUMS"
printf '%s  %s\n' "$HASH" "$ASSET" >> "$DIST/SHA256SUMS"
say 'SHA256SUMS:'
cat "$DIST/SHA256SUMS" >&2

# Smoke test from the unpacked archive, with HOME redirected to the temp dir.
tar -xzf "$DIST/$ASSET" -C "$SMOKE" || die "unpacking $ASSET failed"
HOME=$SMOKE/home
XDG_CONFIG_HOME=$HOME/.config
XDG_CACHE_HOME=$HOME/.cache
HF_HOME=$XDG_CACHE_HOME/huggingface
KIKITORI_MODELS=$HOME/.cache/kikitori/models
export HOME XDG_CONFIG_HOME XDG_CACHE_HOME HF_HOME KIKITORI_MODELS
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$KIKITORI_MODELS"
"$SMOKE/kikitori" --version || die 'kikitori --version failed on the unpacked archive'
"$SMOKE/kikitori" doctor --json >/dev/null || die 'kikitori doctor --json failed on the unpacked archive'

say "build-release: $DIST/$ASSET ready, sha256 $HASH"
