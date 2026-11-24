#!/usr/bin/env bash
# Prints the environment a source checkout needs to build the Aura runtime
# library for the `aura build --target` targets (ADR-0101):
#
#   eval "$(scripts/cross-target-env.sh)"
#
# The runtime's dependencies hold C sources, so Cargo needs a C compiler
# for each target:
#
# - the musl target of this machine's architecture uses `musl-gcc`
#   (Ubuntu: musl-tools). `musl-gcc` ships no kernel headers, so the
#   compiler also reads the distribution's from a directory of links;
# - the glibc target of the other architecture uses
#   `<arch>-linux-gnu-gcc` (Ubuntu: gcc-aarch64-linux-gnu or
#   gcc-x86-64-linux-gnu).
#
# The musl target of the other architecture has no packaged C compiler.
# Its runtime library is built on a machine of that architecture.
set -euo pipefail

arch="$(uname -m)"
case "$arch" in
  x86_64) other=aarch64 ;;
  aarch64) other=x86_64 ;;
  *)
    echo "cross-target-env: unsupported architecture $arch" >&2
    exit 1
    ;;
esac

if [[ -n "${AURA_KERNEL_HEADERS_DIR:-}" ]]; then
  headers="$AURA_KERNEL_HEADERS_DIR"
elif [[ -n "${XDG_CACHE_HOME:-}" ]]; then
  headers="$XDG_CACHE_HOME/aura/kernel-headers-$arch"
elif [[ -n "${HOME:-}" ]]; then
  headers="$HOME/.cache/aura/kernel-headers-$arch"
else
  headers="$(mktemp -d "${TMPDIR:-/tmp}/aura-kernel-headers-$arch.XXXXXX")"
fi

if [[ "$headers" == *[[:space:][:cntrl:]]* ]]; then
  echo "cross-target-env: header directory path must not contain whitespace or control characters" >&2
  exit 1
fi
if [[ -L "$headers" ]]; then
  echo "cross-target-env: refusing symlink header directory $headers" >&2
  exit 1
fi
if [[ -e "$headers" && ! -d "$headers" ]]; then
  echo "cross-target-env: header path is not a directory: $headers" >&2
  exit 1
fi
mkdir -p "$headers"
chmod 0700 "$headers"
if [[ ! -O "$headers" ]]; then
  echo "cross-target-env: header directory is not owned by the current user: $headers" >&2
  exit 1
fi

for name in linux asm-generic; do
  ln -sfn "/usr/include/$name" "$headers/$name"
done
ln -sfn "/usr/include/$arch-linux-gnu/asm" "$headers/asm"

printf 'export CC_%s_unknown_linux_musl=musl-gcc\n' "$arch"
printf 'export CFLAGS_%s_unknown_linux_musl=%q\n' "$arch" "-idirafter $headers"
printf 'export CC_%s_unknown_linux_gnu=%s-linux-gnu-gcc\n' "$other" "$other"
printf 'export AURA_TEST_CROSS_TARGETS=%s-unknown-linux-musl,%s-unknown-linux-gnu\n' "$arch" "$other"
