# Performance

"Is it fast?" is the wrong question about a language, because the answer
depends on what your program does. The useful questions are narrower. Which
operations are cheap in Aura today, and which still cost more than they would
in Rust or C? How do you find out where your own program spends its time? And
which published numbers can you trust? We will take them in order, and we
will be careful about where every number comes from. Some come from
measurements made while writing this chapter; the rest come from the
Manual's [Performance](/manual/performance) page, which records each
measurement with its workload, machine, compiler commit, and raw data.

A word about the measurements made for this chapter. They ran on a Linux
x86-64 container with four cores, shared with other work, using a release
build of the Aura 0.11 compiler. A shared machine is noisy, so treat each
number as a rough size rather than a benchmark result. The comparisons
within one setup, and the advice that follows from them, are what to take
away.

## Measure the binary, not `aura run`

Let's measure something. This program finds the starting number below one
million with the longest Collatz sequence, a classic integer-heavy loop:

*Filename: collatz.au*

```aura
def steps(start: int64) -> int64:
    mut n = start
    mut count: int64 = 0
    while n != 1:
        if n % 2 == 0:
            n = n // 2
        else:
            n = 3 * n + 1
        count += 1
    return count

def main():
    mut longest: int64 = 0
    mut best: int64 = 1
    for start in range(1, 1000000):
        length = steps(start)
        if length > longest:
            longest = length
            best = start
    print(f"{best} takes {longest} steps")
```

<span class="caption">Listing 17-2: An integer loop to time</span>

Build it and time it with the shell's `time`:

```console
$ aura build -o collatz collatz.au
aura: built `collatz` with direct backend
$ time ./collatz
837799 takes 524 steps

real	0m0.435s
user	0m0.434s
sys	0m0.000s
```

The same algorithm in Python 3.13.18 on the same container took 8.7 seconds,
about twenty times longer. That ratio is for this program only. Integer
loops are where a compiled language shines most, and Python's integers are
arbitrary-precision objects, while Aura's `int64` is a machine word with an
overflow check on every operation. Aura checks arithmetic because silent
wraparound is a common source of wrong answers, and as this run shows, the
checks do not stop an integer loop from running at native speed.

Do not time `aura run` for questions like this. It executes the program's
MIR, which is meant for a fast edit-and-run loop, not for speed. Even with a
release compiler, `aura run` needed 8.3 seconds for only the first ten
thousand starting numbers, a case the native binary finished in 6
milliseconds. MIR execution is never what you ship. Measure what you ship: the
output of `aura build`, or
`aura run --backend direct`, which runs the same native code.

When you time something, run it several times and look at the spread, keep
other heavy work off the machine, and time the whole process the way users
will run it. Three earlier runs of `collatz` took 0.463, 0.436, and 0.414
seconds, which is consistent enough to trust the first digit.

## What is cheap

A few things that you might expect to cost something in Aura cost little or
nothing, because of how ownership works.

Passing a value with shared access, the default for a bare parameter, does
not copy it and usually does not even touch a reference count. The compiler
knows the callee cannot change or rebind the value while the call runs, so
the callee reads the caller's value where it is. Passing to a `mut`
parameter moves the value into the call and back out, again without a copy.
The Performance page measured this across 30 benchmark programs: after these
two changes, generated code retained values about 62% less often than before
(1,062,951 retains down to 400,936).

Writes to a collection you own, such as `append`, `remove`, `sort`, or a
dictionary insert, change the collection in place. And a `.copy()` of a
`list`, `dict`, or `set` does not copy the elements right away: the two
copies share storage until one of them is written to. So copying a list to
hand to a function that only reads it is cheap, and you pay for the copy only
if somebody writes.

Numbers, `bool` values, and `copy class` records live in machine words, and a
list literal of them is built from their bits without allocating a box per
element. Integer and floating-point loops like `collatz` compile to the kind
of code you would expect from a systems language.

I/O-bound work is close to the native baseline as well. On the Performance
page's foundations host (an Apple M2 Pro), a benchmark that fans out 20
delayed TCP clients over loopback took 104.3 ms in Aura, 104.6 ms in Rust, and
108.8 ms in CPython, and a retrying HTTP worker took 428.8 ms in Aura against
472.6 ms in Rust and 522.4 ms in CPython. When the program spends its time
waiting on the network, the language matters less than the protocol.

## What still costs

Other things cost more than they will eventually, and it is better to know
where they are.

Tasks are cheap compared with threads, but they are not free compared with
calls. Here is a quick experiment: square 10,000 numbers by starting one task
for each, in ten task groups of a thousand, and compare it with ten thousand
ordinary calls. With a release compiler on a Linux x86-64 container, the
task version took about half a second and the call version 3 milliseconds. On the
Performance page's foundations host with a release build, creating and
joining 10,000 tasks took 98.7 ms, against 51.8 ms in CPython and 3.4 ms in
Rust (that measurement predates several releases of runtime work, so treat it
as an order of magnitude). The lesson is the usual one for any task system:
give each task a real piece of work, such as a request, a file, or a batch,
rather than one multiplication.

Function calls carry a small frame record, which is what makes the call
chains in trap diagnostics possible. For call-heavy code such as naive
recursive Fibonacci, that record is a visible part of the cost. The
Performance page reports that Aura 0.7.0 ran `fib(30)` in 2.92 times the
instructions of the Rust version on Linux.

Values that need a heap box, such as strings, class instances, and enum
payloads, cost an allocation, and programs dominated by small strings,
dictionaries, and JSON are where Aura is furthest from Rust. You can see
allocations directly: set `AURA_RUNTIME_STATS=1` and the runtime prints its
own counters on standard error when the program ends. Here is a loop that
formats a thousand numbers into strings and parses them back through a
`int64 | None` union:

```aura
def parse(text: str) -> int64 | None:
    if text == "":
        return None
    return text.len()

def main():
    mut total: int64 = 0
    for i in range(0, 1000):
        match parse(f"{i}"):
            case int64 as n:
                total += n
            case None:
                pass
    print(total)
```

```console
$ aura build -o boxes boxes.au
aura: built `boxes` with direct backend
$ AURA_RUNTIME_STATS=1 ./boxes
2890
aura runtime stats (direct): union_payload_boxes=0 closure_environments=0 opaque_boxes=1001 callable_overflow_allocations=0 cloned_elements=0 retains=0 releases=1001 clones=0 freed_boxes=1001 conversions=0
```

There are 1,001 runtime boxes, roughly one per formatted string, all of
them freed, and no boxes at all for the union, which native code keeps
unboxed. The same counters under `aura run` show different numbers, because
the MIR executor represents values differently; compare counters only between
runs of the same backend.

## The big picture

The best single summary of where Aura stands is the Performance page's set of
30 cross-language programs: complete programs written in Aura, Python, and
Rust that do the same work, from a SQL engine to a ray tracer. For Aura
0.11.0, measured on a shared four-core Linux container, median of ten runs:

- the median program took 3.7 times as long as Rust and 0.40 times as long as
  Python 3.13,
- 25 of the 30 programs were faster than Python, and none was more than 8
  times slower than Rust,
- the programs still slower than Python were `csv_aggregate`, `docdb`,
  `file_io`, `json_roundtrip`, and `template_render`, all of them heavy on
  strings, dictionaries, and JSON.

In Aura 0.8.0 the same median was 16 times Rust. The trend is the point:
each release has taken specific costs out, and the Performance page lists
each step with before and after instruction counts. When you read a claim
about Aura's speed anywhere, including in this book, check it against that
page, which also says which measurements are contractual (repeatable under a
strict runner) and which are diagnostic-grade (useful, but taken on a noisy
machine).

## Profiling your own program

When a program is slower than you need, find out where the time goes before
changing anything. Two settings help.

A normal build strips local symbols, which leaves a profiler with little to
name. Set `AURA_NATIVE_KEEP_SYMBOLS=1` when building, and the strip step is
skipped:

```console
$ AURA_NATIVE_KEEP_SYMBOLS=1 aura build -o collatz-prof collatz.au
aura: built `collatz-prof` with direct backend
```

Only the exact value `1` turns this on. It keeps whatever symbols the build
has; it does not restore symbols already removed from the runtime library,
and small Aura functions are often inlined into their callers by the
optimizer, so do not expect every function you wrote to show up by name. In
the build above, `steps` had no symbol of its own. When you need time per
Aura function, profile a debug build, which names every function, and
remember that it is unoptimized, so use it to compare parts of your program
with each other rather than to judge absolute speed. Kept symbols are part of
the cache key, so profiling builds and normal builds do not overwrite each
other in the native cache.

The usual native profilers work on these binaries: `perf` on Linux,
Instruments or `xctrace` on macOS, and Valgrind's `callgrind`, which counts
instructions instead of measuring time and is the tool the Performance page
uses on its noisy shared container.

`AURA_WORKERS` sets the number of scheduler worker threads, which defaults to
the number of cores. `AURA_WORKERS=1` runs every task on one worker. Comparing
a concurrent program at one worker and at several tells you quickly whether it
is limited by computation that parallelizes, or by something else, such as a
single queue every task waits on.

And when you do change something, measure again, the same way as before.
Performance intuition built in another language transfers less than you
would think, in both directions.

## Summary

You can now take an Aura program from source to something you can ship.
`aura run` executes MIR for fast iteration, and `aura build` produces a
native binary with the runtime linked in, which runs on a machine with no
compiler and reports traps with the same source-located diagnostics. A debug
build with `--debug` adds what lldb and VS Code need for breakpoints,
stepping, and Aura-typed variables. And you know how to measure a program:
time the native binary, count allocations with `AURA_RUNTIME_STATS=1`,
profile with symbols kept, and read published numbers with their context.

Everything so far has been Aura calling Aura, or Aura calling the operating
system through its standard modules. Sometimes the function you need already
exists in a C library. [Chapter 18](/book/ch18-00-calling-c) shows how Aura
calls C, and why it keeps that door deliberately narrow.
