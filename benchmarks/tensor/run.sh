#!/usr/bin/env bash
# Runs the tensor benchmark on the CPU device and, on a Mac with a usable
# GPU, on that device, then the same work in NumPy, PyTorch, and MLX
# where they are installed. Each row prints
# `<name> <size> <milliseconds> <checksum> <first pass milliseconds>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

"$AURA" build --backend direct -o "$OUT/ops_aura" "$HERE/ops.au" >/dev/null
echo "== aura, cpu device"
"$OUT/ops_aura"
echo "== aura, gpu device"
"$OUT/ops_aura" gpu || true

for side in numpy torch-cpu torch-mps torch-cuda mlx; do
  echo "== $side"
  python3 "$HERE/ops.py" "$side" || true
done
