# A kernel with no operating system

`src/main.au` is a freestanding Aura program. It prints over a serial
port, takes a spin lock, keeps a growing block on a heap, and reports a
failed check, on a machine with no
operating system and no Aura runtime.

| File | Purpose |
| --- | --- |
| `src/main.au` | The program: `kernel_main`, the `aura_trap` hook, serial output and a heap through the `mem` module |
| `boards/riscv64.S`, `boards/riscv64.ld` | Startup code and memory layout for QEMU's `virt` RISC-V machine |
| `boards/aarch64.S`, `boards/aarch64.ld` | The same for QEMU's `virt` AArch64 machine |
| `src/x86_64.au` | Interrupt table, breakpoint and timer handlers, serial driver and QEMU exit |
| `boards/x86_64.S`, `boards/x86_64.ld` | Multiboot entry, long-mode transition, page tables and PC memory layout |
| `boards/x86_64.c` | Fixed-address storage and the C ABI bridge for printing Aura string literals |
| `run.sh` | Builds for one board and boots it |
| `test.sh` | Boots all boards and compares serial output and shutdown status |

```sh
examples/freestanding/kernel/run.sh riscv64 target/release/aura
examples/freestanding/kernel/run.sh aarch64 target/release/aura
examples/freestanding/kernel/run.sh x86_64 target/release/aura
```

The script needs `clang`, `ld.lld`, GNU `timeout` and the QEMU system
emulator for the selected architecture. On the RISC-V and AArch64
`virt` boards, expect this output.

```text
p=168
c=111
r=1000
n=100
s=42
h=76127
f=0
trap 1 main.au:234:13
```

- `p` is the number of primes below 1000 and `c` the Collatz steps from 27.
- `r` is a float result: a point turned 1000 times stays on the unit circle.
- `n` is a counter raised 100 times under a lock made of
  `mem.atomic_compare_exchange_uint32`.
- `s` reads back the second sample after a typed pointer writes it in module storage.
- `h` is the sum of the primes below 1000. They are kept in a block from
  `mem.alloc` that `mem.realloc` doubles as it fills. `f` is the number of
  bytes missing from the heap after `mem.free`.
- The last line is the `aura_trap` hook reporting a `uint8` overflow the
  program makes on purpose: code 1, then the file, line, and column.

The compiler selects the QEMU serial register from the target architecture
and computes the decimal lookup table used by the serial output routine.
The startup code names the megabyte after the stack as the heap's region,
which the program hands to `mem.heap_add_region`.

On the virt boards, startup sets the stack pointer, enables floating-point
registers and supplies the board's power-off routine.

## PC Interrupts

The x86-64 board boots a Multiboot image at 1 MiB. Startup switches to
64-bit mode and calls Aura with an aligned stack and interrupts disabled.
The compiler's `--kernel` option keeps generated code off the vector
registers and reserves stack space above the interrupt frame.

```text
breakpoint
timer
ok
```

Aura writes the two interrupt gates into a zeroed table, loads it with
`lidt`, and triggers vector 3 with `int3`. After the breakpoint handler
returns, Aura programs the PC timer and unmasks its IRQ. The timer handler
masks the line, acknowledges the interrupt controller and records the
event. Each handler writes its name to COM1 through port I/O. The final
`ok` comes from the interrupted code after both returns.

The guest writes 0x10 to QEMU's debug-exit port at 0xf4. QEMU returns
status 33. A trap writes 0x11, giving status 35. The runner requires 33
and fails on timeout or a reset. To run the same serial and exit checks as CI:

```sh
examples/freestanding/kernel/test.sh target/release/aura
```
