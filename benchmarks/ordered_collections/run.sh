#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
bench="$root/benchmarks/ordered_collections"
build=$(mktemp -d)
trap 'rm -rf "$build"' EXIT
"${AURA:-$root/target/release/aura}" build "$bench/ordered.au" -o "$build/ordered"
rustc -O "$bench/ordered.rs" -o "$build/reference"
for implementation in aura rust python; do
    echo "$implementation (milliseconds and checksum)"
    case "$implementation" in
        aura) "$build/ordered" ;;
        rust) "$build/reference" ;;
        python) python3 "$bench/ordered.py" ;;
    esac
done
