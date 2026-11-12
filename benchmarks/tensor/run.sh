#!/usr/bin/env bash
# Runs the tensor benchmark on the CPU device and, on a Mac with a usable
# GPU, on the Metal device, then the same work in NumPy, PyTorch, and MLX
# where they are installed. Each row prints
# `<name> <size> <milliseconds> <checksum>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

"$AURA" build --backend direct -o "$OUT/ops_aura" "$HERE/ops.au" >/dev/null
echo "== aura, cpu device"
"$OUT/ops_aura"
echo "== aura, metal device"
"$OUT/ops_aura" metal || true

for side in numpy torch-cpu torch-mps mlx; do
  echo "== $side"
  python3 "$HERE/ops.py" "$side" || true
done
