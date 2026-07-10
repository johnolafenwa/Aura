# Learn Aurora

This book teaches Aurora the way a programmer tends to actually learn a language: by writing short programs that do something real, then extending them until the pieces fit together.

Each chapter introduces one part of the language through a program that would make sense to run. By the end of the track you will have built command-style tools, domain models, text parsers, concurrent worker pools, subprocess runners, and small network services, and you will have met the language rules that keep those programs honest.

## The Three Questions Aurora Wants You To Ask

Programs in Aurora tend to be easier to read when three questions are answered near the code.

**Who owns this value?**
Numbers, booleans, durations, queue handles, and task handles are copy values: binding one to a new name is cheap and both names remain valid. Strings, collections, class instances, file handles, process resources, task groups, and network resources are move values: passing one transfers ownership to the recipient, and the original binding can no longer be used until it is reassigned. This rule is never hidden; it is part of the type.

**Can this call fail?**
Failure that a caller might sensibly handle lives in the return type. `Result[T, E]`, `Option[T]`, `QueueReceive[T]`, `TaskResult[T]`, and the I/O and process error enums let a program decide what to do with a given failure on a given line rather than catching a broad exception somewhere else.

**What closes this resource?**
Files, network sockets, subprocess pipes, supervisors, and task groups should normally live inside a `with` block. The block is what runs cleanup — on normal exit and on runtime errors that unwind through it. `with` is how you turn "please remember to close this" into "this closes itself."

## The Shape Of An Aurora Program

A complete script:

```python
class Point:
    x: float64
    y: float64

def distance(point: borrow Point) -> float64:
    return sqrt((point.x * point.x) + (point.y * point.y))

point = Point(x=3.0, y=4.0)
print(distance(point))
```

Several ideas are already visible. `Point` is a class with named fields. `distance` takes a borrowed point because it only needs to read one; the call site just passes the value and Aurora supplies the borrow. `print` renders a value and adds a newline. The script runs top to bottom; no `main` is required.

Run it with:

```bash
aura run examples/classes/point_distance.au
```

## What This Track Covers

The chapters are ordered so that each idea has a practical reason to exist before the formal rules arrive.

1. [Getting Aurora Running](/learn/install-and-run) — install the CLI, run your first program, build your first binary.
2. [The First Program](/learn/small-programs) — bindings, functions, control flow, and small decisions made with `match`.
3. [Shaping Data](/learn/data-modeling) — classes, enums, methods, and the patterns that keep domain data honest.
4. [Working With Collections](/learn/collections) — `Vec[T]`, `Map[K, V]`, and `Set[T]` on real text and counting problems.
5. [Values, Moves, And Borrows](/learn/ownership-and-borrowing) — the ownership model, explained through the programs that benefit from it.
6. [Results, Options, And `try`](/learn/results-and-options) — how Aurora represents recoverable failure without hiding control flow.
7. [Organizing Code](/learn/modules-and-packages) — splitting a program into files, packages, and workspaces.
8. [Structured Concurrency](/learn/concurrency) — `TaskGroup`, `Task[T]`, `Queue[T]`, cancellation, and worker pools.
9. [Talking To The World](/learn/io-process-networking) — files, processes, sockets, HTTP, and supervisors.
10. [Running And Shipping](/learn/native-builds) — when to use `run`, when to use `build`, and what the native binary gives you.

Three case studies put the pieces together:

- [Log Analyzer](/learn/case-studies/log-analyzer) — a text-processing tool with parsing, aggregation, and a report.
- [Queue Worker Pool](/learn/case-studies/queue-worker-pool) — a structured-concurrency pattern that shuts down cleanly.
- [Supervised Process Runner](/learn/case-studies/process-supervisor) — a small service supervisor with a restart policy and an event stream.

## Reading The Manual Alongside Learn

Each Learn chapter ends by pointing at the matching Manual section. When a rule is surprising or a contract needs checking, go straight to the reference:

- [Types](/manual/types)
- [Ownership And Borrowing](/manual/ownership-and-borrowing)
- [Collections](/manual/collections)
- [Concurrency](/manual/concurrency)
- [Process Module](/manual/process)
- [API Index](/manual/api-index)

The Manual is deliberately less chatty than Learn. It says what a thing is, not why you might want it.
