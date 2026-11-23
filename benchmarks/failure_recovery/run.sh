#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
aura="${AURA:-$here/../../target/release/aura}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

source_dir="$here"
case "${1:-literal}" in
    literal) ;;
    formatted)
        # Only the assertion message changes between the two workloads.
        source_dir="$out/formatted"
        mkdir "$source_dir"
        for extension in au py; do
            sed 's/"negative input"/f"negative input {value}"/' \
                "$here/recover.$extension" > "$source_dir/recover.$extension"
        done
        sed 's/"negative input"/"negative input {}", value/' \
            "$here/recover.rs" > "$source_dir/recover.rs"
        ;;
    *) echo "usage: bash run.sh [literal|formatted]" >&2; exit 2 ;;
esac

"$aura" build --backend direct -o "$out/recover_aura" "$source_dir/recover.au" >/dev/null
rustc -C opt-level=3 -C panic=unwind -o "$out/recover_rust" "$source_dir/recover.rs"
measure=(python3 "$here/measure.py" "$out/recover_aura" "$out/recover_rust" "$source_dir/recover.py")
if [[ -n ${AURA_BENCH_RUNNER:-} ]]; then
    "$AURA_BENCH_RUNNER" "${measure[@]}"
else
    "${measure[@]}"
fi
