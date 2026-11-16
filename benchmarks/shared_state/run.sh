#!/usr/bin/env bash
# Builds and runs the shared state benchmark in Aura, Rust, and Python.
# Each prints one line per phase: `<phase> <milliseconds> <checksum>`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
out="${TMPDIR:-/tmp}/aura-shared-state"
mkdir -p "$out"

"$root/target/release/aura" build "$here/shared_state.au" -o "$out/shared_state_aura"
rustc -C opt-level=3 -C overflow-checks=on -o "$out/shared_state_rust" "$here/shared_state.rs"

echo "== Aura"
"$out/shared_state_aura"
echo "== Rust"
"$out/shared_state_rust"
echo "== Python"
python3 "$here/shared_state.py"
