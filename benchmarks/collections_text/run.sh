#!/usr/bin/env bash
set -eu -o pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

"${AURA:-$root/target/release/aura}" build --backend direct -o "$out/aura-bench" "$here/collections_text.au" >/dev/null
rustc --edition 2021 -O "$here/collections_text.rs" -o "$out/rust-bench"

for language in aura rust python
do
    echo "== $language"
    if [[ $language == python ]]; then
        python3 "$here/collections_text.py" > "$out/$language.txt"
    else
        "$out/$language-bench" > "$out/$language.txt"
    fi
    cat "$out/$language.txt"
    awk '{print $1, $3}' "$out/$language.txt" > "$out/$language.checks"
    diff -u "$out/aura.checks" "$out/$language.checks"
done
