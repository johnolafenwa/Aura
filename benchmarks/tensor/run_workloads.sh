#!/usr/bin/env bash
# Runs the tensor workloads on the CPU device and the GPU device, then the
# same workloads in PyTorch on each device it has. Pass `small` to run
# sizes that finish in seconds. Each row prints
# `<name> <size> <milliseconds> <result> <first pass milliseconds>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

"$AURA" build --backend direct -o "$OUT/workloads" "$HERE/workloads.au" >/dev/null
echo "== aura, gpu device"
"$OUT/workloads" gpu "$@" || true
for kind in cuda mps; do
  echo "== torch $kind"
  python3 "$HERE/workloads.py" "$kind" "$@" || true
done
if [ "${CPU:-0}" = 1 ]; then
  echo "== aura, cpu device"
  "$OUT/workloads" "$@"
  echo "== torch cpu"
  python3 "$HERE/workloads.py" cpu "$@" || true
fi
