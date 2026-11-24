#!/usr/bin/env bash
# Verify serial output and the shutdown protocol for each emulated board.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
aura="${1:-aura}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

cat > "$out/expected" <<'SERIAL'
breakpoint
timer
ok
SERIAL
echo "Booting x86_64" >&2
"$here/run.sh" x86_64 "$aura" | tee "$out/serial"
diff -u "$out/expected" "$out/serial"
"$here/test-boot.sh" "$aura"
