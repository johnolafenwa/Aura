#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
aura="${AURA:-$repo/target/release/aura}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
"$aura" build --backend direct -o "$out/aura-numbers" "$here/main.au" >/dev/null
cargo build --manifest-path "$here/Cargo.toml" --target-dir "$repo/target/numbers-comparison" --locked --release
python3 "$here/compare.py" "$out/aura-numbers" "$repo/target/numbers-comparison/release/numbers"
