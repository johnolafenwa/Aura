#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/package-cli.sh <archive-name>" >&2
  exit 2
fi

archive_name="$1"
archive_root="release/$archive_name"
native_static_libs="$(cargo rustc -q -p aurora-compiler --lib --release --locked -- --print native-static-libs 2>&1 >/dev/null)"
rm -rf "$archive_root"
mkdir -p "$archive_root/bin" "$archive_root/lib/aurora"
cp target/release/aura "$archive_root/bin/aura"
cp target/release/libaurora_compiler.a "$archive_root/lib/aurora/libaurora_compiler.a"

NATIVE_STATIC_LIBS="$native_static_libs" \
  ARCHIVE_ROOT="$archive_root" \
  python3 - <<'PY'
import json
import os
from pathlib import Path

marker = "native-static-libs:"
matching = [
    line.split(marker, 1)[1].strip()
    for line in os.environ["NATIVE_STATIC_LIBS"].splitlines()
    if marker in line
]
if not matching:
    raise SystemExit("rustc did not report native-static-libs")
target = Path(os.environ["ARCHIVE_ROOT"]) / "lib/aurora/native-link-args.json"
target.write_text(json.dumps(matching[-1].split()) + "\n", encoding="utf-8")
PY

cp README.md LICENSE "$archive_root/"
cp crates/aura/README.md "$archive_root/AURA_CLI_README.md"
tar -czf "$archive_name.tar.gz" -C release "$archive_name"
