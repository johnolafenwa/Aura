#!/usr/bin/env bash
# Builds the five routines as freestanding objects with Aura, C, and Rust,
# links each with the same harness, and prints times and code sizes.
# Usage: benchmarks/freestanding/run.sh [path to aura]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
aura="${1:-aura}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

"$aura" build --freestanding -o "$out/aura.o" "$here/kernels.au" 2>/dev/null
clang -O2 -ffreestanding -fno-builtin -c "$here/kernels.c" -o "$out/c.o"
# Rust's division and overflow checks call into `core`, so the timed build
# is a static library that carries it. The size is that of the object alone.
for build in rs rsc; do
    checks=off
    if [ "$build" = rsc ]; then checks=on; fi
    sed "s/rs_/${build}_/g" "$here/kernels.rs" > "$out/$build.rs"
    for kind in obj link; do
        rustc --edition 2021 --crate-type staticlib --emit "$kind" -C opt-level=3 \
            -C panic=abort -C overflow-checks="$checks" --crate-name "$build" \
            --out-dir "$out" "$out/$build.rs"
    done
done

for build in aura c rs rsc; do
    object="$out/$build.o"
    if [ -f "$out/lib$build.a" ]; then object="$out/lib$build.a"; fi
    clang -O2 -DP="$build" "$here/harness.c" "$object" -lm -o "$out/$build"
    echo "== $build"
    "$out/$build"
    size "$out/$build.o" | tail -1 | awk '{ printf "code %d bytes, data %d bytes\n", $1, $2 + $3 }'
done
