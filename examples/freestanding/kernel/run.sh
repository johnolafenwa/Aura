#!/usr/bin/env bash
# Builds the kernel for one board and boots it under QEMU.
# Usage: examples/freestanding/kernel/run.sh x86_64|riscv64|aarch64 [path to aura]
# Needs clang, ld.lld, timeout, and the selected board's qemu-system binary.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
board="${1:?usage: run.sh x86_64|riscv64|aarch64 [aura]}"
aura="${2:-aura}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

case "$board" in
  x86_64)
    "$aura" build --freestanding --target x86_64-unknown-none --kernel -o "$out/kernel.o" "$here/src/x86_64.au"
    clang --target=x86_64-none-elf -c "$here/boards/x86_64.S" -o "$out/start.o"
    clang --target=x86_64-none-elf -O2 -ffreestanding -fno-pic -mcmodel=kernel -mno-red-zone -mgeneral-regs-only \
      -c "$here/boards/x86_64.c" -o "$out/board.o"
    ld.lld --oformat=binary -T "$here/boards/x86_64.ld" --gc-sections \
      "$out/start.o" "$out/kernel.o" "$out/board.o" -o "$out/kernel.bin"
    status=0
    timeout 20 qemu-system-x86_64 -machine pc -display none -monitor none -serial stdio \
      -no-reboot -device isa-debug-exit,iobase=0xf4,iosize=4 -kernel "$out/kernel.bin" || status=$?
    # isa-debug-exit encodes the guest's 0x10 success as (0x10 << 1) | 1.
    if [[ "$status" != 33 ]]; then
      echo "x86_64: expected QEMU status 33, got $status" >&2
      exit 1
    fi
    exit 0
    ;;
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
timeout 20 "${qemu[@]}" -nic none -nographic -kernel "$out/kernel.elf"
