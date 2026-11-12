# Freestanding Builds

A freestanding build compiles Aura code for a machine with no operating
system and no Aura runtime: a kernel, a boot stage, firmware, or a routine
linked into a C program. The build writes one relocatable object file and
links nothing. Your linker, linker script, and startup code place it.

```sh
aura build --freestanding -o kernel.o kernel.au
aura build --freestanding --target riscv64gc-unknown-none-elf -o kernel.o kernel.au
aura check --freestanding kernel.au
```

`aura run`, `aura test`, and an ordinary `aura build` treat the same source
as a hosted program, so logic that does not touch hardware is tested on the
host.

## Exported Functions

A freestanding program has no `main`. Its entry points are the functions
declared with `export "C" def`:

```aura
export "C" def checksum(seed: uint32, count: uint32) -> uint32:
    mut total = seed
    mut i: uint32 = 0
    while i < count:
        total = (total * 31 + i) % 65521
        i = i + 1
    return total

export "C" def divide(a: int64, b: int64, remainder: mut int64) -> int64:
    remainder = a % b
    return a // b
```

Each one is defined in the object under its own name, with the C calling
convention of the target:

```c
uint32_t checksum(uint32_t seed, uint32_t count);
int64_t divide(int64_t a, int64_t b, int64_t *remainder);
```

| Rule | Detail |
| --- | --- |
| Parameters | `bool`, `int8` to `int64`, `uint8` to `uint64`, `float32`, `float64`. A `mut` parameter of one of those types is a C pointer to it. |
| Result | One of the same types, or `None` for `void`. |
| Shape | A module-level function with a body. No type parameters, no receiver, no default values, no decorators. |
| Name | The symbol is the function's name. Two exports with one name in a program are an error. |
| Visibility | `public export "C" def` lets other modules import it. |
| Calls from Aura | An exported function is called like any other function. |

`export` is a word only before the string `"C"` at the start of a
declaration. Anywhere else it is an ordinary name. The declaration needs no
permission in `Aura.toml`. In a hosted build it is an ordinary function and
the binary does not export it.

A freestanding build needs at least one exported function. It compiles the
functions the exports reach, in every module of the program, and nothing
else. An export in an imported module is also defined in the object.

## What A Freestanding Program May Hold

Every value is one or more machine words. Nothing lives on a heap.

| Available | Not available |
| --- | --- |
| `bool`, `int8` to `int64`, `uint8` to `uint64`, `float32`, `float64` | `int128`, `uint128`, `Duration` |
| Classes whose fields are available types, with methods | Classes that hold a `str`, a collection, or an `indirect` field |
| `extern "C"` classes | `str`, `bytes`, `list`, `dict`, `set`, tuples, `Array` |
| `T \| None` and other unions of available types | Enums, `Result`, `Lookup` |
| Functions, generics, traits with static dispatch, `mut` parameters | Closures, function values, trait objects |
| `if`, `while`, `for` over `range` or over a call of a generator function, `match` on a union | `try`, `with`, tasks, queues |
| `assert`, `requires`, `ensures`, checked arithmetic, `as` casts | `print`, f-strings, text building |
| `abs`, `min`, `max`, `sqrt` | Every builtin module |
| `+ - * / // %` on integers, `+ - * /` on floats, shifts, bit operators, comparisons | `**`, float `//` and `%`, `round` |
| Module constants with a literal value | Module constants computed when the program starts |
| `extern "C"` calls with scalar, `mut` scalar, struct, literal text, and callback arguments | Opaque handles, `bytes` and `ptr[T]` arguments. Pass an address as a `uint64`. |

`aura check --freestanding` and the build report each construct outside the
subset as [`AU2052`](/manual/diagnostics), with the function that holds it:

```text
error[AU2052]: `entry` calls `print`, which a freestanding build cannot compile
 --> kernel.au:3:5
```

Only reached functions are checked. A package may hold hosted helpers and
tests that a freestanding build never compiles.

## Failed Checks

Integer overflow, division by zero, a failed `assert`, a failed contract,
and a cast out of range are checked exactly as in a hosted build. A failed
check calls one function, which the program or its environment defines:

```c
void aura_trap(uint32_t code, const char *file, size_t file_length,
               uint32_t line, uint32_t column);
```

| Code | Check |
| --- | --- |
| 1 | Integer overflow, including `abs` of the lowest value |
| 2 | Division or remainder by zero |
| 3 | Failed `assert` |
| 4 | Failed `requires`, `ensures`, or `invariant` |
| 5 | `as` cast out of the target type's range |
| 6 | Shift count out of range |
| 7 | A foreign `bool` that is neither 0 nor 1 |
| 8 | Unreachable code was reached |

- `file` is the source file's path relative to the directory of the file
  that was built. It is not NUL-terminated. `file_length` is its length in
  bytes.
- `line` and `column` count from 1 and name the failed check.
- The hook must not return. A trap instruction follows the call.
- There is no call trace and no message text. An `assert` message is not
  kept in the object.
- The object names `aura_trap` only when it holds a check.

The hook can be written in Aura:

```aura
export "C" def aura_trap(code: uint32, file: uint64, file_length: uint64, line: uint32, column: uint32) -> None:
    while true:
        pass

export "C" def halve(value: int64) -> int64:
    requires value % 2 == 0
    return value // 2
```

`aura build --freestanding --remove-proved-checks` removes the checks that
`aura verify` proves can never fail, as in a hosted build. See
[Verification](/manual/verification#trust).

## Targets

`--target` names the machine. Without it the build targets the
architecture of the machine that runs the compiler.

| `--target` | Calling convention | Instruction set |
| --- | --- | --- |
| `x86_64-unknown-none` | System V | x86-64 baseline with SSE2 |
| `aarch64-unknown-none` | AAPCS64 | ARMv8-A with FP and NEON, aligned access only |
| `riscv64gc-unknown-none-elf` | LP64D | RV64GC |

Every target writes an ELF object, on every host.

- Code is position independent and uses no thread-local or global state,
  so every function is reentrant.
- There are no unwind tables and no stack probes.
- Floats use the hardware float registers. Startup code must enable them
  before the first Aura function runs: SSE on x86-64, FP and NEON on
  AArch64, and the F and D extensions on RISC-V.
- A C struct passed or returned by value is not supported on RISC-V. Pass a
  `mut` parameter.
- `--debug` and `--target-cpu` are refused.

## What The Object Needs

The object refers to these symbols and no others:

| Symbol | When |
| --- | --- |
| `aura_trap` | The object holds a check. |
| The program's `extern "C"` functions | The program calls them. |

The compiler may copy or compare a large value through `memcpy`, `memmove`,
`memset`, or `memcmp`. The object carries a small weak definition of each,
so it links with no C library. A C library's definitions take their place.
A program that exports one of the four names supplies its own.

The build reads the finished object and fails if it needs any other symbol,
so an object cannot depend on the Aura runtime by accident.

## Linking

With a C program on Linux:

```sh
aura build --freestanding -o routines.o routines.au
cc main.c routines.o -o program
```

For bare metal, link with your startup object and linker script:

```sh
aura build --freestanding --target aarch64-unknown-none -o kernel.o kernel.au
ld.lld -T board.ld start.o kernel.o -o kernel.elf
```

Each function is in its own section, so `--gc-sections` drops what the
image does not use.

## Size And Speed

`benchmarks/freestanding` builds five routines with Aura, C, and Rust
without its standard library, and links each with one harness. On an
x86-64 Linux machine:

| Routine | Aura | C (`clang -O2`) | Rust | Rust with overflow checks |
| --- | --- | --- | --- | --- |
| `crc32` | 50 ms | 195 ms | 50 ms | 54 ms |
| `collatz` | 231 ms | 157 ms | 139 ms | 302 ms |
| `isqrt_sum` | 98 ms | 105 ms | 145 ms | 164 ms |
| `gcd_sum` | 201 ms | 131 ms | 156 ms | 175 ms |
| `orbit` | 832 ms | 882 ms | 778 ms | 756 ms |

Aura keeps its overflow and divisor checks, and `%` rounds toward negative
infinity, which costs a few instructions over C's remainder. The five
functions are 993 bytes of code against 914 for C. The Aura object also
holds 115 bytes of memory routines and the file name for the trap hook.

## Current Limits

- No heap values. A freestanding program cannot allocate.
- No reads or writes of raw addresses, no volatile or atomic access, and no
  inline assembly. Hardware access goes through `extern "C"` functions.
- No mutable module-level state. Keep state in the entry function's locals,
  behind `mut` parameters, or in C.
- 32-bit targets are not supported.
