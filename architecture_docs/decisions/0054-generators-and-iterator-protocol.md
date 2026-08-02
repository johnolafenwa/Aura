# ADR-0054: Generators and the iterator protocol

- Status: Proposed
- Date: 2026-08-02
- Version target: Aura 0.4
- Implementation: Not started
- Roadmap decision: Batch S1, design-only checkpoint
- Related: ADR-0006, ADR-0013, ADR-0016, ADR-0017, ADR-0019, ADR-0022,
  ADR-0032, ADR-0033, ADR-0038, ADR-0039, and ADR-0052

## Decision boundary

This ADR is a proposed language and runtime design. `yield`, `Generator[T]`,
`Iterator[T]`, `IntoIterator[T]`, and protocol-based loop selection are not
implemented. Implementation requires separate authorization and a ratified
answer to the questions at the end.

## Context

Aura has eager collection comprehensions and compiler-known iteration for its
builtin collections, ranges, and queues. Data pipelines, parsers, paginated
agent tools, and model-stream transforms also need a lazy producer that keeps
local state between values without allocating an entire result collection.

Aura's runtime already executes stackful resumable tasks. A generator can use
the same guarded-stack and suspension machinery, provided its frame remains on
one worker, ownership across each suspension is explicit, and abandoning an
iteration performs exact cleanup. The language also needs one typed iterator
protocol so loops and comprehensions can consume library-defined producers.

## Goals

- define a lazy `Generator[T]` with one owned yielded item per suspension
- define typed `Iterator[T]` and `IntoIterator[T]` protocols
- preserve deterministic argument evaluation, ownership, cleanup, and traps
- let statement loops and eager comprehensions consume protocol iterators
- retain the specialized collection, range, and Queue contracts
- permit backend optimization and iterator fusion only when behavior is exact

## Non-goals

- generator expressions or lazy comprehensions
- bidirectional `send`, `throw`, or return-value extraction
- `yield from` delegation
- asynchronous-generator syntax
- multi-shot, rewindable, Copy, cloneable, or transferable generators
- cross-worker resumption or work stealing of a live generator frame
- views yielded across a suspension in the first implementation
- exposing stack pointers, frame addresses, or resumption handles through FFI

## Generator declarations

A function whose declared result is `Generator[T]` is a generator function and
may contain `yield` statements:

```aura
def read_pages(client: own Client, first: str) -> Generator[Page]:
    mut cursor = first
    while cursor != "":
        page = client.fetch(cursor)
        cursor = page.next_cursor.clone()
        yield own page
```

`Generator[T]` requires one complete owned element type `T`. The body may use
ordinary statements, loops, match, resources, local functions available to an
ordinary Aura function, and bare `return` to end iteration. `return value` is
rejected. Falling off the body is equivalent to bare `return`.

A function not declared as returning `Generator[T]` cannot contain `yield`.
A generator body cannot infer `T` from its yields; the declaration is the
single element-type contract. Every reachable yield must check as exactly
`T` under ordinary contextual literal typing.

The first implementation rejects view types as `T` and rejects yielding any
active view or capability. It also rejects a generator declaration that uses a
mutable parameter. Every non-Copy parameter must be written `own`; a bare Copy
parameter is snapshotted into the frame, and an `own` parameter is moved into
it. This ensures every value retained after the call belongs to the generator
frame.

## Call and suspension semantics

Calling a generator is lazy with respect to its body:

1. evaluate call arguments once from left to right
2. apply defaults in their ordinary declaration order
3. copy or move each argument into a fresh suspended frame
4. return the owned `Generator[T]` without executing the body

The first `next` resumes at the body entry. Each later `next` resumes after the
last completed `yield`. Local bindings, control-flow position, active
resources, and owned temporaries that are live across the suspension remain in
the frame.

`yield expression` evaluates the expression exactly once. It copies a Copy
`T` or moves an owned non-Copy `T` into the yielded result, then suspends. A
shared non-Copy value must be converted to an independently owned value by an
operation already valid for its type. No hidden clone occurs.

After bare return or body fallthrough, the generator is exhausted. Every
subsequent `next` returns `None` without resuming the frame. A trap propagates
to the caller of `next`, unwinds the generator frame exactly once, marks it
closed, and makes later `next` calls return `None`. Cancellation follows the
same cleanup path and retains its ordinary cancellation result at the
resumption boundary.

Dropping or explicitly closing a suspended generator unwinds every active
`with` resource and owned local in reverse nesting order exactly once. The
builtin operation is:

```aura
generator.close(mut self) -> None
```

Closing an exhausted or already closed generator is idempotent. Closing while
the generator is actively executing is rejected as an overlapping mutable
access. Cleanup code may not yield; a yield attempted while unwinding is a
runtime fault contained at the generator boundary.

## Generator value properties

`Generator[T]` is always non-Copy, non-cloneable, and non-Transfer, regardless
of `T` or the captured frame values. Assignment moves it. It may live and move
within one task, but its first resume pins it to that task's current worker for
the rest of its lifetime. A later resume from another task or worker is
statically rejected when visible and defensively rejected by the runtime.

The generator owns its frame and all stored inputs. It cannot capture a bare
or mutable caller capability. Views created wholly inside the generator may
survive an ordinary runtime wait when their owner remains in the same pinned
frame and the loan design permits that suspension. They must end before a
`yield`; yielded values and caller code never observe the view.

A generator cannot cross a task start, task result, Queue payload, supervisor,
detached-work, module-state, or FFI boundary. A containing aggregate is also
non-Transfer by the ordinary structural path rule.

## Iterator protocol

The protocol has one item type and one mutation-based advance operation:

```aura
trait Iterator[T]:
    def next(mut self) -> T | None
```

`next` mutates the iterator state and returns one owned `T`, or `None` after
exhaustion. Implementations must remain exhausted: once `next` returns `None`,
later calls also return `None`. A trap may close the iterator; it cannot be
translated to end-of-iteration unless the implementation handles the failure
before returning.

`Generator[T]` has a compiler-provided `Iterator[T]` implementation. Its
`next` is the resumption operation above.

Types that produce a distinct iterator implement:

```aura
trait IntoIterator[T]:
    type Iter: Iterator[T]
    def into_iter(own self) -> Iter
```

`Iter` is a required associated type scoped to this protocol. Authorizing this
ADR therefore authorizes the minimum associated-type feature needed to name
the concrete returned iterator; it does not add trait objects, dynamic
dispatch, associated constants, or general higher-kinded types. Every
implementation fixes one unambiguous `T` and `Iter` for its target
specialization. Overlapping implementations use the ordinary unique-most-
specific rule.

`into_iter` consumes the source and returns an owned iterator. A type that
offers shared iteration can expose an ordinary `iter(self) -> Generator[T]`
method whose returned generator owns any required snapshot or whose frame uses
an authorized in-loan relation. The first protocol implementation does not
infer a hidden shared capability.

## Loop and comprehension integration

For a `Generator[T]` expression, bare iteration moves the generator into a
hidden mutable loop-local and repeatedly calls `Iterator.next(mut iterator)`:

```aura
for item in read_pages(own client, first):
    consume(own item)
```

A named non-Copy generator is consumed by the loop and cannot be used
afterward. `break`, `return`, propagated error, trap, and cancellation close
the hidden iterator and run its cleanup exactly once. Exhaustion also closes
it. `continue` keeps the iterator alive and requests the next item.

For another source with a unique `IntoIterator[T]` implementation, bare
iteration calls `into_iter(own source)` once, stores its concrete `Iter`, and
then follows the same `next` loop. The source expression is evaluated once.
Protocol selection and associated-type resolution happen statically; no
runtime trait object is created.

Eager list, set, and dictionary comprehensions use the same selection and
advance sequence for protocol sources. They still allocate their result
eagerly and close the iterator on every exit. A generator is not created merely
because a comprehension consumes an iterator.

Builtin list, dictionary, set, range, enumerate, and zip iteration retain
their compiler-defined shared, mutable, consuming, or value-yielding
contracts. They need not allocate a protocol adapter. Queue iteration retains
its receive contract: bare iteration copies the Queue handle and receives
already-owned values until its defined termination. It does not consume the
Queue through `IntoIterator`, and its explicit ownership modifiers remain
invalid.

Protocol-based iteration uses the bare form in the first implementation.
Explicit `mut` or `own` before a protocol source is rejected because the
protocol already consumes the source into a private mutable iterator. APIs
needing a different capability expose a method that returns a generator or an
owned iterator explicitly.

## Fusion and optimization

The semantic model is allocation of the concrete iterator followed by ordered
`next` calls and exact cleanup. A backend may inline, stack-allocate, or fuse a
generator and its consumer only when it preserves:

- argument and source evaluation counts and order
- every visible side effect before and between yields
- one-at-a-time ownership transfer of yielded items
- short-circuit behavior of filters and nested comprehensions
- trap, cancellation, break, and resource-cleanup order
- scheduler safepoints and fairness obligations
- diagnostic and stack-frame attribution

Fusion may not retain a yielded item past the point where the unfused consumer
would drop or move it. Debug and forced-unfused modes must remain available to
the parity suite. Optimization must never be required for bounded memory: an
unfused generator already retains only its live frame plus the current item.

## Backend contract

MIR and direct execution use one logical generator state machine: new,
suspended, running, exhausted, or closed. A resume transitions suspended to
running, and only yield transitions running back to suspended. Recursive or
concurrent resume while running is a runtime fault.

The implementation may reuse the stackful task substrate, but a generator is
not scheduled independently. It runs synchronously inside `next` until yield,
completion, trap, cancellation, or an ordinary scheduler-aware operation.
Frame allocation uses the guarded-stack policy and contributes to the same
resource limits and diagnostics. Stack traces include both the caller of
`next` and the generator declaration/resumption frame.

Checked interfaces encode generator signatures, iterator implementations,
associated `Iter` types, and non-Transfer status. No generator frame or
protocol object crosses the native C ABI.

## Diagnostics

Dedicated diagnostics must cover:

- `yield` outside a declared generator and a generator body with missing or
  invalid `Generator[T]` result
- a yield whose value does not have owned type `T`
- `return value`, `yield from`, `send`, and every unsupported generator form
- a non-Copy parameter that is not `own`, any mutable parameter, and a view or
  capability retained across yield
- use after a generator is moved into a loop
- task, Queue, module, detached, supervisor, or FFI escape with the frame path
- ambiguous or missing `IntoIterator` / `Iterator` implementations and an
  invalid associated `Iter`
- forbidden protocol iteration modifiers
- recursive/concurrent resume, wrong-worker resume, and yield during cleanup
- stack-limit failure with the generator declaration in the Aura call chain

Diagnostics distinguish source-ownership failures from protocol lookup and
runtime frame failures. No diagnostic suggests collecting the complete stream
unless that is semantically safe and explicitly requested by the programmer.

## Consequences

Aura gains lazy, bounded producers with explicit ownership and deterministic
cleanup. The worker-affine rule lets the implementation reuse stackful runtime
machinery without making suspended stacks transferable.

The protocol is deliberately consuming. Shared collection traversal remains a
specialized place operation, while user types can choose an explicit method
that constructs an owned iterator. The associated type is a real dependency
that must be implemented and documented with the protocol.

## Completion-test matrix

- lexer/parser: generator declarations, yield precedence, multiline yield,
  bare return, forbidden return values, and all unsupported forms
- static typing: explicit `T`, every yield checked, no inference, Copy and own
  parameters, mutable/capability rejection, and view-at-yield containment
- lazy calls: argument/default evaluation once and in order, zero body effects
  before first next, and frame-allocation failure cleanup
- suspension: locals and control flow retained across many yields, nested
  loops/matches/resources, and exact item order
- ownership: Copy yields, non-Copy moves, shared-source rejection, moved
  generator use, active-item cleanup, and nested aggregate paths
- termination: fallthrough, bare return, repeated `None`, explicit close,
  drop, break, continue, caller return, propagated error, trap, and cancellation
- resources: reverse-order exact-once cleanup from every suspension point,
  failed cleanup, close idempotence, and yield-during-cleanup rejection
- properties: Generator is always non-Copy, non-cloneable, non-Transfer, and
  worker-affine for every `T`
- protocol: direct Generator iteration, user `Iterator`, `IntoIterator` with
  concrete associated type, overlap/ambiguity, exhaustion stability, and
  missing implementation diagnostics
- integration: statement loops, eager list/set/dictionary comprehensions,
  nested protocol iterables, source evaluation once, and result cleanup
- builtins: list/dict/set/range/enumerate/zip behavior unchanged and Queue
  receive iteration retains its exact ownership and termination contract
- scheduler: waits inside generators, safepoints, cancellation wakeup, stack
  limits, no independent scheduling, and wrong-worker defense
- optimization: fused/unfused behavioral equality for maps, filters, nested
  loops, early exit, traps, and owned drop timing
- interfaces/tooling: import metadata, associated-type hover/definition,
  completion, formatting, semantic tokens, stack traces, examples, tutorials,
  and reference integrity
- parity: byte-identical MIR/direct results, diagnostics, cleanup traces, and
  forced-backend execution for every runtime row

## Ratification questions

1. Ratify lazy call semantics with eager argument capture and no body execution
   before the first `next`?
2. Ratify owned yields, bare-return-only completion, and the exclusion of
   `send`, `yield from`, and generator return values?
3. Ratify Generator as unconditionally non-Copy, non-cloneable, non-Transfer,
   and worker-affine?
4. Ratify `Iterator.next(mut self) -> T | None` and the minimal associated
   `IntoIterator.Iter` type dependency?
5. Ratify consuming bare protocol iteration while builtin place iteration and
   Queue receive iteration retain their specialized contracts?
6. Should a trap permanently close the generator and make later `next` return
   `None`, or should every later access report the original failure state?
7. Is explicit idempotent `close(mut self)` part of the first surface, or is
   deterministic drop sufficient?
