---
layout: home

hero:
  name: Aura
  text: Compiled. Statically typed. Familiar.
  tagline: Python-inspired syntax with deterministic ownership, structured concurrency, and native executables.
  image:
    src: /aura-mark.svg
    alt: Aura language mark
  actions:
    - theme: brand
      text: Start Learning
      link: /learn/
    - theme: alt
      text: Read The Manual
      link: /manual/
    - theme: alt
      text: AI Agent Docs
      link: /#ai-agents
features:
  - title: Native Compilation
    details: Build native executables that need no language interpreter and no garbage collector.
  - title: Static Types
    details: Every expression has a type. Before the program runs, the compiler checks calls, fields, ownership, mutation, matches, and task boundaries.
  - title: Ownership-Based Reliability
    details: The compiler enforces explicit rules for shared access, mutation, ownership transfer, and resource cleanup.
---

## Why Aura

Aura is a compiled, statically typed language with familiar,
indentation-based syntax. The compiler checks types, ownership, mutation,
exhaustive matching, failure handling, and task boundaries. Programs build as
native executables with deterministic cleanup and no garbage collector.

The current preview is designed for reliable applications, agent runtimes,
machine learning (ML) infrastructure, evaluation workers, and control-plane
services.

Aura is a technical preview. The language and its APIs may change before a
stable release.

## At A Glance

| | Python | Rust | Aura |
| --- | --- | --- | --- |
| Syntax | Indentation-based and concise | Explicit and low-level | Python-inspired and indentation-based |
| Types | Dynamic, with optional hints | Static | Static, with inference |
| Execution | Interpreter and virtual machine | Native executables | Native executables |
| Memory | Reference counting and garbage collection | Ownership | Ownership, no garbage collector |
| Failure | Exceptions | `Result`, `Option`, panics | Typed `Result`, `Option`, outcome enums |
| Concurrency | Threads and async functions | Threads and async ecosystem | Structured task groups across multiple cores |
| Current focus | General-purpose applications and scripting | Systems and application software | Reliable applications, agents, and ML infrastructure |

## A First Program

```aura
def scale(values: mut list[int64], factor: int64):
    for value in mut values:
        value *= factor

def total(values: list[int64]) -> int64:
    mut sum = 0
    for value in values:
        sum += value
    return sum

mut scores = [10, 20, 30]
scale(scores, 3)

print(f"scores: {scores}")
print(f"total: {total(scores)}")
```

Each signature says what the function does to its arguments. `scale` asks for
`mut` access and changes the list in place. `total` only reads it. The
compiler enforces both contracts, and every operation is statically checked.

## Built For Agents And ML Infrastructure

Serving models and running agents means working with sockets, subprocesses,
queues, deadlines, and retries. Aura's rules make the ways these can fail
visible in the code:

- **Values have owners.** A bare parameter shares, `mut` mutates, and `own`
  transfers. Cleanup follows the owning scope.
- **Failure has a type.** Recoverable failures return `Result`, `Option`, or
  an outcome enum, and the code handles them where they happen.
- **Concurrency has a scope.** A `TaskGroup` owns its child tasks. Leaving the
  scope joins them, cancels stragglers, and loses nothing.
- **The standard library covers infrastructure.** Files, processes, TCP,
  HTTP, WebSockets, TLS, queues, retries, and supervisors follow the same
  ownership and failure rules as the rest of the language.

## Long-Term Direction

Aura aims to become a general-purpose systems language that can build every
type of software. The intended scope spans applications, services, databases,
language runtimes, embedded software, operating systems, and device drivers.

Aura 0.3 lays the foundation: static typing, deterministic ownership, native
compilation, structured concurrency, typed failure, packages, and integrated
tooling. Later releases will add freestanding compilation, low-level memory
access, hardware interfaces, portable layout controls, cross-compilation, and
specialized runtime profiles.

<AgentDocs />

## Start Building

[Install Aura](/install/), then run a file or build a native executable:

```bash
aura run program.au
aura build -o ./program program.au
```

[Learn Aura](/learn/) starts with runnable scripts and works up to tasks,
typed failures, and I/O. [The Manual](/manual/) is the normative reference. It
covers the grammar, ownership rules, execution model, APIs, diagnostics, and
limits.
