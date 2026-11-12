# Freestanding Builds Design Checkpoint

Status: **Step 1 accepted 2026-11-12 as ADR-0094.** Steps 2 and 3 proceed
on the defaults below. The choice marked "owner's call" in section F
stays open to change until release.

A freestanding build compiles Aura code for a machine with no operating
system and no Aura runtime: a kernel, a bootloader stage, firmware, or a
routine linked into a C program. This checkpoint settles what such a
build accepts, what it emits, how the program starts and stops, and how
it reaches memory and hardware.

## Starting Point

- `aura build` links every program against the Aura runtime, a Rust
  static library of about 130,000 lines that needs an operating system:
  threads, files, sockets, an allocator, and unwinding.
- Generated code calls the runtime for every heap value: `str`, `list`,
  `dict`, `set`, tuples, enums, class instances kept in a box, closures,
  and tasks. It also calls the runtime to record call frames and to
  report a trap.
- Code over fixed-width scalars and Copy classes of scalars already
  compiles to machine words with no heap value. The emitter calls such a
  function a frameless leaf. Its only runtime calls are on trap paths.
- The object is always written for the host. LLVM is linked with the
  x86-64 and AArch64 code generators on every host.
- `extern "C"` declarations, C structs, callbacks, and out-parameters
  exist (FFI v1). Aura code cannot define a function under a C symbol,
  and it cannot read or write an address.

## A. What A Freestanding Build Is

- **A1. A flag on `aura build` and `aura check` (default).**
  `aura build --freestanding -o kernel.o app.au` writes one relocatable
  ELF object. `aura check --freestanding` reports what the build would
  reject. The flag applies to the whole program.
- A2. A manifest key, `profile = "freestanding"`. Deferred. A library is
  often useful on both sides, and the flag keeps one package buildable
  both ways. A manifest key can be added later for editor support.

The build links nothing. The output is an object file, and the user's
linker, linker script, and startup code place it. `aura run` and
`aura test` treat the same source as an ordinary hosted program, so
logic that does not touch hardware is tested on the host.

## B. The Language Subset

A freestanding program holds only values that are machine words.

| Allowed | Not available |
| --- | --- |
| `bool`, `int8` to `int64`, `uint8` to `uint64`, `float32`, `float64` | `int128`, `uint128`, `Duration` |
| Copy classes whose fields are allowed types, with methods | classes kept in a box, `str` values, `bytes` |
| `extern "C"` classes | `list`, `dict`, `set`, tuples, `Array` |
| unions of allowed types with `None` that hold no box | enums with payloads, `Result`, `Lookup` |
| functions, generics, `mut` parameters, `while`, `if`, `for` over `range` | closures, function values, trait objects |
| `assert`, contracts, checked arithmetic, casts | `print`, f-strings, `try`, `with`, tasks, queues |
| `extern "C"` calls with scalar, `mut` scalar, struct, literal text, and callback arguments | every builtin module but `mem` |

- **B1. Check the program the exports reach (default).** The check walks
  the functions reachable from the exported functions. A package may
  hold hosted code the freestanding program never calls.
- **B2. Report by construct.** Each rejected value or operation is an
  `AU2052` diagnostic that names the function, the construct, and why a
  freestanding build cannot hold it.
- **B3. A backstop.** After code generation, the build reads the
  object's undefined symbols. Any symbol other than a declared extern,
  `aura_trap`, and the four C memory routines is an internal error, so
  an object can never depend on the runtime by accident.

Float `//`, `%`, and `**`, and integer `**`, need library code. They are
not available in the first version.

## C. Entry Points

- **C1. `export "C" def` (default, owner's call on the spelling).**

  ```aura fragment
  export "C" def kernel_main(magic: uint32, info: uint64) -> None:
      ...
  ```

  The function is defined under its own name as a C symbol with the C
  calling convention. Parameters are scalars or `mut` scalars, which are
  C pointers, and the result is a scalar or `None`. `export` is a word
  only before an ABI string at the start of a declaration. It needs no
  `allow_ffi`: an export gives foreign code a way in and gives Aura code
  no new power. There is no `symbol "name"` clause.
- C2. A body on `extern "C" def`. Rejected: an `extern` block already
  holds a trusted contract, and one word would mean both "defined
  elsewhere" and "defined here".
- C3. A decorator, `@export`. Rejected: decorators are ordinary functions
  that wrap a function, and this one would change linkage.

Aura code calls an exported function like any other. A hosted build
compiles it as an ordinary function and does not export it. A
freestanding program has no `main` wrapper: the exports are its roots.

## D. Traps

A failed check calls one hook and never continues:

```c
void aura_trap(uint32_t code, const uint8_t *file, uint64_t file_len,
               uint32_t line, uint32_t column);
```

- **D1. The program or its environment defines the hook (default).**
  `export "C" def aura_trap(...)` in Aura, or a C or assembly function.
  The object references the symbol only when it holds a check. If the
  hook returns, the next instruction is a trap instruction.
- **D2. Codes.** 1 integer overflow, 2 division by zero, 3 failed
  `assert`, 4 failed contract, 5 cast out of range, 6 shift out of
  range, 7 invalid foreign value, 8 unreachable code.
- **D3. No call trace and no cleanup.** The hook gets the source
  position of the failed check. Frame records, the depth limit of 256
  calls, and trap-time cleanup are runtime features.

`aura build --remove-proved-checks` works as it does for hosted builds.

## E. Targets

`--target` names the machine. The default is the host's architecture.

| Target | Notes |
| --- | --- |
| `x86_64-unknown-none` | System V calls, SSE2, no red zone |
| `aarch64-unknown-none` | AAPCS64, FP and NEON, strict alignment |
| `riscv64gc-unknown-none-elf` | LP64D |

Code is position independent, has no unwind tables and no stack probes,
and keeps no global state, so it is reentrant. 32-bit targets are out of
scope: the emitter holds every value in 64-bit words.

## F. Memory And Hardware

- **F1. A `mem` builtin module (default, owner's call on the gate).**
  Functions over a `uint64` address: `read_uint32(address)` and
  `write_uint32(address, value)`, and the same for `uint8`, `uint16`, and
  `uint64`; `volatile_` forms that the optimizer never removes, merges,
  or reorders; and atomic load, store, swap, add, and compare-exchange
  for `uint32` and `uint64`, with `fence()`. A read or write through a
  bad address has undefined behavior, as in C.
- **F2. The gate is `allow_ffi = true`.** Raw memory is the same grant as
  calling C: the package states that it steps outside Aura's memory
  safety. A separate `unsafe` block syntax is deferred.
- F3. Static mutable variables, inline assembly, and port I/O are not in
  this version. State lives in the entry function's locals, behind `mut`
  parameters, or at fixed addresses.

## G. Support Code

The object is self-contained apart from the trap hook and the program's
own externs. LLVM may call `memcpy`, `memmove`, `memset`, and `memcmp`
for large copies, so the object carries weak definitions of the four. A
C library's definitions replace them at link time.

## H. Proof

- Unit and CLI tests build each target and check the object: machine,
  exported symbols, and no undefined symbol outside the allowed set.
- On Linux the tests link objects with a C harness and no C library
  startup, run them, and compare results with `aura run` of the same
  source.
- A bare-metal example boots under QEMU on each target and prints
  through a UART.
- Size and speed are measured against C and Rust `no_std` builds of the
  same routines.

## Steps

1. Profile, subset check, runtime-free code generation, trap hook,
   exports, three targets (ADR-0094).
2. The `mem` module: raw, volatile, and atomic access (ADR-0095).
3. Bare-metal examples, measurements, book chapter, manual page.
