#!/usr/bin/env bash
# Builds the C library and runs the Aura, Rust, and Python sides of the FFI
# call-overhead benchmark. Each prints `<case> <nanoseconds per call>`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
AURA="${AURA:-$ROOT/target/release/aura}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

case "${1:-}" in
  "") aura_source=main; comparison=ffi_calls ;;
  v2) aura_source=v2; comparison=v2 ;;
  *) echo "usage: $0 [v2]" >&2; exit 2 ;;
esac

if [[ "$(uname)" == "Darwin" ]]; then
  LIBRARY="$HERE/native/libffi_calls.dylib"
  cc -O2 -dynamiclib -o "$LIBRARY" "$HERE/native/ffi_calls.c"
else
  LIBRARY="$HERE/native/libffi_calls.so"
  cc -O2 -shared -fPIC -o "$LIBRARY" "$HERE/native/ffi_calls.c"
fi

echo "== aura"
"$AURA" build --backend direct -o "$OUT/ffi_calls_aura" "$HERE/src/$aura_source.au" >/dev/null
"$OUT/ffi_calls_aura"

echo "== rust"
rustc -C opt-level=3 -L "$HERE/native" -C "link-arg=-Wl,-rpath,$HERE/native" \
  -o "$OUT/ffi_calls_rust" "$HERE/$comparison.rs"
"$OUT/ffi_calls_rust"

echo "== python"
python3 "$HERE/$comparison.py" "$LIBRARY"
