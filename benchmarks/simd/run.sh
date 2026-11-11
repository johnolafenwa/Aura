#!/usr/bin/env bash
# Runs the Aura, Rust, NumPy, and plain Python sides of the Array kernel
# benchmark. Each prints `<kernel> <size> <milliseconds> <checksum>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

"$AURA" build --backend direct -o "$OUT/kernels_aura" "$HERE/kernels.au" >/dev/null
echo "== aura"
"$OUT/kernels_aura"
echo "== aura, baseline kernels"
AURA_ARRAY_KERNELS=baseline "$OUT/kernels_aura"

echo "== rust"
rustc -C opt-level=3 -o "$OUT/kernels_rust" "$HERE/kernels.rs"
"$OUT/kernels_rust"
echo "== rust, target-cpu=native"
rustc -C opt-level=3 -C target-cpu=native -o "$OUT/kernels_rust_native" "$HERE/kernels.rs"
"$OUT/kernels_rust_native"

echo "== numpy"
python3 "$HERE/kernels.py"
echo "== python, 1/100 of the repetitions"
python3 "$HERE/kernels.py" python
