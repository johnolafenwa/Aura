# Batch 9 Design Checkpoint: Iterators And Generators

Status: **Proposed 2026-11-01.** The defaults below are recommendations for
the owner to ratify. Implementation starts on the defaults, since each step
is reversible until a release ships it.

This checkpoint settles the details that
[ADR-0054](decisions/0054-generators-and-iterator-protocol.md) left open.
The ADR's ratified direction stays binding: lazy generators, an item/end
result distinct from every item, one associated-type iterator model,
explicit idempotent `close`, exact early-exit cleanup, persistent failure,
and live frames that start out pinned.

## Starting Point

- Iteration today is compiler-known: `for` and comprehensions accept lists,
  dicts, sets, spans, `range`, `enumerate`, `zip`, and `Queue` receive
  iteration. Each form has its own shared,
  mutable, consuming, or value contract.
- There is no user-defined iteration. A class cannot be the source of a
  `for` loop, and nothing lazy exists: every comprehension builds its whole
  result.
- Traits are nominal, generic (`trait Shape[T]`), and dispatched statically
  through `impl`. There are no associated types and no trait objects.
- Generic enums exist (`Result[T, E]`), separate from anonymous unions such
  as `T | None`, and `match` handles them on both backends.
- Tasks are stackful coroutines (corosensei) on pinned workers. A task's
  stack never migrates and there is no work stealing
  ([concurrency manual](../docs/manual/concurrency.md#workers-and-scheduling)).
  Both backends run on this one scheduler; the MIR interpreter runs Aura
  calls as Rust recursion on the task's stack and probes its headroom.

## A. The Advancement Result

ADR-0054 wrote `Step[T]`, `Item(value)`, and `End` as notation.

- **A1. `enum Step[T]: Item(T), End` (default).** A builtin prelude enum,
  spelled `Step.Item(x)` and `Step.End`, matched like any enum. It is
  distinct from `T | None`, so an iterator of `int64 | None` yields
  `Step.Item(None)` without confusion.
- A2. Reuse `Result[T, Done]`. Rejected: it reads as failure and would
  collide with iterators of `Result` items (section B).
- A3. `Optional`-style `T | End`. Rejected by the ADR: it is not a tagged
  sum when `T` is itself a union containing `End`.

`Step` becomes a reserved builtin type name (`AU2002` for a user type of
that name, as for `Result`).

## B. Recoverable Stream Errors

- **B1. Errors are items (default).** A fallible stream is an iterator of
  `Result[T, E]`. The producer decides whether to continue after an `Err`.
  `for` does not propagate errors on its own; the body uses `try` or
  `match` on each item. No error ever becomes `Step.End`.
- B2. A fallible `next(mut self) -> Result[Step[T], E]`. Rejected for now:
  it doubles the protocol, and every adapter would need two forms.

## C. Failure, Cancellation, And Observation

A trap in a generator body is a trap of the task that called `next`. Aura
traps stop the task, and a generator cannot leave its task (section F), so
no code can observe the generator after it failed.

- **C1 (default).** The trap propagates out of `next` with a call chain
  that shows both the `next` caller and the generator body frame. The
  generator's frame unwinds exactly once, with the task. The ADR's
  "persistent failure" holds trivially: the failed generator is unreachable.
  The runtime still records a `failed` state, so a defensive resume after a
  contained fault reports the original failure instead of resuming freed
  storage.
- Cancellation of the owning task while the generator body waits works the
  same way: the task unwinds, and the generator's `with` exits run once in
  reverse order as part of that unwinding.
- If Aura later adds trap containment inside a task, the recorded `failed`
  state gives `next` a defined answer (re-report the failure); that API is
  out of scope here.

## D. Associated Types

`IntoIterator` needs to name the iterator a source produces.

- **D1 (default).** A trait may declare `type Name: Bound` lines. An
  `impl` must fix each with `type Name = ConcreteType`. Inside the trait and
  its implementations the type is written `Self.Name`. Outside, a generic
  bound `S: IntoIterator[T]` exposes `S.Iter`. Resolution is static: each
  implementation fixes one type for its target specialization, and there
  are no trait objects.
- D2. Skip associated types and give `into_iter` a fixed return type
  `Generator[T]`. Simpler, but every collection-like class would pay for a
  generator frame even when a hand-written iterator class would do, and it
  contradicts the ratified "one associated-type model".

Associated types ship last (step 9e), after generators work, so the
protocol is usable before the trait machinery grows.

## E. Loop Consumption

- **E1 (default).** `for x in source` consumes `source` when it is a
  `Generator[T]`, an `Iterator[T]` implementation, or an `IntoIterator[T]`
  implementation. The loop closes the iterator on every exit: exhaustion,
  `break`, `return`, `try` propagation, trap, and cancellation.
- **E2 (default, extends the ADR).** `for x in mut it` borrows an
  `Iterator[T]` (including a generator) mutably. The loop calls `next` but
  does not close it, so code can take a header with `break` and then keep
  iterating the same iterator in a second loop. The ADR rejected modifiers
  on protocol sources; this keeps `own` rejected and allows only `mut` on
  values that already are iterators.
- E3. Bare consumption only, as the ADR wrote. Partial iteration then needs
  a hand-written `while` over `next()` and a `match`.

## F. Generator Inputs, Transfer, And Affinity

- **F1 (default, as the ADR).** A generator function's non-Copy parameters
  must be `own`; `mut` parameters are rejected; a Copy parameter is
  snapshotted. A view or capability may not be live across a `yield`.
  `Generator[T]` is non-Copy, non-cloneable, and non-Transfer, so it cannot
  cross a task start, task result, `Queue`, module state, or FFI.
- **F2 (default).** Because tasks never migrate and a generator runs only
  inside `next` on its owning task, the generator's frame moves with its
  task. If Batch 8 adds task migration, a task migrates together with every
  generator frame it owns (joint migration), instead of pinning the task.
  This removes the ADR's "first resume pins it to the worker" rule as a
  separate mechanism.

## G. Execution Strategy

- **G1. Stackful frames (default).** Each generator owns a coroutine stack
  from a per-worker pool, created on the first `next`. `next` resumes it
  synchronously on the calling task; `yield` suspends it back to `next`.
  Both backends share the runtime implementation: native code calls
  runtime helpers, and the interpreter runs the body by Rust recursion on
  the generator stack. Scheduler waits inside a generator body (a `Queue`
  receive, a sleep, file I/O) suspend the whole task with the generator
  stack live; the task's resume point is inside the generator stack, which
  the runtime records so stack probes and call chains see both stacks.
  `close` resumes the frame in an unwinding mode that runs its cleanup.
- G2. Stackless state machines in MIR. Every local live across a `yield`
  becomes a field of a heap frame and each `yield` becomes a resume point.
  Faster to create and smaller, but it must thread Aura's explicit
  retain/release, lending, and `with` cleanup through every resume point.
  Left as a later optimization for generators the compiler can prove simple
  (no `with` across a yield, scalar locals), under the ADR's fusion rules.

Default generator stack: 256 KiB reserved, committed lazily, with
`aura.generator_stack(bytes)`-style tuning deferred until measurement shows
a need. Interpreter headroom probes apply to generator stacks unchanged.

## H. Standard Adapters

- **H1 (default).** Ship the protocol, generators, and loop integration
  first. Adapters such as `map`, `filter`, `take`, and `chain` arrive as
  ordinary generator functions in the standard library once closures can be
  stored in generator frames (closures are already Transfer-checked values),
  in a later step.
- Lazy comprehensions and generator expressions stay non-goals.

## Delivery Plan

| Step | Scope | Depends on |
| --- | --- | --- |
| 9a | This checkpoint | Nothing |
| 9b | `Step[T]` builtin enum; `trait Iterator[T]` with `next(mut self) -> Step[T]`; `for` and comprehensions over user `Iterator` implementations (bare and `mut`), with close-free semantics for user iterators | 9a |
| 9c | Generator functions: `Generator[T]` result, `yield`, bare `return`, laziness, input rules, MIR `Yield`, runtime generator stacks on both backends, `next`, `close`, drop cleanup, exhausted and failed states | 9b |
| 9d | `for` over generators: consumption, close on every exit, `mut` borrowing, comprehensions, scheduler waits inside generator bodies, call chains across both stacks | 9c |
| 9e | Associated types and `trait IntoIterator[T]` with `type Iter: Iterator[T]`; `for` over `IntoIterator` sources | 9d |
| 9f | Manual page, tutorial chapter, ADR-0054 status, LSP (hover, completion, semantic tokens for `yield`), performance probes for `next` and generator creation | 9e |

Every step keeps the full local gates, coverage floors, and interpreter and
native parity. Each runtime row of ADR-0054's completion-test matrix lands
with the step that implements it.

## Diagnostics

New codes are assigned in the step that needs them and registered in
`aura-core/src/diag.rs`:

| Area | When |
| --- | --- |
| Generator declarations | `yield` outside a generator; a missing or invalid `Generator[T]` result; `return value` in a generator; a `yield` whose value is not `T` |
| Inputs and escapes | a non-Copy parameter that is not `own`; a `mut` parameter; a view live across `yield`; a generator crossing a task, `Queue`, module, or FFI boundary |
| Protocol | a `for` source with no `Iterator` or `IntoIterator` implementation, or an ambiguous one; an `impl` missing an associated type; `own` before a protocol source |
| Runtime | resuming a running generator; yielding during cleanup |

## Owner Questions

1. A1: name the result `Step[T]` with `Step.Item(x)` and `Step.End`?
2. B1: model recoverable errors as `Result` items, with no fallible `next`?
3. C1: treat a generator trap as a trap of the task that called `next`?
4. D1: add associated types as `type Name: Bound` in traits and
   `type Name = T` in impls, written `Self.Name` and `S.Name`?
5. E2: allow `for x in mut it` to iterate a borrowed iterator without
   closing it (an extension of the ADR)?
6. F2: move generator frames jointly with their task instead of pinning
   them separately?
7. G1: implement generators as stackful frames first, with stackless state
   machines as a later optimization?

## Ratification Record

Pending.
