# Why Aura

Aura 0.3.4 is a technical preview of a compiled, statically typed language for
reliable software. It combines Python-inspired syntax, deterministic
ownership, structured concurrency, typed failure, and native executables.

The current language focuses on agents, machine learning (ML) infrastructure,
evaluation workers, network services, and the control-plane software around
models. These workloads benefit from readable source code, explicit resource
lifetimes, structured tasks, and failures that appear in the type system.

Three commitments shape the language:

- **Deterministic ownership.** Bare access is shared, `mut` is exclusive
  mutation, and `own` transfers a value. The owning scope defines cleanup.
- **Structured concurrency.** A `TaskGroup` owns every child task started in
  its scope. When the scope exits, every child is accounted for.
- **Typed failure.** Files, subprocesses, sockets, HTTP, retries, and
  supervisors report recoverable failure through `Result`, `Option`, and
  focused outcome enums.

Ownership governs access, transfer, and cleanup. It does not govern
scheduling. Aura leaves unspecified the order in which concurrent tasks
complete, how work is scheduled across workers, and the order of output. The
[Ownership](/manual/ownership-and-borrowing),
[Concurrency](/manual/concurrency), and
[Control-Plane Modules](/manual/control-plane) chapters define the exact
contracts.

## Built For Agents And ML Infrastructure

ML products contain much more than model code. They include inference
gateways, queue workers, evaluation pipelines, tool executors, subprocess
supervisors, network clients, storage paths, timeouts, and retries. Agent
runtimes add long-lived task trees that talk to unreliable external systems
again and again.

Aura gives this work one coherent contract:

- Static types describe the data.
- Ownership describes resource lifetime.
- Structured concurrency accounts for child tasks.
- Typed outcomes keep operational failure visible.
- Native compilation produces deployable executables with no garbage
  collector.

That makes Aura a focused language for the reliable control plane around
models:

- model serving and inference coordination
- agent runtimes and tool execution
- concurrent data and evaluation workers
- process, queue, and network supervision
- infrastructure where cleanup and failure handling are correctness
  requirements

## Long-Term Direction

Aura aims to become a general-purpose systems language that can build every
type of software. The intended scope spans applications, services, databases,
language runtimes, embedded software, operating systems, and device drivers.

Aura 0.3 lays the foundation: static typing, deterministic ownership, native
compilation, structured concurrency, typed failure, packages, and integrated
tooling. Later releases will add freestanding compilation, low-level memory
access, hardware interfaces, portable layout controls, cross-compilation, and
specialized runtime profiles.

## Familiar Source, Strong Guarantees

Python showed the value of readable, low-friction source code. Rust showed
that ownership can prevent broad classes of memory and concurrency errors
before a program runs. Aura combines those lessons in an indentation-based
language with a narrower focus on control-plane software.

The familiar syntax makes compiled software cheaper to read and write. It does
not weaken the language contract. The compiler requires exact types at public
boundaries, validates ownership and task transfer, and checks that matches are
exhaustive. Runtime diagnostics carry source context.

## Adjacent Languages

These projects share parts of Aura's motivation. The comparisons below cover
focus and language contracts. The primary sources were checked on 31 July
2026.

### Mojo

Mojo is a close neighbor: it has Python-shaped compiled syntax and
compiler-tracked ownership. Its roadmap centers on
[high-performance kernels on CPUs, GPUs, and ASICs, with Python interoperability](https://mojolang.org/docs/roadmap/).
Its ownership documentation gives each value one owner and defines
[default immutable, `mut`, and `var` argument conventions](https://mojolang.org/docs/manual/values/ownership/).

Aura 0.3 centers on the application control plane around models and agents.
That covers scoped child tasks, transferable messages, typed I/O and process
failures, timeouts, retries, and supervision. GPU programming, heterogeneous
hardware, and interoperability with Python libraries are future areas for
Aura.

### Nim

Nim is a broad, established systems language. The Nim project describes it as
a [statically typed compiled language combining ideas from Python, Ada, and
Modula](https://nim-lang.org/). It produces native executables and uses
deterministic, customizable memory management. Its documentation recommends
[ORC for newly written code](https://nim-lang.org/2.2.6/mm.html). Its
[typed-threads documentation](https://nim-lang.org/docs/typedthreads.html)
covers shared-heap and explicit thread facilities.

Aura offers a smaller, integrated contract. It has capabilities declared at
each call boundary, task values that are structurally transferable, `TaskGroup`
scope, and typed control-plane APIs. Nim offers more metaprogramming, more
backends, a larger ecosystem, and wider portability today.

### Go

Go is a production reference point for simple concurrent service software.
Its documentation defines lightweight
[goroutines and channel communication](https://go.dev/doc/effective_go#concurrency)
and treats [errors as values](https://go.dev/blog/errors-are-values). It also
explains that the standard toolchain ships a
[tracing garbage collector](https://go.dev/doc/gc-guide).

Aura uses a different lifetime contract. Non-copy task captures and messages
must satisfy structural `Transfer`. Resources have owners. A `TaskGroup`
accounts for the children it starts. Together, these give scoped task
lifetimes and deterministic resource cleanup without a garbage collector.

### Free-threaded Python 3.13+

CPython has offered an optional free-threaded build since Python 3.13. The
official guide says this build can run threads in parallel with the global
interpreter lock (GIL) disabled. Some extension modules may
[re-enable the GIL](https://docs.python.org/3/howto/free-threading-python.html).
The language keeps its shared-object, dynamically typed programming model.

Aura checks ownership and task-transfer boundaries before a program runs. It
gives common control-plane failures concrete result types. Python offers much
more runtime flexibility and ecosystem compatibility. Aura offers a compiled,
ownership-based contract for teams that want those decisions checked.

## Performance And Technical Scope

The [Performance](/manual/performance) chapter records current measurements,
known gaps, the evidence to reproduce them, and the optimization direction for
later releases.

Aura 0.3 is an executable technical preview. It includes the language, the
native compiler, the ownership model, the structured task runtime, numeric
arrays, control-plane modules, package tooling, the Manual, and the editor
extension. The [Current Limits](/manual/current-limits) chapter lists the
exact boundaries of that surface.
