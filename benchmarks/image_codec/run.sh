#!/usr/bin/env bash
# Runs the Aura, Rust, and Python sides of the image benchmark. Each prints
# `<phase> <milliseconds> <check>` for a 1600 by 1200 picture. The host
# needs Python with Pillow and NumPy.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

python3 "$HERE/make_images.py" "$OUT"

echo "== aura"
"$AURA" build --backend direct -o "$OUT/image_codec_aura" "$HERE/image_codec.au" >/dev/null
"$OUT/image_codec_aura" "$OUT"

echo "== rust"
cargo build --quiet --release --manifest-path "$HERE/rust/Cargo.toml" --target-dir "$OUT/rust-target"
"$OUT/rust-target/release/image_codec" "$OUT"

echo "== python"
python3 "$HERE/image_codec.py" "$OUT"
