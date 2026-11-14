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

headers="${AURA_KERNEL_HEADERS_DIR:-${TMPDIR:-/tmp}/aura-kernel-headers-$arch}"
mkdir -p "$headers"
for name in linux asm-generic; do
  ln -sfn "/usr/include/$name" "$headers/$name"
done
ln -sfn "/usr/include/$arch-linux-gnu/asm" "$headers/asm"

echo "export CC_${arch}_unknown_linux_musl=musl-gcc"
echo "export CFLAGS_${arch}_unknown_linux_musl='-idirafter $headers'"
echo "export CC_${other}_unknown_linux_gnu=${other}-linux-gnu-gcc"
echo "export AURA_TEST_CROSS_TARGETS=${arch}-unknown-linux-musl,${other}-unknown-linux-gnu"
