# Freestanding builds

Date: 2026-11-12. Asked for by John: the freestanding roadmap item, bug
free and complete, with size and speed measured.

## Work completed (step 1, ADR-0094)

- `aura build --freestanding [--target name] -o out.o file.au` writes one
  relocatable ELF object for `x86_64-unknown-none`,
  `aarch64-unknown-none`, or `riscv64gc-unknown-none-elf`. It links
  nothing. `--remove-proved-checks` works with it.
- `aura check --freestanding` reports each construct outside the subset as
  `AU2052`, with the function and line. Only functions the exports reach
  are checked.
- `export "C" def` defines a C symbol. It is a contextual word, needs no
  manifest permission, and is an ordinary function in a hosted build.
- A failed check calls `aura_trap(code, file, file_length, line, column)`
  and then traps.
- The emitter compiles every function of a freestanding module as a
  frameless leaf and gives exports and callbacks a direct C entry.
- The object carries weak memory routines, and a backstop refuses any
  other undefined symbol.
- The formatter, the recovery symbol scan in the language server, and the
  VS Code grammar know the declaration.

## Evidence

- Unit tests in `native_codegen_freestanding_tests.rs` build every target
  and read the object's symbols.
- `crates/aura/tests/freestanding.rs` links an object with a C harness on
  Linux and compares its output with `aura run` of the same source, then
  checks the trap code and line for division by zero, overflow, a cast,
  and a contract.
- The same program gave the same output on x86-64 and under
  `qemu-system-aarch64` and `qemu-system-riscv64` with no operating
  system, including a trap with its position.

## Measurements

`benchmarks/freestanding/run.sh`, x86-64 Linux, best of nine runs:

| Routine | Aura | C (`clang -O2`) | Rust | Rust with overflow checks |
| --- | --- | --- | --- | --- |
| `crc32` | 50 ms | 195 ms | 50 ms | 54 ms |
| `collatz` | 231 ms | 157 ms | 139 ms | 302 ms |
| `isqrt_sum` | 98 ms | 105 ms | 145 ms | 164 ms |
| `gcd_sum` | 201 ms | 131 ms | 156 ms | 175 ms |
| `orbit` | 832 ms | 882 ms | 778 ms | 756 ms |

The five functions are 993 bytes of x86-64 code against 914 for C. Whole
objects: 3,218 bytes of code and read-only data on x86-64, 3,072 on
AArch64, and 2,866 on RISC-V. About 2,000 of those bytes are a table LLVM
22 builds for the `crc32` loop, which is also why Aura and Rust run it
four times as fast as clang 18.

## Gaps that stay

- No heap values, so no `str`, collections, enums, or closures.
- The x86-64 target uses SSE2 registers for floats. Startup code must
  enable SSE, and an interrupt handler must save those registers if it
  calls Aura code that uses floats.
- A C struct by value is not supported on RISC-V.
- `gcd_sum` is 1.5 times C because `%` rounds toward negative infinity
  and keeps its overflow check.

## Next

Step 2 is the `mem` module (ADR-0095). Step 3 is bare-metal examples
under QEMU and a book chapter.
