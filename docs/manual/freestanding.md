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

Every value is one or more machine words. Three kinds of value also own
blocks on a heap: text, lists, dictionaries, and an enum that holds
itself. See [Text](#text), [Lists](#lists), [Dictionaries](#dictionaries), and
[Enums That Hold Themselves](#enums-that-hold-themselves).
Other values live in registers, stack frames or static storage, and a
fixed array stores its elements inline.
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
| `bool`, `int8` to `int128`, `uint8` to `uint128`, `float32`, `float64` | `Duration` |
| Classes whose fields are available types, with methods | Classes that hold a `set` |
| `str` with literals, `+`, comparisons, `len`, `byte_len`, `starts_with`, `ends_with`, `contains`, and f-strings of text, integers, and `bool` | Every other `str` method, indexing, slices, floats and format specs in f-strings, `str \| None` |
| `list` of integers, floats, `bool`, `str`, classes, tuples, enums, lists, or unions of those, with literals, `append`, `pop`, `insert`, `swap`, `reverse`, `reserve`, `clear`, `len`, `is_empty`, element reads and writes, `view` of an element, and `for` loops | Lists of unions of words alone such as `list[int64 \| None]`, the other `list` methods, slices |
| `dict` with `int64`, `uint64`, or `str` keys and any value a list may hold, with literals, `d[key]`, `d[key] = value`, `key in d`, `len`, `is_empty`, `keys`, `values`, `items`, and `clear` | Other key types, `get`, `remove`, and the other `dict` methods |
| `extern "C"` classes, `fixed[T, N]` arrays and typed raw pointers | `set`, `bytes`, `Array` |
| Tuples of available types | Tuples that hold any other type |
| Enums and `Result` whose payloads are scalars, classes, tuples, or enums of available types, and `indirect` payloads of the enum's own type | Enums that hold a union or a collection, and `Lookup` |
| `T \| None` and other unions of available types, `str \| None` among them | Unions that hold any other type |
| Functions, generics, traits with static dispatch, `mut` parameters | Closures, function values, trait objects |
| `if`, `while`, `for` over `range`, a fixed array, a list, or a call of a generator function, `match` on a union or an enum, `try` on a `Result` of available types | `with`, tasks, queues |
| `assert`, `requires`, `ensures`, checked arithmetic, `as` casts | `print` |
| `abs`, `min`, `max`, `sqrt`, and the [`mem` module](/manual/mem) | Every other builtin module |
| `+ - * / // %` on integers, `+ - * /` on floats, shifts, bit operators, comparisons | `**`, float `//` and `%`, `round` |
| Pure module constants, including computed read-only tables, and mutable module cells with static initializers | Module bindings that require runtime initialization |
| `extern "C"` calls with scalar, `mut` scalar, struct, text, and callback arguments | Opaque handles and `bytes` arguments that need hosted storage. |

`aura check --freestanding` and the build report each construct outside the
subset as [`AU2052`](/manual/diagnostics), with the function that holds it:

```text
error[AU2052]: `entry` calls `print`, which a freestanding build cannot compile
 --> kernel.au:3:5
```

Only reached functions are checked. A package may hold hosted helpers and
tests that a freestanding build never compiles.

The compiler can build a collection in a pure helper on the build machine, then emit its values in the destination object's read-only data. Read the computed table from an export, using a runtime index if needed. The kernel uses this for its decimal digits. [Compile-Time Evaluation](/manual/compile-time)

## Module Storage

A module declaration such as `mut samples: fixed[uint16, 2] = [40, 0]` creates one writable cell shared by all exports that use it. A zero-initialized cell occupies `.bss`. Other initializers occupy `.data`. The startup code must clear `.bss` and, on boards that load writable data from ROM, copy the initial contents of `.data` to RAM before calling an export.

Use `samples[0]` to read or assign one element and `len(samples)` for its constant length. An unconstrained index is checked before the access. Ordinary hosted packages share mutable data through immutable bindings to synchronization handles. [Names and scopes](/manual/names-and-scopes) · [Fixed arrays](/manual/types)

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

An f-string may hold text, integers of every width up to 64 bits, and
`bool`: `f"{name}: {count}"`. A float, or a part with a format such as
`{count:>4}`, is not available yet.

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

## Lists

A freestanding program holds lists of integers, floats, `bool`, text,
classes, tuples, enums, and other lists.

```aura
def squares(n: int64) -> list[int64]:
    mut out: list[int64] = []
    for i in range(0, n):
        out.append(i * i)
    return out

export "C" def sum_of_squares(n: int64) -> int64:
    mut sum = 0
    for value in squares(n):
        sum = sum + value
    return sum
```

A list is a header block from the allocation hooks, with a reference
count, the length, and the capacity, and a buffer of 8-byte words. A
number takes one word, and a class, a tuple, or an enum takes one for
each of its words. The buffer doubles when it is full, through
`aura_realloc`. The compiler frees both with the list's last reference,
and first releases what each element owns: its text, its list, or its
tree.

Available: literals, `append`, `pop`, `insert`, `swap`, `reverse`,
`reserve`, `clear`, `len`, `is_empty`, `items[i]`, `items[i] = value`,
`view value = items[i]`, and `for value in items`. A negative index
counts from the end. A write to an element releases the value it replaces. A list of
`str | None` is available. A list of a union of words alone, such as
`list[int64 | None]`, is not available yet. A list may be a `mut` parameter, a result, or a field of a
class, a tuple, or an enum.

An index outside the list is a failed assertion in
`aura/freestanding_list.au`, trap code 3.

## Dictionaries

A freestanding program holds dictionaries whose keys are `int64`,
`uint64`, or `str`. A value is anything a list may hold.

```aura
def count_words(words: list[str]) -> dict[str, int64]:
    mut counts: dict[str, int64] = {}
    for word in words:
        if word in counts:
            seen = counts[word]
            counts[word.clone()] = seen + 1
        else:
            counts[word.clone()] = 1
    return counts

export "C" def repeats() -> int64:
    counts = count_words(["to", "be", "or", "not", "to", "be"])
    return counts["to"] + counts.len()
```

A dictionary is a header block from the allocation hooks, a buffer of
entries in the order they were added, and an index of the entries by
hash. Both buffers double when they are full. The compiler frees all
three with the dictionary's last reference, after it releases what each
key and each value owns.

Available: literals, `d[key]`, `d[key] = value`, `key in d`, `len`,
`is_empty`, `keys`, `values`, `items`, `clear`, and `view value =
d[key]`. `keys`, `values`, and `items` return lists, so `for (key, value)
in d.items()` runs in the order the keys were added. `get` and `remove`
are not available yet, because they return a `Lookup`.

`d[key]` for a key the dictionary does not hold is a failed assertion in
`aura/freestanding_dict.au`, trap code 3.

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
| 9 | Fixed-array index out of bounds |
| 10 | A frozen sequence index is out of range or a frozen dictionary key is absent |

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

Section and alignment also apply to mutable module storage and to explicitly
typed scalar module constants with literal initializers. For example,
`@machine.section(".board.config")` and `@machine.align(64)` above
`MAGIC: uint32 = 0x12345678` emit a four-byte value on a 64-byte boundary. A mutable cell keeps at least
its natural alignment and remains writable.

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

- The default build emits position-independent code.
- Synchronize access to mutable module state shared between cores or interrupt handlers.
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

The freestanding benchmark builds six routines with Aura, C and Rust, including a 16-element inline array updated over twenty million iterations. On x86-64 Linux, each executable ran on the same dedicated CPU core. The table gives the best elapsed time from nine runs per routine with the same C harness. The ratio compares Aura with the faster of C and Rust with overflow checks off. All four builds returned the same results.

| Routine | Aura | C | Rust | Rust with overflow checks | Aura / fastest C or Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `crc32` | 45.6 ms | 144.8 ms | 45.2 ms | 46.1 ms | 1.01× |
| `collatz` | 158.9 ms | 122.3 ms | 121.4 ms | 228.1 ms | 1.31× |
| `isqrt_sum` | 120.0 ms | 119.8 ms | 139.9 ms | 129.0 ms | 1.00× |
| `gcd_sum` | 171.7 ms | 120.2 ms | 121.7 ms | 136.4 ms | 1.43× |
| `orbit` | 880.5 ms | 1007.6 ms | 883.6 ms | 884.7 ms | 1.00× |
| `ring_mix` | 19.5 ms | 17.9 ms | 19.7 ms | 16.5 ms | 1.09× |

## Current Limits

- The heap values of the language are `str`, lists, dictionaries, and
  enums that hold themselves: no `set` yet, and a dictionary has no
  `get` and no `remove`. A program allocates other memory
  through the [`mem` module](/manual/mem).
- Text has no indexing or slices, an f-string takes no float and no
  format spec.
- A field that holds text, a list, or an enum that holds itself cannot
  be assigned. Build a new value instead.
- Releasing a long list calls itself once for each link, so a very long
  list needs stack to match.
- 32-bit targets are not supported.
