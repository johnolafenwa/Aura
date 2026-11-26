# Without an Operating System

Every program so far has run on top of an operating system and Aura's
runtime. The runtime is what gives you strings, lists, tasks, and files, and
it needs an operating system underneath for memory and threads. A kernel, a
bootloader, or the firmware of a small board has neither. This section
builds a program that boots on a bare machine, prints over a serial port,
and reports its own failed checks, with no C in it beyond a dozen lines of
startup assembly.

## A function C can call

A freestanding program has no `main`. Something else decides when its code
runs: a bootloader, an interrupt, or a C program that links it in. So the
program's entry points are ordinary functions that the outside world can
call by name. You mark one with `export "C"`:

```aura
export "C" def checksum(seed: uint32, count: uint32) -> uint32:
    mut total = seed
    mut i: uint32 = 0
    while i < count:
        total = (total * 31 + i) % 65521
        i = i + 1
    return total
```

Save that as *sum.au* and build it with one new flag:

```console
$ aura build --freestanding -o sum.o sum.au
aura: built `sum.o` for `x86_64-unknown-none` (freestanding object, 1600 bytes)
```

The result is an object file, not a program. It holds `checksum` as a C
symbol and depends on nothing but one function we will meet in a moment. A
C program can link it and call `uint32_t checksum(uint32_t, uint32_t)`.

The parameters and the result of an exported function are numbers and
`bool`. A `mut` parameter is a pointer on the C side, which is how such a
function hands back a second result.

## What fits in a freestanding program

Aura keeps a number, a `bool`, or a class made of those directly in machine
registers and on the stack. Those values need no runtime, and they are what
a freestanding program is made of: fixed-width integers and floats, classes
of them, tuples and enums of them, `T | None`, functions, generics,
traits, loops, `match`, `assert`, and contracts.

Some of the values Aura keeps on the heap for you are out:
sets, closures, tasks. Four kinds are in. A dictionary works, with text or integer keys. A list
works, of numbers, text, classes, enums, or other lists, with `append`, indexing, and `for`. A `str` works, with `+`, comparisons, and a few methods, so a kernel
can build a message and hand it to its console. And an enum may hold
itself through an `indirect` payload, so you can write a linked list or a
tree. The compiler frees each block when nothing refers to it. You do not have to remember the list. Ask
the compiler:

```console
$ aura check --freestanding kernel.au
error[AU2052]: `entry` calls `print`, which a freestanding build cannot compile
 --> kernel.au:3:5
```

The check looks only at the functions your exports reach. A package can
keep hosted helpers and tests next to its freestanding code, and
`aura test` still runs the logic on your own machine. That is the practical
way to work: test the arithmetic hosted, then build it freestanding.

## When a check fails

Aura's checks do not go away. An addition that overflows, a division by
zero, a failed `assert`: each one still stops the program. With no runtime
to print a message, the program calls a function named `aura_trap` and you
decide what happens:

```aura
export "C" def aura_trap(code: uint32, file: uint64, file_length: uint64, line: uint32, column: uint32) -> None:
    while true:
        pass
```

`code` says which check failed, 1 for overflow and 2 for division by zero
among others, and the rest is the position in your source. The hook must
not return. This one hangs the machine. A real one prints the position
first, as ours will.

## Checks that cannot fail

A trap on a board with no screen is a hang, or a line on a serial port
nobody is watching. So freestanding code is where it pays most to know
before boot that a check cannot fail, and `aura verify` from
[Chapter 11](/book/ch11-04-contracts-and-proofs) works on this code as on
any other. Let's ask it about `checksum`:

```console
$ aura verify sum.au
error[AU5101]: cannot prove integer result in range
 --> sum.au:5:18
  |
5 |         total = (total * 31 + i) % 65521
  |                  ^
  = note: in `checksum` (overflow check)
  = note: counterexample: seed = 0, count = 1
verified sum.au: 2 of 3 checks proved in 1 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

It found a way into `aura_trap`. The `% 65521` keeps `total` small after
each step, but nothing keeps it small before the first one: a caller that
passes a seed of four billion overflows `uint32` on the first
multiplication. The counterexample looks odd because the verifier proves a
loop one iteration at a time, so the `total` it has in mind is any `uint32`
at all, and it prints only the parameters. The fix is to state the range of
the seed, and to tell the verifier what the loop keeps true:

```aura
export "C" def checksum(seed: uint32, count: uint32) -> uint32:
    requires seed < 65521
    requires count <= 1_000_000
    ensures result < 65521
    mut total = seed
    mut i: uint32 = 0
    while i < count:
        invariant total < 65521
        total = (total * 31 + i) % 65521
        i = i + 1
    return total
```

```console
$ aura verify sum.au
verified sum.au: 5 of 5 checks proved in 1 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Now the arithmetic cannot trap for any seed the contract allows. A proved
check is one the object file does not need to carry, and the same flag that
removes proved checks from a hosted binary works here:

```console
$ aura build --freestanding --remove-proved-checks -o sum.o sum.au
aura: 6 checks proved and removed (0 bounds, 4 arithmetic, 2 assertion)
aura: built `sum.o` for `x86_64-unknown-none` (freestanding object, 1888 bytes)
```

The checks on the arithmetic in the loop are gone, along with the run-time
checks of the `invariant` and the `ensures`. The two `requires` clauses
stay. `checksum` is exported, so its
callers are C code that no verifier has read, and a seed out of range still
reaches `aura_trap` with code 4, the code for a failed contract. That is the
shape to aim for in a kernel: checks at the edge where foreign code calls
in, and proved arithmetic behind them.

## Touching hardware

A serial port on most boards is a few bytes of memory at a fixed address:
write a byte there and it goes out on the wire. Aura has no pointers, so
how do we write to an address? With the `mem` module, which reads and
writes integers at a `uint64` address:

```aura fragment
import mem

def put(uart: uint64, byte: uint8) -> None:
    mem.volatile_write_uint8(uart, byte)
```

The word *volatile* matters. An ordinary write to memory is something the
optimizer may drop if a later write replaces it. For a device, every write
is an event, so `volatile_write_uint8` promises exactly one write, exactly
here.

`mem` is the sharpest tool in the language. Nothing checks the address, and
a wrong one can corrupt anything. For that reason it needs the same
permission as calling C: `allow_ffi = true` in *Aura.toml*.

It is also where proofs end. To `aura verify`, a `mem` read returns an
unknown value of its type, so a status register may hold anything on every
read, which is the honest view of hardware. Writes are not modeled at all.
The verifier cannot tell you that an address is right, and a write that
lands on memory an Aura value occupies is outside anything a proof covers.
Arithmetic on a value after you have read it is checked like any other.

There are no strings in a freestanding program, so printing a number means
producing its digits ourselves:

```aura fragment
def put_number(uart: uint64, value: uint64) -> None:
    mut divisor: uint64 = 1
    while value // divisor >= 10:
        divisor = divisor * 10
    mut rest = value
    while divisor > 0:
        put(uart, (48 + rest // divisor) as uint8)
        rest = rest % divisor
        divisor = divisor // 10
```

## Memory that grows

Everything so far has a size the compiler knows. A kernel also keeps
tables that grow: the processes it runs, the pages it has handed out. For
those it needs a heap, and a freestanding program has one as soon as it
says where the heap may live:

```aura fragment
mem.heap_add_region(heap_start(), heap_size())
```

From then on `mem.alloc` hands out blocks of that region, `mem.realloc`
resizes one, and `mem.free` gives it back. Here is a list of numbers that
doubles its room whenever it fills:

```aura fragment
mut room: uint64 = 4
mut count: uint64 = 0
mut primes = mem.alloc(room * 8, 8)
assert primes != 0, "the heap has no memory"
...
if count == room:
    primes = mem.realloc(primes, room * 8, 8, room * 16)
    assert primes != 0, "the heap is full"
    room = room * 2
mem.write_uint64(primes + count * 8, candidate)
count = count + 1
```

Two things differ from the lists you know. A block is an address and
nothing more, so you read and write it with `mem` and you free it
yourself, passing the size you asked for. And `mem.alloc` returns 0 when
the heap is full, where a hosted program would stop. A kernel has to be
able to say no to a request and keep running.

Where does the allocator come from, with no runtime? It is a few hundred
lines of Aura, and the compiler adds it to your object when you call
`mem.alloc`. A kernel that already has an allocator defines three
functions, `aura_alloc`, `aura_free`, and `aura_realloc`, and Aura calls
those instead.

## Booting it

The complete program is in *examples/freestanding/kernel*. Its
`kernel_main` counts primes, runs a float computation, takes a spin lock
built from `mem.atomic_compare_exchange_uint32`, sums the primes from a
block on the heap, and then overflows a `uint8` on purpose. Each board
adds a short assembly file that sets the stack pointer, turns the float
registers on, and calls `kernel_main`, plus a linker script that says
where memory is.

QEMU can emulate both boards the example supports:

```console
$ examples/freestanding/kernel/run.sh riscv64
p=168
c=111
r=1000
n=100
h=76127
f=0
trap 1 main.au:204:13
```

The first six lines are computed results. `h` is the sum of the primes
below 1000, read back from the heap, and `f` says that no byte of the
heap is missing after the block is freed. The last is our `aura_trap`
hook reporting the overflow, with the file and line it happened on, from a
machine that has no operating system at all. Run it with `aarch64` and the
output is the same.

## What is missing

This is the first step toward systems work in Aura, and it is honest to
say where it stops. The heap holds blocks of bytes, text, lists, and the nodes of
enums that hold themselves, and dictionaries: there are no sets yet,
and text cannot format a float. There is no inline assembly, so startup code and x86 port
I/O stay in an assembly file. State that outlives a function call has to
live at an address you manage through `mem`, where `aura verify` cannot
follow it. The
[Freestanding Builds](/manual/freestanding) and
[Memory Module](/manual/mem) pages have the exact rules.
