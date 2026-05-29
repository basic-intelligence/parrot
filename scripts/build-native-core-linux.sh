#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORE_DIR="$ROOT_DIR/native-core/linux"
OUTPUT_PATH="$ROOT_DIR/src-tauri/binaries/parrot-core-cpu-x86_64-unknown-linux-gnu"
SOURCE_PATH="$ROOT_DIR/target/native-core-release/parrot-core"
TARGET_DIR="$ROOT_DIR/target/native-core-release"
OUTPUT_DIR="$(dirname "$OUTPUT_PATH")"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Linux sidecar builds must run on Linux so the sidecar matches x86_64-unknown-linux-gnu." >&2
  exit 1
fi

if [[ "$(uname -m)" != "x86_64" ]]; then
  echo "Linux sidecar builds must run on x86_64 Linux for the first Linux release." >&2
  exit 1
fi

if [[ -d "$TARGET_DIR" ]]; then
  find "$TARGET_DIR" -maxdepth 2 \( -name "libllama.so*" -o -name "libggml*.so*" \) -exec rm -f {} +
fi

export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,--disable-new-dtags -C link-arg=-Wl,-rpath,\$ORIGIN"
cargo build --manifest-path "$CORE_DIR/Cargo.toml" --profile native-core-release

mkdir -p "$OUTPUT_DIR"
cp "$SOURCE_PATH" "$OUTPUT_PATH"
chmod +x "$OUTPUT_PATH"
rm -f "$OUTPUT_DIR"/libllama.so* "$OUTPUT_DIR"/libggml*.so*

shopt -s nullglob
for library in "$TARGET_DIR"/build/llama-cpp-sys-2-*/out/lib/lib*.so*; do
  cp -P "$library" "$OUTPUT_DIR/"
done
shopt -u nullglob

echo "$OUTPUT_PATH"
