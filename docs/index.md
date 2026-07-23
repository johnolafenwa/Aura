---
layout: home

hero:
  name: Aurora
  text: A language for programs that own their resources.
  tagline: Explicit ownership, structured concurrency, and system APIs that return their failures instead of hiding them.
  image:
    src: /aurora-mark.svg
    alt: Aurora language mark
  actions:
    - theme: brand
      text: Start Learning
      link: /learn/
    - theme: alt
      text: Open The Manual
      link: /manual/

features:
  - title: Ownership You Can Read
    details: Every value has an owner. Bare non-copy parameters borrow; explicit own parameters transfer ownership. The compiler tracks this at every call boundary, and the runtime honours it without a garbage collector.
  - title: Concurrency With A Scope
    details: Child work lives inside a TaskGroup. Leaving the scope waits for the children. Queues carry values between tasks. Cancellation is a signal the scope observes, not an exception that lands anywhere.
  - title: APIs That Tell The Truth
    details: Files, subprocesses, sockets, and supervisors return structured results. Failure is part of the type. Cleanup is part of the with block. Timeouts are arguments, not afterthoughts.
---

## What Aurora Is

Aurora is a compiled language for programs that manage resources on purpose: files, subprocesses, sockets, worker tasks, and the data that moves between them. It is statically typed, has no garbage collector, and carries its ownership and concurrency rules into ordinary application code rather than hiding them behind convention.

Three commitments shape every page of this book:

1. **Values have owners.** Move types — strings, collections, stateful random generators, class instances, task groups, file handles, sockets — transfer ownership in explicit `own` positions. Bare non-copy parameters borrow; borrow forms can also make read or mutation access explicit.
2. **Failure has a type.** Operations that a caller might handle return `Result`, `Option`, or a small set of outcome enums. Control flow over failure is visible in the program, not buried in hidden exception paths.
3. **Concurrency has a scope.** A `TaskGroup` owns its child tasks. The block that created the group is the block that waits for them, cancels them, and accounts for their results.

## A First Program

```python
class Job:
    id: int32
    label: String

def render(job: borrow Job) -> String:
    return f"#{job.id} {job.label}"

jobs = [Job(id=1, label="parse"), Job(id=2, label="compile")]

for job in jobs:
    print(render(job))
```

A few things are already in play. The class holds values that live together. The function `render` explicitly borrows its job because it only needs to read. Bare vector iteration is shared, so the jobs are still there afterwards. The call `render(job)` passes the borrow form the signature asks for; nothing extra is needed at the call site.

These are the ideas the Learn track builds on.

## How To Read This Book

[Learn Aurora](/learn/) is a guided path. It begins with runnable scripts, then grows into programs that have domain types, failure paths, owned resources, child tasks, and I/O.

[The Manual](/manual/) is the normative language reference and standard-library contract. Start with the [Language Specification](/manual/language-specification) for its authority and reading order, then use the complete grammar, semantic chapters, execution model, and API chapters to answer precise questions.

When a concept first appears in Learn, the Manual section that defines it is linked nearby.

## Running Aurora

Once the CLI is on your path, the two commands you will use most are `run` and `build`:

```bash
aura run program.au
aura build -o ./program program.au
```

The next chapter covers getting the CLI installed.
