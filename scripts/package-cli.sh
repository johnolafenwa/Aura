#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/package-cli.sh <archive-name>" >&2
  exit 2
fi

archive_name="$1"
if [[ ! "$archive_name" =~ ^aura-v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?-(x86_64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin)$ ]]; then
  echo "invalid release archive name: $archive_name" >&2
  exit 2
fi
archive_root="release/$archive_name"
native_static_libs="$(CARGO_TERM_COLOR=never cargo rustc -q -p aura-compiler --lib --release --locked -- --print native-static-libs 2>&1 >/dev/null)"
rm -rf "$archive_root"
mkdir -p "$archive_root/bin" "$archive_root/lib/aura" "$archive_root/examples/agents"
cp target/release/aura "$archive_root/bin/aura"
cp target/release/libaura_compiler.a "$archive_root/lib/aura/libaura_compiler.a"
cp examples/basic_addition.au "$archive_root/examples/basic_addition.au"
cp examples/agents/retrying_network_worker.au "$archive_root/examples/agents/retrying_network_worker.au"

NATIVE_STATIC_LIBS="$native_static_libs" \
  ARCHIVE_ROOT="$archive_root" \
  python3 scripts/write-native-link-args.py

cp README.md LICENSE "$archive_root/"
cp crates/aura/README.md "$archive_root/AURA_CLI_README.md"
tar -czf "$archive_name.tar.gz" -C release "$archive_name"
