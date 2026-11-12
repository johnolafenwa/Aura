# ADR-0094: A freestanding build writes one object that needs no runtime

- Status: Accepted
- Date: 2026-11-12
- Builds on: ADR-0074 (FFI v1), ADR-0077 (C structs), ADR-0080 (callbacks)
- Design checkpoint: [Freestanding Builds](../28-freestanding-design-checkpoint.md)

## Context

`aura build` links every program against the Aura runtime, a Rust static
library that needs an operating system: threads, files, an allocator, and
unwinding. Generated code calls it for every heap value, to record call
frames, and to report a failed check. Aura code could not be linked into a
kernel, a boot stage, or firmware, and could not define a function under a
C symbol.

Code over fixed-width numbers and Copy classes already compiled to machine
words. Its only runtime calls were frame records and the failure path.

## Decision

- **`aura build --freestanding -o out.o file.au` writes one relocatable
  ELF object and links nothing.** `aura check --freestanding` reports what
  the build would refuse. The flag applies to the whole program. `aura run`
  and `aura test` treat the same source as a hosted program.
- **`export "C" def name(...)` defines a function under a C symbol.** Its
  parameters are scalars or `mut` scalars, which are C pointers, and its
  result is a scalar or `None`. `export` is a word only before a string at
  the start of a module-level declaration. The declaration needs no
  manifest permission: it gives foreign code a way in and gives Aura code
  no new power. A hosted build compiles it as an ordinary function.
- **The exports are the roots.** A freestanding program has no `main`
  wrapper. The build compiles the functions the exports reach and checks
  only those.
- **The subset is values held in machine words.** Scalars up to 64 bits,
  classes of those, `extern "C"` classes, unions that need no box,
  functions, generics, static trait dispatch, `mut` parameters, loops,
  `assert`, contracts, casts, and foreign calls with scalar, `mut` scalar,
  struct, literal text, and callback arguments. Every other construct is
  an `AU2052` diagnostic that names the function and the construct.
- **A failed check calls `aura_trap(code, file, file_length, line,
  column)` and then executes a trap instruction.** The program or its
  environment defines the hook. Codes: 1 overflow, 2 division by zero,
  3 `assert`, 4 contract, 5 cast, 6 shift count, 7 invalid foreign value,
  8 unreachable. There is no call trace, no message text, and no cleanup.
- **Three targets**: `x86_64-unknown-none`, `aarch64-unknown-none`, and
  `riscv64gc-unknown-none-elf`, chosen with `--target`, defaulting to the
  compiler's own architecture. Code is position independent with no
  unwind tables, no stack probes, and no global state.
- **The object carries weak `memcpy`, `memmove`, `memset`, and `memcmp`.**
  LLVM may call them for a large copy. A C library's definitions replace
  them, and a program that declares one supplies its own.
- **A backstop reads the finished object.** Any undefined symbol other
  than `aura_trap`, the four memory routines, and the program's own
  `extern` and `export` names fails the build as a compiler fault.

## How the emitter changes

In a freestanding module every function is compiled as a frameless leaf:
no frame record, no frame descriptor, no write-through sinks, no thunks,
no leaf variants. Exports and callbacks get a direct C entry. Every site
that called a runtime failure helper calls the hook instead.

## Alternatives rejected

- **A manifest profile, `profile = "freestanding"`.** A library is often
  useful on both sides, and a flag keeps one package buildable both ways.
- **A body on `extern "C" def`.** One word would mean both "defined
  elsewhere" and "defined here", and an `extern` block already holds a
  trusted contract.
- **A decorator, `@export`.** A decorator is an ordinary function that
  wraps a function. This one would change linkage.
- **A `symbol "name"` clause on an export.** The function's name is the
  symbol. No program so far needs another.
- **Gating `export` behind `allow_ffi`.** The permission marks a package
  that steps outside Aura's checks. An export does not.
- **A small freestanding runtime with an allocator.** It would bring
  `str` and collections, at the cost of a second runtime to keep correct.
  Programs in the subset need none of it. It can follow as its own step.
- **Compiling the x86-64 target without SSE.** A kernel that never saves
  vector registers wants that, and it needs a soft-float library. Deferred.

## Consequences

- A freestanding object for five integer and float routines is about the
  size of the same C, and runs within 1.5 times C and Rust
  (`benchmarks/freestanding`).
- The emitter has a second mode. Each new runtime helper call needs a
  freestanding answer: the hook, inline code, or an `AU2052` report. The
  backstop turns a missed one into a build failure.
- RISC-V is a new LLVM code generator in every `aura` binary.
- Raw memory, volatile, and atomic access are ADR-0095.
