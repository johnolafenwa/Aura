#!/usr/bin/env bash
# Runs the Aura, Rust, and NumPy sides of the Array view benchmark. Each
# prints `<name> <size> <milliseconds> <checksum>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

"$AURA" build --backend direct -o "$OUT/views_aura" "$HERE/views.au" >/dev/null
echo "== aura"
"$OUT/views_aura"

echo "== rust"
rustc -C opt-level=3 -o "$OUT/views_rust" "$HERE/views.rs"
"$OUT/views_rust"

echo "== numpy"
python3 "$HERE/views.py"
