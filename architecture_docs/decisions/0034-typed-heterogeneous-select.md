# ADR-0034: Typed heterogeneous `select`

- Status: Accepted
- Date: 2026-07-28
- Accepted: 2026-07-29
- Roadmap decision: Batch 4, Phase 5.8
- Related: ADR-0008, ADR-0019, and ADR-0033

## Context

Aurora can wait for one task with `Task.result`, receive from one queue with
`Queue.get`, and wait for one of several same-result-type tasks with
`wait_any`. A concurrent service often needs a different operation: wait for
whichever of several queues, tasks, and deadlines becomes ready first without
polling or dedicating a helper task to each source.

The original language proposal showed a statement-shaped `select`, but that
surface was removed before Aurora 0.1. Phase 5.8 is explicitly limited to a
builtin-style wait. Statement syntax would add grammar, binding, control-flow,
and ownership questions that are not necessary to establish the runtime
primitive.

A heterogeneous wait also needs stronger runtime machinery than a loop over
the existing one-source APIs. Checking a source and then subscribing to it can
lose a wakeup. Letting every callback consume its source can produce several
winners. Leaving registrations behind after one source wins can wake a
completed task, retain queues and task handles, or race freed selection state.
The Phase 5.8 implementation therefore cannot land until atomic registration,
one-winner arbitration, and loser cleanup are proved on the pinned-worker
scheduler.

## Decision

### Source and result surface

`select` is a variadic positional builtin:

```aurora
select(source, ...)
```

It accepts one or more sources, each of which is exactly one of:

- `Queue[Q]`, meaning one receive from that queue
- `Task[T]`, meaning observation of that task's terminal result
- `Duration`, meaning a relative deadline

Named arguments are not accepted. The sources may be interleaved in any order,
and any source category may be absent. All queue sources in one call must have
the same payload type `Q`; all task sources must have the same result type
`T`. Queue and task types are independent, so `Q` need not equal `T`. When no
queue source is present, `Q` is `None`; when no task source is present, `T` is
`None`. A deadline-only call therefore has result type
`SelectOutcome[None, None]`. Mixed queue payload types or mixed task result
types are rejected rather than widened to a union.

The builtin returns this builtin generic enum:

```aurora
enum SelectOutcome[Q, T]:
    Queue(int32, QueueReceive[Q])
    Task(int32, TaskResult[T])
    Deadline(int32)
    Cancelled
```

As with other builtin enum declarations, payloads are owned even though the
declaration shape omits capability syntax. The first payload in each
source-specific variant is the source's zero-based position in the original
argument list, not its position among sources of the same category.

`SelectOutcome.Queue(index, outcome)` preserves the `QueueReceive` house
style. A queue source can produce `Item(value)` or `Closed`; it has no
individual timeout, so `QueueReceive.TimedOut` is not produced by `select`,
and current-task cancellation is represented by the outer
`SelectOutcome.Cancelled`. An already closed queue is ready. If a closed queue
still has buffered items, its ordinary receive rule applies: an item is
selected before `Closed`.

`SelectOutcome.Task(index, outcome)` preserves the `TaskResult` house style. A
terminal child can produce `Ready(value)`, `Error(message)`, or `Cancelled`.
Child cancellation is therefore distinct from cancellation of the selecting
task. A task source has no individual timeout, so `TaskResult.TimedOut` is not
produced by `select`.

`SelectOutcome.Deadline(index)` identifies the relative deadline that expired.
Every `Duration` is measured from one common wait-start instant after all
source expressions have been evaluated and validated. Zero is an immediate
deadline. A negative duration or a duration that cannot be added to the host
monotonic clock traps with `AU4001`; Aurora does not clamp or wrap it.

### Evaluation and deterministic choice

Source expressions are evaluated exactly once, from left to right. Their
zero-based indexes are fixed by that evaluation order. Queue handles,
repeatable Task handles, and Duration values are copied into the selection.
The ownership rule for a non-repeatable Task source is described below.

At every arbitration point, the selecting task's cancellation is checked
first. If cancellation is observed, the result is
`SelectOutcome.Cancelled`, even when a source is also ready. Otherwise, when
more than one source is ready at the same arbitration point, the source with
the lowest original argument index wins. This rule covers already-ready
sources, equal or zero deadlines, duplicate handles, and readiness published
while the task is parked.

A wakeup is evidence that arbitration should run, not permission for a
callback to return a result by itself. Queue receives are still atomic with
other queue consumers. If a queue that published readiness loses its item to
another consumer before this selection can claim it, that source is no longer
ready and arbitration continues. Once one source is atomically claimed, its
winner is committed; cancellation or another readiness event observed later
does not replace it.

### Task observation rights and duplicate sources

ADR-0033's repeatable and single-consumer task-result rules apply to every
Task source:

- A Task whose result is repeatable is a copy source. It remains usable after
  `select`, whether it wins or loses.
- A Task whose result is not repeatable consumes its unique observation right
  at call entry, just as the vector passed to `wait_any` is consumed. Every
  such source must therefore be supplied through owned access.
- If a non-repeatable Task wins, its `TaskResult` is returned. If it loses to
  another source, its observation right is deliberately abandoned. Timeout,
  current-task cancellation, source validation failure after source
  evaluation, and task failure do not restore a consumed source-level right.

Queue handles may be duplicated in one call. Repeatable Task handles and
Duration values may also be duplicated. Duplicate queue entries compete as
independent indexed receives, but only the selected entry removes one value.
The lowest index wins when duplicate entries are simultaneously ready.

The same non-repeatable Task handle may not appear more than once. The checker
rejects a statically visible duplicate with `AU3009` and explains that one
call cannot duplicate a single-consumer observation right. The runtime claims
non-repeatable observation rights in source order before waiting and retains
ADR-0033's atomic claim as defense in depth. A duplicate or already-claimed
handle that reaches the runtime traps with `AU4001`; it cannot deliver or
clone a result. A runtime claim failure does not restore any source-level move
already performed by the call.

### Atomic registration and loser cleanup

One selection owns one composite waiter and one winner state. Its required
protocol is:

1. evaluate and validate every source, establish the common deadline base,
   and claim non-repeatable Task observation rights
2. check cancellation and probe all sources in index order
3. subscribe the composite waiter to every source that is not yet ready,
   using each source's synchronized state or generation
4. recheck cancellation and every source in index order before parking
5. have queue, task-completion, deadline, and cancellation events publish
   readiness and enqueue the selecting task directly on its pinned worker
6. on wake, arbitrate again in the specified order and atomically claim at
   most one ready source
7. unregister every losing source before returning or propagating a trap

The check-subscribe-recheck sequence is mandatory: no event may be lost
between an initial readiness check and registration. Event publication may
coalesce several wakes, but it may not choose a result out of index order.
Only the arbitration step consumes a queue item or reads a selected task
result. Registration alone has no observable source effect.

Loser cleanup is required on a source win, current-task cancellation, dynamic
validation or claim failure, runtime trap, and unwinding. Cleanup must be
idempotent and safe against a callback already in flight. After cleanup, a
late notification may observe that the selection is closed but may not retain
the selection, enqueue its task again, consume a queue value, observe a task
result, or access freed state.

The implementation uses the persistent reactor and pinned-worker wake paths.
It must not introduce polling, a periodic scheduler tick, one helper task per
source, or task migration. Cross-worker Queue and Task notifications use the
same synchronized worker inboxes as their one-source operations.

### MIR and direct-backend contract

The checker produces the same resolved source sequence and
`SelectOutcome[Q, T]` type for both backends. MIR lowering keeps each evaluated
source in order and calls the shared selection runtime with typed source
descriptors.

The direct backend uses one internal runtime entry point with the intended
shape:

```text
aurora_direct_select(tuple_ptr)
```

`tuple_ptr` identifies an owned runtime tuple containing the already evaluated
sources in source order. This is an internal compiler/runtime ABI, not a
source API or a stable foreign-function interface. The runtime validates the
descriptor values as defense in depth, applies the same observation claims
and registration protocol as MIR, and returns the canonical runtime
representation of `SelectOutcome`. The two backends share arbitration,
deadline, cancellation, and loser-cleanup semantics rather than maintaining
different polling loops.

### Diagnostics and source compatibility

The diagnostic contract is:

- zero sources, named arguments, and other call-shape errors use `AU2004`
- a source that is not a Queue, Task, or Duration, or inconsistent queue/task
  payload types, uses `AU2002` and names the accepted source categories or the
  category-specific common type requirement
- consuming a non-repeatable Task through shared access uses `AU3002`; using a
  source binding after that call uses the ordinary moved-value `AU3001`
- a statically visible duplicate non-repeatable Task uses `AU3009`
- negative or host-range-overflowing deadlines and runtime observation-claim
  failures use `AU4001`

Messages for single-consumer failures teach that `select` consumes every
non-repeatable Task source at call entry and abandons losing rights. They do
not suggest cloning a non-repeatable handle.

`select` becomes a builtin function name and `SelectOutcome` becomes a builtin
enum name under the ordinary builtin-redefinition rules. Programs that
previously declared those names must rename them. This is the only new source
incompatibility. The lexer does not make `select` a keyword, and the removed
statement form remains rejected:

```aurora
select:
    case value = queue.get():
        pass
```

No branch syntax, receive/send expression syntax, pattern binding, default
arm, or fairness policy is added in this batch. Such sugar requires a later
decision. `wait_any`, `wait_all`, `Queue.get`, and `Task.result` retain their
existing signatures and behavior.

## Consequences

Aurora programs can express one heterogeneous wait without polling or helper
tasks, while retaining typed queue and task outcomes and a deterministic
source index. Multiple queues must share one payload type and multiple tasks
must share one result type; programs needing more heterogeneous payloads can
wrap them in an explicit user enum.

Index priority is deterministic rather than fair. A persistently ready
lower-index source can starve a higher-index source. Callers that require
round-robin service can rotate the argument order between calls; Aurora does
not hide that policy in the scheduler.

Passing a non-repeatable Task is an observation attempt even if a queue or
deadline wins. This conservative rule keeps one static ownership contract
across all races. A program that may need the losing result must publish it
through a Queue or use a repeatable result type instead.

The nested outcome types retain Aurora's established distinctions and make
match handling familiar. Their timeout/cancellation variants are intentionally
broader than the subset a selected Queue or Task source can produce; the outer
enum records selection-level deadline and cancellation.

## Completion-test matrix

| Contract | Required evidence |
| --- | --- |
| Call shape and inference | One or more positional Queue, Task, and Duration sources in every category combination; `Q` and `T` inference including absent categories as `None`; zero-source, named-argument, wrong-source, mixed-`Q`, and mixed-`T` diagnostics with stable codes and guidance. |
| Evaluation and indexing | Side-effecting expressions prove exactly-once left-to-right evaluation; outcomes carry the original zero-based index across interleaved categories. |
| Queue outcomes | Ready item, buffered-then-closed, empty closed, pending wake, duplicate Queue handles, competing external consumers, exactly one removed item, and unchanged losing queues. |
| Task outcomes | Ready, error, and child-cancelled terminal results; pending completion from another worker; selected non-repeatable delivery; loser abandonment; repeatable winner and loser reuse. |
| Task ownership | Static consumption, second-use `AU3001`, shared-access `AU3002`, visible duplicate `AU3009`, recursive repeatability, and the runtime `AU4001` claim fallback. |
| Deadlines | Zero, distinct and equal durations, common-base behavior, negative duration, host-clock overflow, and a deadline racing Queue and Task readiness. |
| Priority and cancellation | Lowest original index for every simultaneous-ready category pairing and duplicates; current-task cancellation before every source; committed winner not replaced by later cancellation. |
| Atomic registration | Deterministic check-subscribe race injection for Queue, Task, deadline, and cancellation proves no missed wake; concurrent publication proves one winner and one task enqueue. |
| Loser cleanup | Every result and failure exit unregisters all losers; repeated cleanup and late callbacks are harmless and do not consume, retain, enqueue, or access freed state. |
| Scheduler architecture | Cross-worker Queue/Task wakeups use direct worker inboxes; idle selection blocks without scanning, helper tasks, periodic ticks, or migration. |
| Backend parity | MIR typed-source lowering and the direct tuple ABI produce byte-identical observable output and diagnostics across the full outcome/race matrix. |
| Language tooling | Compiler-service diagnostics, completion, hover, builtin-redefinition checks, builtin enum display, and LSP behavior expose the exact accepted surface. |
| Compatibility and reference | Parser fixtures keep statement `select` rejected; existing wait APIs remain unchanged; maintained examples, tutorials, manual/API/conformance pages, ADR links, and verified reference blocks agree with the implemented builtin. |

The Batch 4 implementation and its focused semantic, fixture, runtime
race/stress, compiler-service, LSP, both-backend parity, full-CI, benchmark,
and frozen-coverage gates passed. Batch 5 then closed the remaining nested
generic payload-typing defect with arithmetic, comparison, f-string, and
reassignment coverage on both backends, completing this matrix.
