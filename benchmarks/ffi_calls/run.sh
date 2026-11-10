#!/usr/bin/env bash
# Builds the C library and runs the Aura, Rust, and Python sides of the FFI
# call-overhead benchmark. Each prints `<case> <nanoseconds per call>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

if [[ "$(uname)" == "Darwin" ]]; then
  LIBRARY="$HERE/native/libffi_calls.dylib"
  cc -O2 -dynamiclib -o "$LIBRARY" "$HERE/native/ffi_calls.c"
else
  LIBRARY="$HERE/native/libffi_calls.so"
  cc -O2 -shared -fPIC -o "$LIBRARY" "$HERE/native/ffi_calls.c"
fi

echo "== aura"
"$AURA" build --backend direct -o "$OUT/ffi_calls_aura" "$HERE/src/main.au" >/dev/null
"$OUT/ffi_calls_aura"

echo "== rust"
rustc -C opt-level=3 -L "$HERE/native" -C "link-arg=-Wl,-rpath,$HERE/native" \
  -o "$OUT/ffi_calls_rust" "$HERE/ffi_calls.rs"
"$OUT/ffi_calls_rust"

echo "== python"
python3 "$HERE/ffi_calls.py" "$LIBRARY"
