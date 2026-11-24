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

An exported function is a root for code generation. The compiler follows
calls from each root across module boundaries. A retained function, a
placed data definition or a module assembly block can also provide the
object's contents. An export in an imported module is defined in the
object.

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
| `abs`, `min`, `max`, `sqrt`, and the [`mem` module](/manual/mem) | Every other builtin module |
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

## Hardware Access

The [`mem` module](/manual/mem) reads and writes integers at a raw address,
with volatile forms for device registers and atomic forms for cells shared
between cores. It needs `allow_ffi = true` in `Aura.toml`.

```aura fragment
import mem

UART: uint64 = 268435456

export "C" def put(byte: uint8) -> None:
    mem.volatile_write_uint8(UART, byte)
```

## Assembly

Write `asm` as a statement in a package with `allow_ffi = true`. This
x86-64 function takes the bit pattern in an integer register, swaps its
bytes and returns the result:

```aura fragment
export "C" def swap_bytes(value: uint64) -> uint64:
    mut swapped: uint64 = 0
    asm("movq $1, $0\nbswapq $0", output("reg", swapped), input("reg", value), options("pure", "preserves_flags"))
    return swapped
```

| Clause | Contract |
| --- | --- |
| `input("reg", expr)` | Evaluate an integer or boolean expression once into a general-purpose register. |
| `output("reg", binding)` | Store into a distinct mutable scalar binding. Its Aura type sets the width. |
| `input("fpreg", expr)`, `output("fpreg", binding)` | Bind a `float32` or `float64` to a floating-point register. These registers are unavailable under `--kernel`. |
| `clobber("rax", ...)` | List additional registers the template overwrites. A fixed register mentioned in the text needs an operand or clobber whenever its value matters to generated code. |
| `options("pure")` | At least one output. The result depends only on the inputs. The template observes and changes no memory and has no other observable effects. |
| `options("preserves_flags")` | The template preserves condition codes. |
| `options("noreturn")` | Control never reaches the next statement. The template has no outputs. |

Number the outputs first and the inputs second, starting at `$0`. Use
`$$` for a literal dollar in inline assembly. On x86-64 use AT&T syntax.
A register name in place of a class pins the operand, as in
`input("dx", port)` for a `uint16`. A boolean input holds zero or one.
A boolean output is true if its low byte is nonzero. For explicit register
spellings use braced LLVM modifiers, such as `${0:k}` for an x86 32-bit
general register and `${0:w}` or `${0:s}` for AArch64 integer or floating
single-word registers. By default assembly can change memory and flags.

Use `module_asm` at module scope for a complete routine. The linker sees
the symbols you define in the template. Each imported module contributes
its text once. The formatter preserves spaces and line breaks inside a
template.

```aura fragment
module_asm(""".text
.globl pause_once
pause_once:
    pause
    ret""")
```

The checker validates the operand types, register names and declared
clobbers for the selected target. LLVM checks instruction text while
writing the object. See [Verification](/manual/verification#trust) for
the proof boundary.

## Machine Attributes

Declare a hardware interrupt entry with `@machine.interrupt`. This handler
receives an x86 exception's hardware error code. Startup supplies its
address to the appropriate interrupt gate.

```aura fragment
@machine.interrupt("x86_64_error")
@machine.section(".text.fault")
@machine.align(16)
@machine.used
export "C" def fault(code: uint64) -> None:
    pass
```

| Convention | Aura parameters | Entry and return |
| --- | --- | --- |
| `"x86_64"` | None | Preserves interrupted registers and returns with `iretq`. |
| `"x86_64_error"` | One `uint64` | Passes the hardware error word to the handler and discards that word before `iretq`. |
| `"aarch64"` | None | Uses an aligned EL1 stack with FP/NEON enabled. Preserves the exception return state and returns with `eret`. |
| `"riscv64"` | None | Uses an aligned machine-mode stack. Preserves the exception PC, status and live floating-point state and returns with `mret`. |

Each handler returns `None` and can call ordinary Aura or foreign
functions. Only a hardware exception may enter it. Ordinary code that
needs the same operation can call a helper shared with the handler.

| Attribute | Effect |
| --- | --- |
| `@machine.naked` | The body is exactly one operand-free `asm("...")`. The template owns the stack, registers and return instruction. |
| `@machine.section("name")` | Place the definition in a named ELF section. |
| `@machine.align(bytes)` | Align the definition to a power of two in bytes, up to 2²⁹. |
| `@machine.link_name("symbol")` | Write a function under the given object symbol. Aura callers use its source name. |
| `@machine.used` | Keep a function and its section when linking with `--gc-sections`. |
| `@machine.noinline`, `@machine.always_inline` | Set the function's inlining policy. Choose one. |

Section and alignment also apply to an explicitly typed scalar module
constant with a literal initializer. For example,
`@machine.section(".board.config")` and `@machine.align(64)` above
`MAGIC: uint32 = 0x12345678` emit a four-byte value on a 64-byte boundary.

## Targets

`--target` names the machine. Without it the build targets the
architecture of the machine that runs the compiler.

| `--target` | Calling convention | Instruction set |
| --- | --- | --- |
| `x86_64-unknown-none` | System V | x86-64 baseline with SSE2 |
| `aarch64-unknown-none` | AAPCS64 | ARMv8-A with FP and NEON, aligned access only |
| `riscv64gc-unknown-none-elf` | LP64D | RV64GC |

Every target writes an ELF object, on every host.

- The default build emits position-independent code.
- There are no unwind tables and no stack probes.
- Floats use the hardware float registers. Startup code must enable them
  before the first Aura function runs: SSE on x86-64, FP and NEON on
  AArch64, and the F and D extensions on RISC-V.
- A C struct passed or returned by value is not supported on RISC-V. Pass a
  `mut` parameter.
- `--debug` and `--target-cpu` are refused.

## x86-64 Kernel Code

Build every Aura object that can run on an interruptible kernel stack with
`--kernel`. Pass the same profile to `aura check`.

```sh
aura check --freestanding --target x86_64-unknown-none --kernel kernel.au
aura build --freestanding --target x86_64-unknown-none --kernel -o kernel.o kernel.au
```

| x86-64 setting | Ordinary freestanding build | With `--kernel` |
| --- | --- | --- |
| Code placement | Position independent, default code model | Static relocations, kernel code model for addresses in the high 2 GiB |
| Floating point | SSE2, System V float ABI | Software floating point, integer-register float ABI |
| Stack | A leaf can use the 128 bytes below the stack pointer, the red zone | Each function reserves the stack space it needs |

Kernel mode disables MMX, SSE, SSE2 and x87 in compiler-generated code.
Use the same register and stack conventions for foreign code and assembly.
Floating-point parameters and results cross a call as IEEE bit patterns
in general-purpose registers. Arithmetic may call compiler-runtime helpers
such as `__adddf3`. A square root can call `sqrtf` or `sqrt`. Supply those
routines with the kernel's soft-float ABI. Run `llvm-nm -u kernel.o` to
see which symbols one object needs.

## What The Object Needs

The object refers to these symbols and no others:

| Symbol | When |
| --- | --- |
| `aura_trap` | The object holds a check. |
| The program's `extern "C"` functions | The program calls them. |
| Soft-float helpers | An x86-64 kernel object needs software arithmetic or square root. |

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
- No mutable module-level state. Keep state in the entry function's locals,
  behind `mut` parameters, or in C.
- 32-bit targets are not supported.
