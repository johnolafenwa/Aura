---
layout: home

hero:
  name: Aura
  text: A Python-shaped language for agent control planes.
  tagline: Deterministic ownership, structured concurrency, and typed failure from files and processes through networking and retries.
  image:
    src: /aura-mark.svg
    alt: Aura language mark
  actions:
    - theme: brand
      text: Start Learning
      link: /learn/
    - theme: alt
      text: Open The Manual
      link: /manual/
    - theme: alt
      text: Why Aura
      link: /positioning

features:
  - title: Deterministic Ownership
    details: Bare parameters grant shared access, mut grants exclusive mutation, and own transfers a value. Cleanup follows the owning scope; this lifecycle contract is separate from task scheduling order.
  - title: Concurrency With A Scope
    details: Child work lives inside a TaskGroup. Leaving the scope waits for the children. Queues carry values between tasks. Cancellation is a signal the scope observes, not an exception that lands anywhere.
  - title: Typed Failure For Control Planes
    details: Files, subprocesses, sockets, retries, and supervisors return structured results. Failure is part of the type. Cleanup is part of the with block. Timeouts are arguments, not afterthoughts.
---

## What Aura Is

Aura is a compiled language for programs that manage resources on purpose: files, subprocesses, sockets, worker tasks, and the data that moves between them. It is statically typed, has no garbage collector, and carries its ownership and concurrency rules into ordinary application code rather than hiding them behind convention.

That combination is Aura's current wedge: deterministic ownership,
structured concurrency, and typed failure for agent control planes. Task
schedules remain deliberately unspecified. [Why Aura](/positioning) compares
the 0.2 technical preview with Mojo, Nim, Go, and free-threaded Python 3.13+,
and publishes the exact measured workloads behind the performance snapshot.

Three commitments shape every page of this book:

1. **Values have owners.** Move types — strings, collections, stateful random generators, class instances, task groups, file handles, sockets — transfer ownership in explicit `own` positions. Bare parameters grant shared access, while `mut` grants exclusive mutable access.
2. **Failure has a type.** Operations that a caller might handle return `Result`, `Option`, or a small set of outcome enums. Control flow over failure is visible in the program, not buried in hidden exception paths.
3. **Concurrency has a scope.** A `TaskGroup` owns its child tasks. The block that created the group is the block that waits for them, cancels them, and accounts for their results.

The maintained data surface also includes contiguous numeric `Array[T]`
values for `int32`, `int64`, `float32`, and `float64`. Their shapes are
explicit, arithmetic is same-dtype and exact-shape, and slices are owned
copies. See [Numeric Arrays](/manual/numeric-arrays).

## Measured Snapshot

On one post-reboot Mac14,9 (M2 Pro, 10 cores, 16 GiB), Aura's release
benchmark recorded these protocol-window medians against CPython 3.9.6. Lower
is faster; the ratio is Aura divided by CPython.

| exact workload | Aura | CPython | Aura / CPython |
| --- | ---: | ---: | ---: |
| naive recursive `fib(30)` | 93.875250 ms | 158.491666 ms | 0.592304 |
| create and join 10,000 tasks | 101.743042 ms | 51.950667 ms | 1.958455 |
| 20-client delayed loopback TCP fan-out | 104.505375 ms | 108.605459 ms | 0.962248 |
| 16-cycle retrying HTTP worker | 429.291292 ms | 520.447791 ms | 0.824850 |

See [Why Aura](/positioning#measured-snapshot) for methodology,
provenance, integer-loop results, numeric-Array measurements, and limitations.

## A First Program

```python
class Job:
    id: int32
    label: String

def render(job: Job) -> String:
    return f"#{job.id} {job.label}"

jobs = [Job(id=1, label="parse"), Job(id=2, label="compile")]

for job in jobs:
    print(render(job))
```

A few things are already in play. The class holds values that live together.
The bare `job: Job` parameter gives `render` implicit shared access because it
only needs to read. Bare vector iteration is also shared, so the jobs are still
there afterwards. The call `render(job)` satisfies that shared parameter
convention; nothing extra is needed at the call site.

These are the ideas the Learn track builds on.

## How To Read This Book

[Learn Aura](/learn/) is a guided path. It begins with runnable scripts, then grows into programs that have domain types, failure paths, owned resources, child tasks, and I/O.

[The Manual](/manual/) is the normative language reference and standard-library contract. Start with the [Language Specification](/manual/language-specification) for its authority and reading order, then use the complete grammar, semantic chapters, execution model, and API chapters to answer precise questions.

When a concept first appears in Learn, the Manual section that defines it is linked nearby.

## Running Aura

Once the CLI is on your path, the two commands you will use most are `run` and `build`:

```bash
aura run program.au
aura build -o ./program program.au
```

The next chapter covers getting the CLI installed.
