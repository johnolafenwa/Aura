#!/usr/bin/env bash
# Runs the Aura, Rust, and Python sides of the SQLite benchmark. Each
# prints `<phase> <milliseconds>` for 500,000 rows. The host needs the
# SQLite library and its linker file.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

echo "== aura"
"$AURA" build --backend direct -o "$OUT/ffi_sqlite_aura" "$HERE/src/main.au" >/dev/null
"$OUT/ffi_sqlite_aura"

echo "== rust"
rustc -C opt-level=3 -o "$OUT/ffi_sqlite_rust" "$HERE/ffi_sqlite.rs"
"$OUT/ffi_sqlite_rust"

echo "== python"
python3 "$HERE/ffi_sqlite.py"
