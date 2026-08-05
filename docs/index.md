---
layout: home

hero:
  name: Aura
  text: Compiled. Statically typed. Familiar.
  tagline: Python-inspired syntax, deterministic ownership, structured concurrency, and native executables for reliable software.
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
    details: Build native executables for deployment with no language interpreter or garbage collector.
  - title: Static Types
    details: Every expression has a type. The compiler checks calls, fields, ownership, mutations, matches, and task boundaries before execution.
  - title: Ownership-Based Reliability
    details: Shared access, mutation, ownership transfer, and resource cleanup follow explicit rules enforced by the compiler.
---

## Why Aura

Aura brings familiar source code to a compiled, statically typed language. Its
indentation-based syntax is easy to read, while compiler checks cover types,
ownership, mutation, exhaustive matching, failure handling, and task
boundaries. Programs build as native executables with deterministic cleanup
and no garbage collector.

The current preview is designed for reliable applications, agent runtimes, ML
infrastructure, evaluation workers, and control-plane services.

Aura is a technical preview. The language and APIs may still change before a
stable release.

## At A Glance

| | Python | Rust | Aura |
| --- | --- | --- | --- |
| Syntax | Indentation-based and concise | Explicit and low-level | **Python-inspired and indentation-based** |
| Types | Dynamic, with optional hints | Static | **Static, with inference** |
| Execution | Interpreter and virtual machine | Native executables | **Native executables** |
| Memory | Reference counting and garbage collection | Ownership | **Ownership, no garbage collector** |
| Failure | Exceptions | `Result`, `Option`, panics | **Typed `Result`, `Option`, outcome enums** |
| Concurrency | Threads and async functions | Threads and async ecosystem | **Structured task groups across multiple cores** |
| Current focus | General-purpose applications and scripting | Systems and application software | **Reliable applications, agents, and ML infrastructure** |

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

The syntax is familiar and every operation remains statically checked. Each
signature states what it does to its arguments: `scale` asks for `mut` access
and changes the list in place, while `total` only reads it. The compiler
enforces both contracts.

## Built For Agents And ML Infrastructure

Serving models and running agents involves sockets, subprocesses, queues,
deadlines, and retries. Aura's rules make the failure modes visible:

- **Values have owners.** Bare parameters share, `mut` mutates, `own`
  transfers. Cleanup follows the owning scope.
- **Failure has a type.** Recoverable failures return `Result`, `Option`, or
  an outcome enum, handled where they happen.
- **Concurrency has a scope.** A `TaskGroup` owns its children: leaving the
  scope joins them, cancels stragglers, and loses nothing.
- **The standard library speaks infrastructure.** Files, processes, TCP,
  HTTP, WebSockets, TLS, queues, retries, and supervisors follow the same
  ownership and failure rules as everything else.

## Long-Term Direction

Aura's long-term goal is to become a general-purpose systems language capable
of building every type of software. The intended scope spans applications,
services, databases, language runtimes, embedded software, operating systems,
and device drivers.

Aura 0.3 establishes the foundation through static typing, deterministic
ownership, native compilation, structured concurrency, typed failure,
packages, and integrated tooling. Later releases will extend that foundation
with freestanding compilation, low-level memory access, hardware interfaces,
portable layout controls, cross-compilation, and specialized runtime profiles.

<AgentDocs />

## Start Building

Install Aura, then run a file or build a native executable:

```bash
aura run program.au
aura build -o ./program program.au
```

[Learn Aura](/learn/) starts with runnable scripts and works up to tasks,
typed failures, and I/O. [The Manual](/manual/) is the normative reference:
grammar, ownership rules, execution model, APIs, diagnostics, and limits.
