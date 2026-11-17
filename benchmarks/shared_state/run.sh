#!/usr/bin/env bash
# Builds and runs the shared state benchmark in Aura, Rust, and Python.
# Each prints one line per phase: `<phase> <milliseconds> <checksum>`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

"${AURA:-$root/target/release/aura}" build "$here/shared_state.au" -o "$out/shared_state_aura"
rustc --edition=2021 -C opt-level=3 -C overflow-checks=on -o "$out/shared_state_rust" "$here/shared_state.rs"

export AURA_WORKERS="${AURA_WORKERS:-4}"

run() {
    local name="$1"
    shift
    echo "== $name"
    "$@" | tee "$out/$name.txt"
    awk '{$2=""; print}' "$out/$name.txt" > "$out/$name.checksums"
}

run Aura "$out/shared_state_aura"
run Rust "$out/shared_state_rust"
run Python python3 "$here/shared_state.py"
diff -u "$out/Rust.checksums" "$out/Aura.checksums"
diff -u "$out/Rust.checksums" "$out/Python.checksums"
