---
layout: home

hero:
  name: Aura
  text: Compiled. Statically typed. Familiar.
  tagline: Python-inspired syntax with deterministic ownership, contracts the compiler can prove, structured concurrency, and native executables.
  image:
    src: /aura-mark.svg
    alt: Aura language mark
  actions:
    - theme: brand
      text: Start Learning
      link: /book/
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
  - title: Proved Before It Runs
    details: Functions state what they need and promise with requires and ensures. aura verify proves those contracts, and that no integer overflows and no index is out of range, for every input.
---

## Why Aura

Aura is a compiled, statically typed language with familiar,
indentation-based syntax. The compiler checks types, ownership, mutation,
exhaustive matching, failure handling, and task boundaries. `aura verify`
proves that a program's contracts hold and that its arithmetic and indexing
cannot trap, for every input. Programs build as native executables with
deterministic cleanup and no garbage collector.

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
| Correctness beyond types | Tests and assertions | Tests and assertions | Contracts, checked at run time and proved by `aura verify` |
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

## Proved, Not Only Tested

A function can also state what it needs from its caller and what it promises
in return. `aura run` checks those contracts while the program runs.
`aura verify` proves them for every input, together with the traps the body
could hit.

```aura
def average(scores: list[int64]) -> int64:
    requires 0 < len(scores) and len(scores) <= 1000
    requires forall(s in scores, 0 <= s and s <= 100)
    ensures 0 <= result and result <= 100
    mut total = 0
    for i in range(len(scores)):
        invariant 0 <= total and total <= i * 100
        total += scores[i]
    return total // len(scores)

def main():
    print(average([70, 85, 100]))
```

```console
$ aura verify scores.au
verified scores.au: 10 of 10 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The ten checks cover the `ensures` clause, the loop invariant, the call in
`main`, and three traps: the sum cannot overflow, `scores[i]` is in range,
and the divisor is never zero. When a check cannot be proved, the report
gives the input that breaks it. [Contracts and proofs](/book/ch11-04-contracts-and-proofs)
in the book teaches this step by step, and
[Verification](/manual/verification) lists what the verifier models.

## Built For Agents And ML Infrastructure

Serving models and running agents means working with sockets, subprocesses,
queues, deadlines, and retries. Aura's rules make the ways these can fail
visible in the code:

- **Values have owners.** A bare parameter shares, `mut` mutates, and `own`
  transfers. Cleanup follows the owning scope.
- **Promises have proofs.** `requires`, `ensures`, and invariants state what
  code relies on, and `aura verify` proves them before the program ships.
- **Failure has a type.** Recoverable failures return `Result`, `Option`, or
  an outcome enum, and the code handles them where they happen.
- **Concurrency has a scope.** A `TaskGroup` owns its child tasks. Leaving the
  scope joins them, cancels stragglers, and loses nothing.
- **The standard library covers infrastructure.** Files, processes, TCP,
  HTTP, WebSockets, TLS, JSON, regular expressions, queues, retries, and
  supervisors follow the same ownership and failure rules as the rest of the
  language.
- **Numbers run where the hardware is.** `Array` and the `simd` module
  compute on the CPU, and the `tensor` module runs the same program on the
  CPU, on the Metal GPU of a Mac, or on an NVIDIA GPU.

## Long-Term Direction

Aura aims to become a general-purpose systems language that can build every
type of software. The intended scope spans applications, services, databases,
language runtimes, embedded software, operating systems, and device drivers.

Aura 0.17 lays the foundation: static typing, deterministic ownership, native
compilation, structured concurrency, typed failure, contracts with a
verifier, packages, and integrated tooling. It also has the first low-level pieces: freestanding compilation
with no runtime, memory access at an address, a C foreign function interface,
and builds for other Linux machines. Later releases will add heap values in
freestanding code, inline assembly, portable layout controls, and
specialized runtime profiles.

<AgentDocs />

## Start Building

[Install Aura](/install/), then run a file or build a native executable:

```bash
aura run program.au
aura verify program.au
aura build -o ./program program.au
```

[The Aura Programming Language](/book/) is the book. It starts with a
first program and works up to tasks, typed failures, and I/O, building real
tools along the way. [The Manual](/manual/) is the normative reference. It
covers the grammar, ownership rules, execution model, APIs, diagnostics, and
limits.
