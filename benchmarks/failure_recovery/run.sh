#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
aura="${AURA:-$here/../../target/release/aura}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

"$aura" build --backend direct -o "$out/recover_aura" "$here/recover.au" >/dev/null
rustc -C opt-level=3 -C panic=unwind -o "$out/recover_rust" "$here/recover.rs"
measure=(python3 "$here/measure.py" "$out/recover_aura" "$out/recover_rust" "$here/recover.py")
if [[ -n ${AURA_BENCH_RUNNER:-} ]]; then
    "$AURA_BENCH_RUNNER" "${measure[@]}"
else
    "${measure[@]}"
fi
