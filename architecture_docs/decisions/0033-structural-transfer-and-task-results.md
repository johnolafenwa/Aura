# ADR-0033: Structural Transfer and task-result consumption

- Status: Provisional
- Date: 2026-07-28
- Roadmap decision: Batch 4, Phase 5.6
- Amends: ADR-0008 and ADR-0020's task/queue clone-barrier availability

## Context

Aurora's Phase 5.7 pinned-worker runtime will be allowed to run sibling tasks
on different host threads. Values captured by a child or returned through its
task handle can therefore cross a worker boundary. Move checking alone is not
enough to make that safe: a uniquely owned value can still contain
thread-affine host state, and a shared or mutable capability is an alias rather
than an owned message.

The boundary must be checked before a second Aurora worker exists. It must also
compose through user aggregates and generics without asking users to implement
an unsafe marker trait.

ADR-0008 separately requires task results that are not ordinary repeatable
data to have one consumer. The current runtime can clone some stored results
for every observation, but that implementation behavior is not a sound static
contract for non-copy owned data and cannot be used for exclusive state.

## Provisional decision

### `Transfer` is compiler-derived

`Transfer` is a structural property of a resolved Aurora type. It is not a
builtin user-declarable trait and cannot be asserted with an annotation or
unsafe escape hatch in this batch. Source code may independently declare an
ordinary trait also named `Transfer` and write implementations of that user
trait, but neither the spelling nor such an implementation affects the
compiler-derived property. The checker derives the property from the complete
specialized type.

The following types are `Transfer`:

- every copy type
- `String`
- `Vec[T]` and `Set[T]` when `T` is `Transfer`
- `Map[K, V]` when both `K` and `V` are `Transfer`
- a tuple when every element is `Transfer`
- a user class when every stored field is `Transfer`
- a user or builtin data enum when every payload in every variant is
  `Transfer`; this includes structural `Option`, `Result`, queue/task outcome,
  error, JSON-data, and similar value wrappers
- `Queue[T]` and `Task[T]` handles, independently of the stored payload,
  because transferring a handle transfers only its handle identity rather than
  the payload itself; constructing a `Queue[T]` or sending through
  `put`/`try_put` separately requires `T: Transfer`. Phase 5.7 must make the
  referenced runtime state cross-worker thread-safe before a handle can
  actually be used by different workers.

Structural derivation follows nested fields and payloads and reports the first
non-`Transfer` leaf together with its containing path. Recursive class and
enum definitions are checked by type-dependency component: a recursive
component is `Transfer` when every non-recursive stored edge in that component
is `Transfer`. In Phase 5.6, an unresolved type parameter does not prove
`Transfer` and is rejected at a task or Queue boundary. Unlike clone-safety,
this batch does not infer and export a deferred Transfer obligation from a
generic definition. A caller may use a generic helper outside such a boundary,
or start a generic target after call inference has produced complete concrete
types.

These categories are not `Transfer` unless a later decision proves a specific
type thread-safe and adds it to the compiler-owned whitelist:

- a shared or mutable capability view, including a capability to an otherwise
  `Transfer` value
- `random.Rng`
- `TaskGroup`
- live filesystem, process, pipe, supervisor, listener, socket, stream, HTTP
  exchange, WebSocket, TLS, and other host-resource values

Owned data returned by an API is not made non-`Transfer` merely because it came
from a host operation. For example, an error or completed-output value is
classified from its stored data. The exclusion applies to live host authority
or thread-affine state. In particular, `process.Completed`,
`net.HttpResponse`, and `net.UdpDatagram` are owned snapshot data and are
`Transfer`; `process.Child`, `net.HttpExchange`, and `net.UdpSocket` retain
live host authority and are not.

The capability exclusion does not reject an owned Copy snapshot. If task-start
argument evaluation reads a Copy value through shared or mutable access, the
captured value is an independent owned copy and may cross when its type is
`Transfer`. A non-copy access cannot use that materialization: capturing it by
value requires ownership, and transporting the capability itself is forbidden.

### Task boundaries

Every argument captured by `TaskGroup.start`, `start_soon`,
`start_with_stack`, or `start_soon_with_stack` must have a `Transfer` type
after generic specialization. The check applies to the owned captured value,
not to the target parameter's spelling. A bare target parameter may still
borrow from the child's owned capture for the duration of the call; an `own`
parameter may consume it. A `mut` target remains rejected by the existing
no-writeback rule.

The target's resolved return type must also be `Transfer`, including for a
handle-free `start_soon` call. A generic target may be started only when call
inference has already produced complete concrete capture and result types. An
unresolved type parameter is rejected rather than recorded as a future
obligation. Rejecting the call before scheduling guarantees that no task
begins with a partially validated boundary.

Task-target resolution accepts explicit callable specialization in the narrow
forms `function[Types]` and `Type.associated_method[Types]`. The bracketed form
is reinterpreted only where a TaskGroup start method expects its callable
target; ordinary indexing elsewhere is unchanged. A target whose defaults and
declared return provide complete concrete context may also be passed bare.

Every value admitted to a Queue must be `Transfer`. The requirement is checked
by `Queue[T](...)`, `put`, and `try_put`. `Queue[T]` with unresolved `T` is
rejected at those boundaries in Phase 5.6 rather than creating a deferred
contract. Copying a Queue handle and calling handle-only operations such as
`get`, `get_or_none`, `get_or`, or `close` do not themselves inspect `T` or
recheck Transfer. This deliberate conservative Phase-1 policy prevents Aurora
from creating or populating an unsafe Queue while leaving the handle rule
sound for any future foreign Queue source.

A boundary diagnostic must explain the complete reason, not merely say that a
type lacks `Transfer`. It names whether the failed boundary is a captured
argument or task result and gives a nested path such as:

```text
this value cannot cross a task boundary because
`Job.source` contains non-Transfer `fs.File`
```

For a non-copy capability view, the guidance says to pass owned `Transfer` data
rather than an alias. It may note that Copy access materializes an owned
snapshot. For a host resource or `random.Rng`, it says to keep the value on its
owning task and send transferable input or output data instead.
Diagnostics do not suggest a user `Transfer` implementation because no such
surface exists.

These boundary failures use `AU3008`. They are distinct from `AU3009`, which
rejects an operation such as `clone`, collection `get`, or an implicit
container copy that would duplicate a single-consumer task-result right. Once
a non-repeatable task handle has been consumed by an observation, using that
same binding again is the ordinary moved-value diagnostic `AU3001`. Attempting
to consume through shared access is the existing borrow violation `AU3002`.

### Repeatable and single-consumer results

ADR-0008's static rule is enforced at the same boundary:

- a result `T` is **repeatable** when `T` is copyable, when `T` is a
  `Queue[...]` handle, or when `T` is `Task[U]` and `U` is itself repeatable
- every other `Transfer` result, including `String`, `Vec`, `Map`, `Set`, and
  non-copy structural classes or enums, is **single-consumer**

`Task[T]` is always `Transfer`, but it is copyable only when `T` is repeatable
under that recursive rule. In particular, `Task[Task[String]]` is not copyable:
the outer result cannot be copied to manufacture two handles carrying the same
single-consumer right.

All task-result observation surfaces participate in one rule. For a
non-repeatable `T`, `result`, `result_or_none`, and `result_or` consume the
unique observation right when called. The consumption is conservative:
timeout, cancellation, task failure, and the collapsed `None` from
`result_or_none` do not restore that right. `wait_any` and `wait_all` consume
the complete `Vec[Task[T]]`; for `wait_any`, observation rights belonging to
unchosen tasks are deliberately abandoned. A caller that needs retries or
preservation after timeout must arrange a repeatable result or a separate
Queue protocol rather than aliasing a single-consumer handle.

The runtime retains an atomic one-winner claim on every non-repeatable stored
result as defense in depth against backend defects or foreign handles. A
failed second runtime claim cannot return or clone the value and traps with
`AU4001`: `task result has already been observed; non-repeatable task results
allow exactly one observing attempt`. This fallback does not replace static
`AU3009`/`AU3001` enforcement.

Joining or abandoning children during `TaskGroup` cleanup is not a
source-level result observation and does not claim or duplicate a child's
result right. Cleanup may account for failure and release runtime state, but it
does not make a successful value available to another observer.

### Runtime phase boundary

Phase 5.6 establishes the static contract while Aurora task execution remains
cooperative and single-threaded. It does not make the current request broker,
task state, queue internals, or task-handle internals thread-safe. Phase 5.7
must implement and prove those synchronization changes before enabling more
than one Aurora worker. Passing Phase 5.6 therefore does not itself constitute
a multicore claim.

## Consequences

Ordinary owned data can move into and out of tasks without user ceremony, and
the rule composes through user models. Thread-affine resources remain local to
the task that owns them; programs communicate transferable descriptions,
bytes, results, or handle identities instead. Phase 5.7 supplies the
cross-worker synchronization behind those handles.

Changing a nested field can change the `Transfer` status of every aggregate
that contains it. Phase 5.6 conservatively rejects an unresolved generic type
at a task or Queue boundary instead of silently assuming that it is safe.

Non-copy task results have at most one delivered value even when they are
clone-safe. Conservative observation consumption can result in zero delivery.
Programs that need fan-out place data in an explicit message-passing
abstraction or arrange for the consuming task to publish separate owned
messages.

## Completion-test matrix

| Contract | Required evidence |
| --- | --- |
| Positive leaves | Every copy scalar/category and `String` crosses task-start and task-result boundaries on MIR and direct backends. |
| Recursive positives | Nested `Vec`, `Map`, `Set`, tuple, class, enum, `Option`, `Result`, and recursive user data pass exactly when every stored component is `Transfer`. |
| Transferable handle identity | `Queue[T]` and `Task[T]` handles cross independently of `T` without inspecting, cloning, or transferring the stored payload; Phase 5.7 must make the referenced runtime state cross-worker thread-safe before multicore use. |
| Queue payloads | Construction plus `put`/`try_put` require `T: Transfer`; handle copies and handle-only `get`/fallback/`close` operations do not recheck `T`. Nested negative payloads report the complete reason with `AU3008`. |
| Negative leaves | Shared/mutable capabilities, `random.Rng`, `TaskGroup`, and representative filesystem, process, network, HTTP, WebSocket, and TLS resources are rejected as captures and results. |
| Nested diagnostics | A failure through each aggregate kind names the boundary and the complete field/element/payload path to the non-`Transfer` leaf, with ownership-oriented guidance. |
| Generic boundary | Fully concrete inferred, explicit `target[Types]`, associated/static-method, default-context, and imported target specializations pass or fail structurally; unresolved type parameters at task or Queue boundaries are rejected conservatively with `AU3008`, and no source or inferred Transfer contract is exported in this batch. |
| All start surfaces | `start`, `start_soon`, `start_with_stack`, and `start_soon_with_stack` apply identical argument and result rules before scheduling. |
| Repeatable results | Copy results, `Queue[...]`, and recursively repeatable `Task[...]` results may be observed repeatedly through every applicable result/wait helper. |
| Single-consumer results | `Task[T]` is non-copy when `T` is non-repeatable; direct observations consume its unique right on every outcome, copied aliases/shared views cannot observe, both multi-wait helpers consume the whole task vector, and `wait_any` abandons unchosen rights. Include nested `Task[Task[String]]`, conservative `result_or_none` timeout/cancellation, loser abandonment, TaskGroup cleanup non-observation, and the runtime atomic-claim fallback. |
| Duplication diagnostics | Boundary rejection is `AU3008`; cloning, collection `get`, and implicit aggregate/container copies that duplicate a right are `AU3009`; a second observation of an already-consumed binding is `AU3001`; shared-access consumption is `AU3002`. |
| No user escape hatch | A same-named ordinary user trait and its implementations confer no compiler-derived Transfer property; annotations and other source assertions cannot override structural classification. |
| Editor and diagnostics parity | Compiler-service/LSP results preserve the stable code, nested reason, primary boundary span, guidance, and MIR/direct fixture parity. |
| Phase boundary | Scheduler tests remain single-worker through Phase 5.6; no test or documentation claims parallel execution before the pinned-worker gate. |

The ADR moves from Provisional only after the focused semantic, fixture,
compiler-service, both-backend parity, full-CI, and frozen-coverage gates pass.
