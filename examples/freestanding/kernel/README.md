# A kernel with no operating system

`src/main.au` is a freestanding Aura program. It prints over a serial
port, takes a spin lock, and reports a failed check, on a machine with no
operating system and no Aura runtime.

| File | Purpose |
| --- | --- |
| `src/main.au` | The program: `kernel_main`, the `aura_trap` hook, serial output through the `mem` module |
| `boards/riscv64.S`, `boards/riscv64.ld` | Startup code and memory layout for QEMU's `virt` RISC-V machine |
| `boards/aarch64.S`, `boards/aarch64.ld` | The same for QEMU's `virt` AArch64 machine |
| `run.sh` | Builds for one board and boots it |

```sh
examples/freestanding/kernel/run.sh riscv64 target/release/aura
examples/freestanding/kernel/run.sh aarch64 target/release/aura
```

The script needs `clang`, `ld.lld`, and `qemu-system-riscv64` or
`qemu-system-aarch64`. Both boards print:

```text
p=168
c=111
r=1000
n=100
trap 1 main.au:154:13
```

- `p` is the number of primes below 1000 and `c` the Collatz steps from 27.
- `r` is a float result: a point turned 1000 times stays on the unit circle.
- `n` is a counter raised 100 times under a lock made of
  `mem.atomic_compare_exchange_uint32`.
- The last line is the `aura_trap` hook reporting a `uint8` overflow the
  program makes on purpose: code 1, then the file, line, and column.

The startup code does three things the Aura program cannot: it sets the
stack pointer, turns the float registers on, and powers the machine off.

There is no x86-64 board. A PC needs a long-mode boot stub and port I/O
instructions, both assembly. `aura build --freestanding` targets x86-64
all the same, for code linked into a kernel or a C program.
