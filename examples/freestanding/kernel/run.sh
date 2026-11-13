#!/usr/bin/env bash
# Builds the kernel for one board and boots it under QEMU.
# Usage: examples/freestanding/kernel/run.sh riscv64|aarch64 [path to aura]
# Needs clang, ld.lld, and qemu-system-riscv64 or qemu-system-aarch64.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
board="${1:?usage: run.sh riscv64|aarch64 [aura]}"
aura="${2:-aura}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

case "$board" in
  riscv64)
    target=riscv64gc-unknown-none-elf
    clang_target=(--target=riscv64-unknown-elf -march=rv64gc -mabi=lp64d)
    qemu=(qemu-system-riscv64 -machine virt -bios none)
    ;;
  aarch64)
    target=aarch64-unknown-none
    clang_target=(--target=aarch64-none-elf)
    qemu=(qemu-system-aarch64 -machine virt -cpu cortex-a57)
    ;;
  *)
    echo "unknown board: $board" >&2
    exit 2
    ;;
esac

"$aura" build --freestanding --target "$target" -o "$out/kernel.o" "$here/src/main.au"
clang "${clang_target[@]}" -c "$here/boards/$board.S" -o "$out/start.o"
ld.lld -T "$here/boards/$board.ld" --gc-sections "$out/start.o" "$out/kernel.o" -o "$out/kernel.elf"
timeout 20 "${qemu[@]}" -nographic -kernel "$out/kernel.elf"
