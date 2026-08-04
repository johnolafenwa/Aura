---
layout: home

hero:
  name: Aura
  text: Simple, safe systems programming.
  tagline: A compiled systems language for agents and frontier ML systems, with Python-like syntax, Rust-like ownership, and Go-like task-based concurrency.
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
  - title: Familiar Python-Like Syntax
    details: Readable, indentation-based syntax makes systems code easier to learn and maintain. Static typing still catches mistakes before a program runs.
  - title: Safe Rust-Like Ownership
    details: Deterministic ownership controls access, transfer, and cleanup. Aura compiles native executables with no garbage collector.
  - title: Simple Go-Like Tasks
    details: Go-like tasks provide a direct surface for structured concurrency. Task groups account for child work, cancellation, and results.
---

## Systems Programming For Everyone

Aura exists to democratize systems programming. It combines the readability of
Python-like code with ownership-based safety and task-based concurrency. The
result is **Python-like code with Rust-style safety.** Go-like tasks give the
same language a direct concurrency model for ML systems and reliable agents.

Aura is designed to be a **simple and safe systems language** for building
**agents and frontier ML systems**. It combines **Python-like syntax**,
**Rust-like ownership**, and **Go-like task-based concurrency** in one focused
language.

<AgentDocs />

Aura is a **compiled systems language**. Every program is **statically typed**.
Values follow a **deterministic ownership** model, resources have defined
cleanup points, and the runtime has **no garbage collector**. These guarantees
make failures, lifetimes, and concurrency visible while the code remains
familiar to Python developers.

## Aura, Python, And Rust At A Glance

| | Python | Rust | Aura |
| --- | --- | --- | --- |
| Syntax | Familiar, concise, indentation-based | Explicit systems syntax | **Python-like and indentation-based** |
| Type system | Dynamic, with optional annotations | Static | **Static with local inference** |
| Execution | Commonly bytecode on an interpreter | Compiled native executables | **Compiled native executables** |
| Memory management | Reference counting and cyclic garbage collection | Ownership and borrowing, no garbage collector | **Deterministic ownership, no garbage collector** |
| Failure model | Exceptions | `Result`, `Option`, and panics | **Typed `Result`, `Option`, outcome enums, and source diagnostics** |
| Concurrency | Threads, processes, and async frameworks | Threads and an async ecosystem | **Scoped `TaskGroup` children and owned value transfer** |
| Primary strength | Fast development and a vast ecosystem | Maximum control, performance, and mature systems tooling | **Safe infrastructure for agents and frontier ML systems** |

Python makes programs easy to read and write. Rust sets a high standard for
ownership-based memory safety. Go makes concurrent service software direct.
Aura brings those strengths together for safe systems programming with typed
operational failure.

## A Language For Agents And Frontier ML Systems

The work around a model is systems work. Inference services manage sockets,
files, processes, queues, deadlines, and shared compute. Agents call tools,
retry remote operations, supervise workers, and recover from failure. A small
mistake in any of those paths can become a stalled service, a leaked resource,
or a lost task.

Aura makes those responsibilities explicit:

1. **Values have owners.** Bare parameters grant shared access, `mut` grants
   exclusive mutation, and `own` transfers a value. Cleanup follows the owning
   scope.
2. **Failure has a type.** Operations return `Result`, `Option`, or focused
   outcome enums, so callers handle each recoverable failure at the relevant
   line.
3. **Concurrency has a scope.** A `TaskGroup` owns its children. Leaving the
   scope waits for them, cancels unfinished work, and accounts for every
   result.
4. **Infrastructure is part of the language surface.** Files, subprocesses,
   TCP, HTTP, WebSockets, TLS, queues, retries, and supervisors share the same
   ownership and failure rules.

This foundation is designed for frontier model-serving infrastructure, agent
runtimes, data and evaluation workers, tool execution, and the control planes
that connect them.

## A First Program

```aura
class Job:
    id: int32
    label: str

def render(job: Job) -> str:
    return f"#{job.id} {job.label}"

jobs = [Job(id=1, label="embed"), Job(id=2, label="infer")]

for job in jobs:
    print(render(job))
```

The `Job` fields are statically typed. The bare `job: Job` parameter grants
shared access, so `render` can read the job while the caller keeps ownership.
The loop also reads the list through shared access. The compiler checks these
rules before execution.

## Start Building

[Learn Aura](/learn/) begins with runnable scripts and grows into programs with
domain types, owned resources, child tasks, typed failures, and I/O.

[The Manual](/manual/) is the normative language and standard-library
reference. It covers the complete grammar, type and ownership rules, execution
model, maintained APIs, diagnostics, limits, and conformance contract.

Once Aura is installed, run a source file or build a native executable:

```bash
aura run program.au
aura build -o ./program program.au
```
