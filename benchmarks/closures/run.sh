#!/usr/bin/env bash
# Runs the Aura, Rust, and Python sides of the stored closure benchmark. Each
# prints `<name> <calls> <milliseconds> <checksum>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
case "${1:-stored}" in
    stored)
        workload=closures
        set --
        ;;
    local)
        workload=local_functions
        set -- "${2:-7}" "${3:-48000000}"
        ;;
    *) echo "usage: $0 [stored|local [seed [count]]]" >&2; exit 2 ;;
esac
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

"$AURA" build --backend direct -o "$OUT/closures_aura" "$HERE/$workload.au" >/dev/null
echo "== aura"
"$OUT/closures_aura" "$@"

echo "== rust"
rustc -C opt-level=3 -o "$OUT/closures_rust" "$HERE/$workload.rs"
"$OUT/closures_rust" "$@"

echo "== python"
python3 "$HERE/$workload.py" "$@"
