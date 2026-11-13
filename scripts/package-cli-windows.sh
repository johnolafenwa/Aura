#!/usr/bin/env bash
# Packages the Windows release archive. Run it from Git Bash after
# `cargo build -p aura --release --locked`.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/package-cli-windows.sh <archive-name>" >&2
  exit 2
fi

archive_name="$1"
if [[ ! "$archive_name" =~ ^aura-v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?-x86_64-pc-windows-msvc$ ]]; then
  echo "invalid release archive name: $archive_name" >&2
  exit 2
fi
archive_root="release/$archive_name"
python="$(command -v python3 || command -v python)"
native_static_libs="$(CARGO_TERM_COLOR=never cargo rustc -q -p aura-runtime --lib --release --locked -- --print native-static-libs 2>&1 >/dev/null)"
rm -rf "$archive_root" "$archive_name.zip"
mkdir -p "$archive_root/bin" "$archive_root/lib/aura" "$archive_root/examples/agents"
cp target/release/aura.exe "$archive_root/bin/aura.exe"
cp target/release/aura_runtime.lib "$archive_root/lib/aura/aura_runtime.lib"
cp examples/basic_addition.au "$archive_root/examples/basic_addition.au"
cp examples/agents/retrying_network_worker.au "$archive_root/examples/agents/retrying_network_worker.au"

NATIVE_STATIC_LIBS="$native_static_libs" \
  ARCHIVE_ROOT="$archive_root" \
  "$python" scripts/write-native-link-args.py

# The runtime names the import libraries of the Rust `windows-targets`
# crates without carrying them. The linker looks for them beside the runtime.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
cargo_home="$(cygpath -u "$cargo_home")"
"$python" -c 'import json, sys; print("\n".join(json.load(open(sys.argv[1], encoding="utf-8"))))' \
  "$archive_root/lib/aura/native-link-args.json" | tr -d '\r' |
  { grep -E '^windows\.[0-9.]+\.lib$' || true; } | sort -u |
  while IFS= read -r library; do
    found=""
    for candidate in "$cargo_home"/registry/src/*/windows_x86_64_msvc-*/lib/"$library"; do
      if [[ -f "$candidate" ]]; then
        found="$candidate"
        break
      fi
    done
    if [[ -z "$found" ]]; then
      echo "cannot find the import library $library in $cargo_home/registry" >&2
      exit 1
    fi
    cp "$found" "$archive_root/lib/aura/$library"
  done

cp README.md LICENSE "$archive_root/"
cp crates/aura/README.md "$archive_root/AURA_CLI_README.md"
(cd release && "$python" -m zipfile -c "../$archive_name.zip" "$archive_name")
