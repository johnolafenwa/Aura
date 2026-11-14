#!/usr/bin/env bash
# Packages the files `aura build --target <target>` reads from an
# installation (ADR-0101) as a target pack:
#
#   scripts/package-target.sh aura-v0.16.0-preview-target-x86_64-unknown-linux-musl
#
# The pack unpacks over an installation prefix: its one top-level
# directory holds `lib/aura/targets/<target>`. A pack is built on a Linux
# machine of the target's architecture, after `cargo build -p aura
# --release`:
#
# - a glibc target's pack holds the release runtime library the CLI
#   archive of that machine holds;
# - a musl target's pack builds the runtime library for the target, which
#   needs `musl-gcc` and the Rust target, and adds the start files and the
#   C and unwind libraries a static link reads.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/package-target.sh <pack-name>" >&2
  exit 2
fi

pack_name="$1"
if [[ ! "$pack_name" =~ ^aura-v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?-target-((x86_64|aarch64)-unknown-linux-(gnu|musl))$ ]]; then
  echo "invalid target pack name: $pack_name" >&2
  exit 2
fi
target="${BASH_REMATCH[2]}"
arch="${BASH_REMATCH[3]}"
libc="${BASH_REMATCH[4]}"
if [[ "$(uname -s)" != Linux || "$(uname -m)" != "$arch" ]]; then
  echo "the pack for $target is built on a Linux machine of its architecture" >&2
  exit 2
fi

pack_root="release/$pack_name"
target_root="$pack_root/lib/aura/targets/$target"
rm -rf "$pack_root"
mkdir -p "$target_root"

if [[ "$libc" == musl ]]; then
  eval "$(scripts/cross-target-env.sh)"
  native_static_libs="$(CARGO_TERM_COLOR=never cargo rustc -q -p aura-runtime --lib --release --locked --target "$target" -- --print native-static-libs 2>&1 >/dev/null)"
  cp "target/$target/release/libaura_runtime.a" "$target_root/libaura_runtime.a"
  support="$(rustc --print sysroot)/lib/rustlib/$target/lib/self-contained"
  for file in crt1.o crti.o crtbegin.o crtend.o crtn.o libc.a libunwind.a; do
    cp "$support/$file" "$target_root/$file"
  done
  cp packaging/targets/MUSL-NOTICES.md "$target_root/NOTICES.md"
else
  native_static_libs="$(CARGO_TERM_COLOR=never cargo rustc -q -p aura-runtime --lib --release --locked -- --print native-static-libs 2>&1 >/dev/null)"
  cp target/release/libaura_runtime.a "$target_root/libaura_runtime.a"
fi

NATIVE_STATIC_LIBS="$native_static_libs" \
  ARCHIVE_ROOT="$pack_root" \
  LINK_ARGS_DIR="$target_root" \
  python3 scripts/write-native-link-args.py

# `aura` refuses a pack another build made: its runtime library would not
# hold the functions that compiler calls.
target/release/aura --version > "$target_root/aura-version"

cp LICENSE "$target_root/LICENSE"
tar -czf "$pack_name.tar.gz" -C release "$pack_name"
echo "$pack_name.tar.gz"
