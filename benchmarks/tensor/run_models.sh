#!/usr/bin/env bash
# Runs GPT-2 with its real weights and the nanoGPT training program on
# the GPU device, then the same programs in PyTorch on each device it
# has. Pass the path of GPT-2's `model.safetensors` and of the training
# text, then `small` for sizes that finish in seconds. Each row prints
# `<name> <size> <milliseconds> <result> <first pass milliseconds>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
WEIGHTS="$1"
TEXT="$2"
shift 2
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

"$AURA" build --backend direct -o "$OUT/gpt2" "$HERE/gpt2.au" >/dev/null
"$AURA" build --backend direct -o "$OUT/nanogpt" "$HERE/nanogpt.au" >/dev/null
echo "== aura, gpu device"
"$OUT/gpt2" "$WEIGHTS" gpu "$@" || true
"$OUT/nanogpt" "$TEXT" gpu "$@" || true
for kind in cuda mps; do
  echo "== torch $kind"
  python3 "$HERE/gpt2.py" "$WEIGHTS" "$kind" "$@" || true
  python3 "$HERE/nanogpt.py" "$TEXT" "$kind" "$@" || true
done
if [ "${CPU:-0}" = 1 ]; then
  echo "== aura, cpu device"
  "$OUT/gpt2" "$WEIGHTS" "$@"
  "$OUT/nanogpt" "$TEXT" "$@"
  echo "== torch cpu"
  python3 "$HERE/gpt2.py" "$WEIGHTS" cpu "$@" || true
  python3 "$HERE/nanogpt.py" "$TEXT" cpu "$@" || true
fi
