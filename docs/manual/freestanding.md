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

Every value is one or more machine words. Two kinds of value also own
blocks on a heap: text, and an enum that holds itself. See
[Text](#text) and [Enums That Hold Themselves](#enums-that-hold-themselves).
A program reaches other heap memory through addresses: see
[The Heap](#the-heap).

A tuple or an enum whose parts are all available types is available too.
The compiler keeps it in machine words, as it keeps a class: a tuple is its
elements, and an enum is the variant's number and room for each variant's
payloads.

```aura
enum Reading:
    Value(int64)
    Missing

def read(port: uint64) -> (Reading, uint64):
    if port == 0:
        return (Reading.Missing, port)
    return (Reading.Value(42), port + 1)
```

| Available | Not available |
| --- | --- |
| `bool`, `int8` to `int64`, `uint8` to `uint64`, `float32`, `float64` | `int128`, `uint128`, `Duration` |
| Classes whose fields are available types, with methods | Classes that hold a collection |
| `str` with literals, `+`, comparisons, `len`, `byte_len`, `starts_with`, `ends_with`, `contains` | Every other `str` method, indexing, slices, f-strings, `str \| None` |
| `extern "C"` classes | `bytes`, `list`, `dict`, `set`, `Array` |
| Tuples of available types | Tuples that hold any other type |
| Enums and `Result` whose payloads are scalars, classes, tuples, or enums of available types, and `indirect` payloads of the enum's own type | Enums that hold a union or a collection, and `Lookup` |
| `T \| None` and other unions of available types | Unions that hold any other type |
| Functions, generics, traits with static dispatch, `mut` parameters | Closures, function values, trait objects |
| `if`, `while`, `for` over `range` or over a call of a generator function, `match` on a union or an enum, `try` on a `Result` of available types | `with`, tasks, queues |
| `assert`, `requires`, `ensures`, checked arithmetic, `as` casts | `print`, f-strings, text building |
| `abs`, `min`, `max`, `sqrt`, and the [`mem` module](/manual/mem) | Every other builtin module |
| `+ - * / // %` on integers, `+ - * /` on floats, shifts, bit operators, comparisons | `**`, float `//` and `%`, `round` |
| Module constants with a literal value | Module constants computed when the program starts |
| `extern "C"` calls with scalar, `mut` scalar, struct, text, and callback arguments | Opaque handles, `bytes` and `ptr[T]` arguments. Pass an address as a `uint64`. |

`aura check --freestanding` and the build report each construct outside the
subset as [`AU2052`](/manual/diagnostics), with the function that holds it:

```text
error[AU2052]: `entry` calls `print`, which a freestanding build cannot compile
 --> kernel.au:3:5
```

Only reached functions are checked. A package may hold hosted helpers and
tests that a freestanding build never compiles.

## Text

A freestanding program holds `str` values: in locals, parameters, results,
classes, tuples, and enums.

```aura
def greeting(name: str, tries: int64) -> str:
    mut text = "hello " + name
    if tries > 1:
        text = text + " again"
    return text

export "C" def greeting_length(tries: int64) -> int64:
    text = greeting("board", tries)
    if text.ends_with("again"):
        return text.len() + 1
    return text.len()
```

A `str` lives in a block from the allocation hooks: a reference count, the
length in bytes, the bytes, and a zero byte after them. The compiler
counts the references and frees the block with its last one. The
operations are written in Aura and compiled into the object only when the
program holds a `str`.

Available: literals, `+`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `len`,
`byte_len`, `starts_with`, `ends_with`, and `contains`. A literal used in
an operation is read where it lies in the program's data and allocates
nothing.

A foreign function takes a `str` value as it takes a literal: a `cstr`
parameter gets the address of the bytes, which end in a zero byte, and a
`str` parameter gets the address and the length.

```aura fragment
extern "C" def uart_write(text: str) -> None

export "C" def report(tries: int64) -> None:
    uart_write(greeting("board", tries))
```

```c
void uart_write(const char *text, size_t length);
```

The program needs a heap, as in the next section.

## Enums That Hold Themselves

An enum may hold itself through an `indirect` payload, which is how a
program writes a list or a tree:

```aura
enum Chain:
    End
    Link(int64, indirect Chain)

def sum_of(chain: Chain) -> int64:
    match chain:
        case Chain.End:
            return 0
        case Chain.Link(value, rest):
            return value + sum_of(rest)

export "C" def sum_to(n: int64) -> int64:
    mut chain = Chain.End
    for i in range(0, n):
        chain = Chain.Link(i, chain)
    return sum_of(chain)
```

Each `indirect` payload lives in a block from the allocation hooks: a
reference count, then the payload's words. The enum value itself stays in
machine words and holds the block's address. The compiler counts the
references, and the program frees a block when its last reference goes, so
`sum_to` returns with every block freed.

A class, a tuple, or a `Result` may hold such an enum. `match own` moves
its payloads out, and `try` passes one on in an error.

The program needs a heap: name a region with `mem.heap_add_region` first,
or define the hooks. See [The Heap](#the-heap). A request the heap turns
down is a failed assertion, trap code 3.

A program with no such enum allocates nothing and needs no hook.

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

## The Heap

`mem.alloc`, `mem.free`, and `mem.realloc` give a freestanding program
blocks of memory. The program first names the memory the heap may use:

```aura fragment
import mem

export "C" def kernel_main(heap_start: uint64, heap_size: uint64) -> None:
    mem.heap_add_region(heap_start, heap_size)
    table = mem.alloc(4096, 16)
    if table == 0:
        return
    mem.write_uint64(table, 1)
    mem.free(table, 4096, 16)
```

[Memory Module](/manual/mem#allocation) has the rules of the three
functions. They call three C symbols, the allocation hooks:

```c
void *aura_alloc(size_t size, size_t align);
void aura_free(void *address, size_t size, size_t align);
void *aura_realloc(void *address, size_t old_size, size_t align, size_t new_size);
```

A program that calls one of the three functions and declares no hook gets
Aura's allocator. The build compiles it into the object, which then
defines these symbols:

| Symbol | C signature | Purpose |
| --- | --- | --- |
| `aura_alloc`, `aura_free`, `aura_realloc` | Above | The hooks. |
| `aura_heap_add_region` | `void aura_heap_add_region(uint64_t start, uint64_t size)` | What `mem.heap_add_region` calls. Startup code in C or assembly may call it too. |
| `aura_heap_free_bytes` | `uint64_t aura_heap_free_bytes(void)` | What `mem.heap_free_bytes` calls. |

- **Regions.** The heap starts empty, and every request fails until the
  program adds a region. A program may add several, at any time. A region
  is never given back. A region too small to hold a 32-byte block is
  ignored.
- **Cost.** A block takes its size rounded up to a multiple of 16, plus 8
  bytes, and at least 32 bytes. The allocator adds 4 to 6 KB of code, by
  target, to the object.
- **State.** The allocator keeps its lists in 784 bytes of the object's
  `.bss` section. Startup code must zero `.bss` before the first
  allocation, as it must for C.
- **Reuse.** A freed block joins the free blocks on either side of it, so
  freeing everything leaves each region whole.
- **Cores.** A spin lock guards every call, so several cores may
  allocate. A call from an interrupt handler deadlocks if the handler
  interrupted an allocation. Turn interrupts off around allocations, or
  do not allocate in a handler.
- **Checks.** A failed check inside the allocator, which a damaged heap
  can cause, calls `aura_trap` with the file `aura/freestanding_heap.au`.

### Your Own Allocator

The five symbols are weak. A definition in C, assembly, or another object
replaces the build's at link time. Define all three hooks or none: memory
from one allocator must not reach the `free` of another.

```c
void *aura_alloc(size_t size, size_t align) { return kernel_alloc(size, align); }
void aura_free(void *address, size_t size, size_t align) { kernel_free(address, size); }
void *aura_realloc(void *address, size_t old_size, size_t align, size_t new_size) {
    return kernel_resize(address, old_size, new_size, align);
}
```

A hook written in Aura is an exported function:

```aura fragment
export "C" def aura_alloc(size: uint64, align: uint64) -> uint64:
    return bump(size, align)
```

A program that declares any of the three hooks, as an `export "C" def` or
an `extern "C" def`, gets no allocator from the build. The object then
refers to each heap symbol the program calls and does not define.

- A hook returns 0 when it has no memory. `mem.alloc` hands the 0 on.
- `align` is a power of two, and `size` is at least 1.
- `aura_free` gets the size and alignment the block was made with.

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
| `aura_alloc`, `aura_free`, `aura_realloc`, `aura_heap_add_region`, `aura_heap_free_bytes` | The program calls the `mem` function for one and declares an allocation hook itself. See [The Heap](#the-heap). |

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

- The heap values of the language are `str` and enums that hold
  themselves: no collection yet. A program allocates other memory
  through the [`mem` module](/manual/mem).
- Text has no formatting of numbers, no indexing, and no f-strings yet,
  and `str | None` is not available.
- A value that holds such an enum cannot be a member of a union, and a
  field that holds one cannot be assigned. Build a new value instead.
- Releasing a long list calls itself once for each link, so a very long
  list needs stack to match.
- No inline assembly and no port I/O instructions. Memory-mapped devices
  are reached with the [`mem` module](/manual/mem), and everything else
  through `extern "C"` functions.
- No mutable module-level state. Keep state in the entry function's locals,
  behind `mut` parameters, or in C.
- 32-bit targets are not supported.
