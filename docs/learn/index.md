# Learn Aura

This book teaches Aura through short programs that do something real. Each
chapter extends them until the pieces fit together.

Every chapter introduces one part of the language through a program worth
running. By the end you will have built command-style tools, domain models,
text parsers, concurrent worker pools, subprocess runners, and small network
services. Along the way you will meet the language rules that keep those
programs correct.

## The Three Questions Aura Wants You To Ask

Aura programs are easier to read when the code answers three questions close
to where it matters.

**Who owns this value?**
Small values copy: numbers, booleans, durations, and queue handles. Binding
one to a new name is cheap, and both names keep working. Everything else
moves: strings, collections, class instances, files, processes, task groups,
and network resources. Assignment hands ownership over. A task handle sits in
between. It copies when its result can be read more than once. At a call
boundary, the signature tells you which applies. A bare parameter shares, and
`own` transfers.

**Can this call fail?**
Failure that a caller might reasonably handle lives in the return type.
`Result[T, E]`, `T | None`, `Lookup[T]`, `QueueReceive[T]`, `TaskResult[T]`,
and the I/O and process error enums let a program handle each failure at the
line where it matters.

**What closes this resource?**
Files, network sockets, subprocess pipes, supervisors, and task groups should
normally live inside a `with` block. The block runs cleanup on normal exit and
on runtime errors that unwind through it. With `with`, "remember to close
this" becomes "this closes itself."

## The Shape Of An Aura Program

A complete script:

```aura
class Point:
    x: float64
    y: float64

def distance(point: Point) -> float64:
    return sqrt((point.x * point.x) + (point.y * point.y))

point = Point(x=3.0, y=4.0)
print(distance(point))
```

Several ideas already show:

- `Point` is a class with named fields.
- `distance` only reads its argument. The bare parameter grants shared
  access, so the caller keeps the point.
- `print` renders a value and adds a newline.
- The script runs top to bottom. It needs no `main`.

Run it with:

```bash
aura run examples/classes/point_distance.au
```

## What This Track Covers

The chapters are ordered so that each idea has a practical reason to exist
before its formal rules arrive.

1. [Getting Aura Running](/learn/install-and-run): install the CLI, run your first program, and build your first binary.
2. [Aura For Python Developers](/learn/from-python): the fast track. What
   carries over from Python, and what will surprise you.
3. [The First Program](/learn/small-programs): bindings, functions, control flow, and small decisions made with `match`.
4. [Shaping Data](/learn/data-modeling): classes, enums, methods, and the patterns that keep domain data consistent.
5. [Working With Collections](/learn/collections): `list[T]`, `dict[K, V]`,
   `set[T]`, eager owned comprehensions, owned list and str slices, and
   fixed-shape numeric `Array[T]` values.
6. [Converting Between Types](/learn/casting): `as`, `.to_float()`, parsing
   text, and why nothing converts implicitly.
7. [Values, Moves, And Borrows](/learn/ownership-and-borrowing): the ownership model, explained through the programs that benefit from it.
8. [Results And Optional Values](/learn/results-and-options): how Aura represents absence and recoverable failure without hiding control flow.
9. [Testing](/learn/testing): writing tests, reading a failed assertion, parameterized cases, and CI output.
10. [Organizing Code](/learn/modules-and-packages): splitting a program into files, packages, and workspaces.
11. [Structured Concurrency](/learn/concurrency): `TaskGroup`, `Task[T]`, `Queue[T]`, cancellation, and worker pools.
12. [Talking To The World](/learn/io-process-networking): files, processes, sockets, HTTP, and supervisors.
13. [Running And Shipping](/learn/native-builds): when to use `run`, when to use `build`, and what the native binary gives you.
14. [Calling A Small C API](/learn/ffi): package-authorized FFI v0 for
    fixed-width values, temporary byte views, and opaque handles.

Three case studies put the pieces together:

- [Log Analyzer](/learn/case-studies/log-analyzer): a text-processing tool with parsing, aggregation, and a report.
- [Queue Worker Pool](/learn/case-studies/queue-worker-pool): a structured-concurrency pattern that shuts down cleanly.
- [Supervised Process Runner](/learn/case-studies/process-supervisor): a small service supervisor with a restart policy and an event stream.

## Reading The Manual Alongside Learn

Each Learn chapter ends with a link to the matching Manual section. When a
rule surprises you or you need to check a contract, go straight to the
reference:

- [Types](/manual/types)
- [Ownership And Borrowing](/manual/ownership-and-borrowing)
- [Collections](/manual/collections)
- [Numeric Arrays](/manual/numeric-arrays)
- [Concurrency](/manual/concurrency)
- [Process Module](/manual/process)
- [Foreign Function Interface (FFI) v0](/manual/ffi)
- [API Index](/manual/api-index)

The Manual is terser than Learn. It says what a thing is, not why you might
want it.
