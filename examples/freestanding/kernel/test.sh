#!/usr/bin/env bash
# Verify serial output and the shutdown protocol for each emulated board.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
aura="${1:-aura}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

cat > "$out/x86_64.expected" <<'SERIAL'
breakpoint
timer
ok
SERIAL
cat > "$out/virt.expected" <<'SERIAL'
p=168
c=111
r=1000
n=100
trap 1 main.au:154:13
SERIAL

for board in x86_64 riscv64 aarch64; do
    expected=virt
    if [[ "$board" == x86_64 ]]; then expected=x86_64; fi
    echo "Booting $board" >&2
    "$here/run.sh" "$board" "$aura" | tee "$out/serial"
    diff -u "$out/$expected.expected" "$out/serial"
done
