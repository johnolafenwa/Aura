---
layout: home

hero:
  name: Aura
  text: Simple, safe systems programming.
  tagline: A compiled language with Python's syntax, Rust's ownership, and Go's concurrency — built for agents and ML infrastructure.
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
  - title: Reads Like Python
    details: Indentation, f-strings, comprehensions, keyword arguments. Static types catch mistakes before anything runs.
  - title: Owns Like Rust
    details: Every value has one owner. Access is shared, mut, or own — checked at compile time, cleaned up deterministically, no garbage collector.
  - title: Runs Like Go
    details: Lightweight tasks on a multi-core scheduler. A TaskGroup joins, cancels, and accounts for every child. No async/await coloring.
---

## Why Aura

Python is easy to write. Rust is safe to run. Aura is both: code that looks
like Python, checked by a compiler that thinks like Rust, on a runtime that
schedules like Go.

Aura is a technical preview. The language and APIs may still change before a
stable release.

## At A Glance

| | Python | Rust | Aura |
| --- | --- | --- | --- |
| Syntax | Indentation-based, concise | Explicit systems syntax | **Indentation-based, concise** |
| Types | Dynamic, optional hints | Static | **Static, with inference** |
| Execution | Interpreter | Native executables | **Native executables** |
| Memory | Reference counting + GC | Ownership, no GC | **Ownership, no GC** |
| Failure | Exceptions | `Result`, `Option`, panics | **Typed `Result`, `Option`, outcome enums** |
| Concurrency | Threads and async, serialized by the GIL | Threads + async ecosystem | **Structured task groups on every core, no function coloring** |
| Built for | Everything, fast | Maximum control | **Agents and ML infrastructure** |

## A First Program

```aura
class Tracker:
    counts: dict[str, int64]

    def record(mut self, event: own str):
        if event in self.counts:
            self.counts[event] += 1
        else:
            self.counts[event] = 1

mut tracker = Tracker(counts={"boot": 1})
tracker.record("request")
tracker.record("request")

match tracker.counts.get("request"):
    case Option.Some(n):
        print(f"request seen {n} times")
    case Option.None:
        print("no requests")
```

It reads like Python, and the compiler sees more: `record` must declare
`mut self` to mutate, the dict takes ownership of its keys, and the `match`
must handle the missing case. All of it is checked before the program runs.

## Built For Agent Infrastructure

Serving models and running agents is systems work — sockets, subprocesses,
queues, deadlines, retries. Aura's rules make the failure modes visible:

- **Values have owners.** Bare parameters share, `mut` mutates, `own`
  transfers. Cleanup follows the owning scope.
- **Failure has a type.** Recoverable failures return `Result`, `Option`, or
  an outcome enum, handled where they happen.
- **Concurrency has a scope.** A `TaskGroup` owns its children: leaving the
  scope joins them, cancels stragglers, and loses nothing.
- **The standard library speaks infrastructure.** Files, processes, TCP,
  HTTP, WebSockets, TLS, queues, retries, and supervisors follow the same
  ownership and failure rules as everything else.

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
