#!/usr/bin/env bash
# Check the serial transcript and shutdown on both supported boards.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
aura="${1:?usage: test-boot.sh path-to-aura}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

line="$(awk '/^    small = small [+] 1$/ { print NR }' "$here/src/main.au")"
cat > "$out/expected" <<TEXT
p=168
c=111
r=1000
n=100
s=42
h=76127
f=0
trap 1 main.au:${line}:13
TEXT
for board in riscv64 aarch64; do
    echo "Booting $board"
    "$here/run.sh" "$board" "$aura" | tee "$out/serial"
    diff -u "$out/expected" "$out/serial"
done
