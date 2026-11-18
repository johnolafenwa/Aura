#!/usr/bin/env bash
# Runs the Aura, Rust, and Python sides of the stored closure benchmark. Each
# prints `<name> <calls> <milliseconds> <checksum>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

"$AURA" build --backend direct -o "$OUT/closures_aura" "$HERE/closures.au" >/dev/null
echo "== aura"
"$OUT/closures_aura"

echo "== rust"
rustc -C opt-level=3 -o "$OUT/closures_rust" "$HERE/closures.rs"
"$OUT/closures_rust"

echo "== python"
python3 "$HERE/closures.py"
