#!/usr/bin/env bash
# Runs the Aura, Rust, NumPy, and plain Python sides of the Array kernel
# benchmark, then the Aura, Rust, and Python sides of the list loop
# benchmark, and the same three sides of the lane value benchmark. Each
# prints `<name> <size> <milliseconds> <checksum>`.
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

"$AURA" build --backend direct -o "$OUT/loops_aura" "$HERE/loops.au" >/dev/null
"$AURA" build --backend direct --target-cpu native -o "$OUT/loops_aura_native" "$HERE/loops.au" >/dev/null
echo "== loops: aura"
"$OUT/loops_aura"
echo "== loops: aura, target-cpu=native"
"$OUT/loops_aura_native"
echo "== loops: rust"
rustc -C opt-level=3 -o "$OUT/loops_rust" "$HERE/loops.rs"
"$OUT/loops_rust"
echo "== loops: rust, target-cpu=native"
rustc -C opt-level=3 -C target-cpu=native -o "$OUT/loops_rust_native" "$HERE/loops.rs"
"$OUT/loops_rust_native"
echo "== loops: python, 1/100 of the repetitions"
python3 "$HERE/loops.py"

"$AURA" build --backend direct -o "$OUT/lanes_aura" "$HERE/lanes.au" >/dev/null
"$AURA" build --backend direct --target-cpu native -o "$OUT/lanes_aura_native" "$HERE/lanes.au" >/dev/null
echo "== lanes: aura"
"$OUT/lanes_aura"
echo "== lanes: aura, target-cpu=native"
"$OUT/lanes_aura_native"
echo "== lanes: rust"
rustc -C opt-level=3 -o "$OUT/lanes_rust" "$HERE/lanes.rs"
"$OUT/lanes_rust"
echo "== lanes: rust, target-cpu=native"
rustc -C opt-level=3 -C target-cpu=native -o "$OUT/lanes_rust_native" "$HERE/lanes.rs"
"$OUT/lanes_rust_native"
echo "== lanes: python, 1/100 of the repetitions"
python3 "$HERE/lanes.py"
