#!/usr/bin/env bash
# Runs the Aura, Rust, and NumPy sides of the Array element benchmark. Each
# prints `<name> <size> <milliseconds> <checksum>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

"$AURA" build --backend direct -o "$OUT/elements_aura" "$HERE/elements.au" >/dev/null
echo "== aura"
"$OUT/elements_aura"

echo "== rust"
rustc -C opt-level=3 -o "$OUT/elements_rust" "$HERE/elements.rs"
"$OUT/elements_rust"

echo "== numpy"
python3 "$HERE/elements.py"
