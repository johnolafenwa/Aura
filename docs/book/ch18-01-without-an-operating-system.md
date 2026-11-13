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
of them, `T | None`, functions, generics, traits, loops, `assert`, and
contracts.

Anything that lives on the heap is out: `str`, lists, dictionaries, enums
with payloads, closures, tasks. You do not have to remember the list. Ask
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

## Booting it

The complete program is in *examples/freestanding/kernel*. Its
`kernel_main` counts primes, runs a float computation, takes a spin lock
built from `mem.atomic_compare_exchange_uint32`, and then overflows a
`uint8` on purpose. Each board adds a short assembly file that sets the
stack pointer, turns the float registers on, and calls `kernel_main`, plus
a linker script that says where memory is.

QEMU can emulate both boards the example supports:

```console
$ examples/freestanding/kernel/run.sh riscv64
p=168
c=111
r=1000
n=100
trap 1 main.au:154:13
```

The first four lines are computed results. The last is our `aura_trap`
hook reporting the overflow, with the file and line it happened on, from a
machine that has no operating system at all. Run it with `aarch64` and the
output is the same.

## What is missing

This is the first step toward systems work in Aura, and it is honest to
say where it stops. There is no allocator, so there are no strings or
collections. There is no inline assembly, so startup code and x86 port
I/O stay in an assembly file. State that outlives a function call has to
live at an address you manage through `mem`. The
[Freestanding Builds](/manual/freestanding) and
[Memory Module](/manual/mem) pages have the exact rules.
