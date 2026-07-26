# Concurrency

Aurora provides single-threaded scheduler-backed lightweight tasks, structured task groups, queues, task handles, cancellation checks, sleeping, and multi-task wait helpers.

The maintained model is structured by default: child tasks should live inside a `TaskGroup`, and leaving the group scope waits for the children. Queue and task waits participate in the scheduler so a blocked task does not block the whole runtime.

## Duration Values

Scheduler APIs use `Duration`. This executable example covers every literal
unit plus the computed surface:

```python
def main() -> int32:
    attempt: int64 = 3
    print(10ms)
    print(1s)
    print(2m)
    print(attempt * Duration.ms(125))
    print(1ms // attempt)
    print(Duration.minutes(-1) < 0ms)
    print(Duration.seconds(2).to_ms())
    print(Duration.ms(1500).to_seconds())
    return 0
```

Durations are signed i128-nanosecond copy values. Use `Duration.ms(value)`,
`Duration.seconds(value)`, or `Duration.minutes(value)` when the count is an
`int64` expression rather than a literal. Checked `+`, `-`, multiplication by
an `int64` in either order, `// int64`, and all comparisons make computed
backoff and deadline selection expressible; for example, a runtime attempt
count can use `attempt * 1ms`.

`to_ms()` and `to_seconds()` convert the exact rational unit value to the
nearest representable IEEE-754 binary64 value, ties-to-even, and may round.
Printing and f-string interpolation instead render the exact decimal
millisecond value with at most six fractional digits and an `ms` suffix.

A negative Duration is representable but is not a valid sleep, timeout, or
backoff. Scheduler APIs on this page have no `io.Error` or `process.Error`
carrier, so a negative value, host-timer overflow, or deadline overflow traps
with `AU4001`. Overflow never changes the operation into an unlimited wait.
The exact host-timer classification is accepted under ADR-0019.

## TaskGroup

Construct a group with `TaskGroup()` and normally bind it with `with`:

```python
with group = TaskGroup():
    task = group.start(work, 1)
```

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `TaskGroup()` | Creates a task group resource. |
| `start` | `start(function, own ...) -> Task[T]` | Captures arguments into task-owned storage, starts a child task, and returns its handle. |
| `start_soon` | `start_soon(function, own ...) -> None` | Captures arguments into task-owned storage and starts a child task without returning a handle. |
| `cancel` | `cancel() -> None` | Signals cancellation to child tasks. |

`start` and `start_soon` accept named functions and associated methods without
`self`. Every argument is copied or moved into task-owned capture storage. A
default-mode non-copy or explicit shared target parameter borrows from that
storage for the child call; an `own` parameter consumes it. `borrow mut`
targets are rejected because detached mutable capture has no caller-visible
writeback.

On normal scope exit, the runtime joins children that continue making bounded progress. It cancels a child left in an indefinitely blocked group-owned wait so cleanup cannot deadlock forever. A failure already observed through its `Task` result is not raised a second time; an unread child failure aborts the group scope and wakes dependent queue/task waits.

## Task[T]

`Task[T]` is a copy handle to a child task result.

| API | Signature | Contract |
| --- | --- | --- |
| `result` | `result(timeout: Duration = ...) -> TaskResult[T]` | Waits for completion and returns a structured outcome; requires clone-safe `T`. |
| `result_or_none` | `result_or_none(timeout: Duration = ...) -> Option[T]` | Returns `Some(value)` on success and `None` on task failure, timeout, or cancellation; requires clone-safe `T`. Without an explicit timeout, this helper performs an immediate check. |
| `result_or` | `result_or(default: own T, timeout: Duration = ...) -> T` | Returns the task value or `default` on task failure, timeout, or cancellation; requires clone-safe `T`. Without an explicit timeout, this helper performs an immediate check. |

`TaskResult[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Ready(value: own T)` | The task returned normally. |
| `Error(message: own String)` | The task failed with a runtime error. |
| `TimedOut` | The wait timed out. |
| `Cancelled` | The wait was interrupted by cancellation. |

Use `result` when the program needs to distinguish failure, timeout, and cancellation. Use `result_or_none` or `result_or` only when those outcomes are intentionally equivalent.

The completed value is stored by the task and cloned for each observation. Repeated observation is supported for copy data and explicitly shared synchronized handles. A result containing an exclusive runtime-backed resource is single-observer-only in 0.1. That restriction is not yet enforced statically, so a second observation can alias the same host resource; transfer such a result to exactly one designated observer.

`random.Rng` is stricter: every task-result observation that could return a
generator, including through a wrapper, is rejected statically with `AU3007`.
For unresolved generic `T`, these methods infer a clone-safety obligation that
is checked after specialization.

## Queue[T]

`Queue[T]` moves values between tasks. Queue handles are copy values.

```python
jobs = Queue[String]()
bounded = Queue[String](capacity=8)
```

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `Queue[T](capacity: int32 = ...)` | Creates an unbounded queue when omitted or a bounded queue for a positive capacity; zero or negative capacity traps with `AU4001`. |
| `put` | `put(value: own T, timeout: Duration = ...) -> Result[None, SendError[T]]` | Sends `value`, waiting for capacity when needed. Returns the unsent value in the error variant. |
| `try_put` | `try_put(value: own T) -> Result[None, SendError[T]]` | Attempts to send without waiting. Returns `Full(value)` when a bounded queue is full. |
| `get` | `get(timeout: Duration = ...) -> QueueReceive[T]` | Receives one structured queue outcome. |
| `get_or_none` | `get_or_none(timeout: Duration = ...) -> Option[T]` | Returns `Some(value)` for an item and `None` for closed, timed-out, or cancelled receives. Without an explicit timeout, this helper performs an immediate check. |
| `get_or` | `get_or(default: own T, timeout: Duration = ...) -> T` | Returns an item or `default` for closed, timed-out, or cancelled receives. Without an explicit timeout, this helper performs an immediate check. |
| `close` | `close() -> None` | Closes the queue and wakes blocked senders and receivers. |

`SendError[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Closed(value: own T)` | The queue was closed before the value could be sent. |
| `Cancelled(value: own T)` | Cancellation interrupted the send. |
| `TimedOut(value: own T)` | The send timeout expired. |
| `Full(value: own T)` | `try_put` found a bounded queue at capacity. |

`QueueReceive[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Item(value: own T)` | A value was received. |
| `Closed` | The queue is closed and no value was available. |
| `TimedOut` | The receive timeout expired. |
| `Cancelled` | Cancellation interrupted the receive. |

Queue iteration:

```python
for value in jobs:
    print(value)
```

Queue iteration receives values: every `Item(value)` arrives already owned by
the loop binding. The Queue handle is a copy value, so ownership modifiers
have nothing to modify. `for value in own jobs`, `for value in borrow jobs`,
and `for value in borrow mut jobs` are all rejected; use the bare form above.
The bare form evaluates and copies the Queue handle once at loop entry. It does
not freeze the source binding: rebinding `jobs` in the body is permitted, but
later receives continue through the captured handle rather than switching to
the newly bound Queue. This source-selection timing is accepted in ADR-0017;
ADR-0006's receive ownership and modifier carve-out are unchanged.
The receive loop ends when the queue closes, cancellation interrupts it, or
the relevant producers in the active task group complete. Closing queues
explicitly is still the clearest program shape.

## Top-Level Concurrency Builtins

| API | Signature | Contract |
| --- | --- | --- |
| `cancelled` | `cancelled() -> bool` | Returns `true` when the current task has been asked to cancel. |
| `sleep` | `sleep(duration: Duration) -> None` | Suspends the current task for at least `duration`, unless cancellation wakes it first. |
| `wait_any` | `wait_any(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAny[T]` | Waits for the first task outcome or timeout; requires clone-safe `T`. `wait_any([])` returns `TimedOut` immediately. |
| `wait_all` | `wait_all(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAll[T]` | Waits until every task is ready, one task errors, timeout expires, or cancellation interrupts the wait; requires clone-safe `T`. |

`WaitAny[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Ready(index: own int32, value: own T)` | Task at `index` returned normally. |
| `Error(index: own int32, message: own String)` | Task at `index` failed. |
| `TimedOut` | No task completed before the timeout. |
| `Cancelled` | Cancellation interrupted the wait. |

`WaitAll[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Ready(values: own Vec[T])` | Every task returned normally. Values are in the same order as the input tasks. |
| `Error(index: own int32, message: own String)` | Task at `index` failed before all tasks completed. |
| `TimedOut` | Not every task completed before the timeout. |
| `Cancelled` | Cancellation interrupted the wait. |

## Cancellation Semantics

Cancellation is cooperative. `group.cancel()` marks child tasks as cancelled. Tasks observe that state through:

- `cancelled()`; the check is also a cooperative scheduler yield point
- `sleep(...)`
- queue send and receive waits
- task result waits
- `wait_any(...)` and `wait_all(...)`
- scheduler-aware process, network, and I/O waits where supported

Long CPU loops should check `cancelled()` directly:

```python
while not cancelled():
    do_step()
```

Cancellation interrupts Aurora's wait for scheduler-aware or worker-backed operations. It cannot forcibly stop an operating-system call that is already running on a blocking worker; such a call may still complete and perform its side effect after the task stops waiting.

Aurora 0.1 task scheduling is cooperative and single-threaded. CPU code that never reaches `cancelled()` or another scheduler-aware operation can starve sibling tasks. Each lightweight task reserves a fixed 1 MiB coroutine stack, and the bootstrap scheduler's readiness pass is linear in the number of waiting tasks/descriptors.

## Detached Work

Aurora does not currently expose a `spawn detached` language form. Keep lightweight task work under `TaskGroup` so scope exit has a clear join and cleanup boundary.

For operating-system child processes, use the `process` module and decide explicitly whether the child should be supervised, waited on, or closed.

## Grammar

Concurrency introduces no `async`, `await`, or detached-spawn grammar.
`TaskGroup`, `Task`, `Queue`, `sleep`, `cancelled`, `wait_any`, and `wait_all`
use ordinary construction and call syntax; structured groups use the ordinary
`with` statement. Queue iteration uses only `for item in queue:`. Duration
literal spelling is defined in [Lexical Structure](/manual/lexical-structure)
and the relevant statement and call productions are in
[Grammar](/manual/grammar).

## Typing Rules

`Queue[T]` and `Task[T]` are copy handles; `TaskGroup` is a managed move
resource. Queue sends, fallback values, task captures, and returned outcome
payloads use the exact owned positions shown in the API tables above. Task
targets are named functions or associated methods without `self`; generic
targets must infer all type arguments. Default/shared and `own` target
parameters are supported, while `borrow mut` targets are rejected. Queue
iteration yields `T` by ownership transfer and rejects all explicit ownership
modifiers. Timeout and capacity expressions must have the documented exact
types. Task-result and multi-task observations infer clone-safety obligations
for unresolved result types and reject a concrete result containing
`random.Rng`. Queue receive operations transfer one owned value and do not
require clone safety. A supplied Queue capacity must be greater than zero.

## Runtime Semantics

Aurora tasks run on one cooperative scheduler thread. Starting a child stores
its captures in task-owned storage. Group exit observes or joins children,
cancels an indefinitely blocked group-owned wait when required for cleanup,
and propagates an unread child failure. Queue send and receive transfer one
value by copy or move according to `T`; bounded queues suspend senders when
full, close wakes waiters, and bare
iteration repeatedly receives until its documented terminal condition.
Timeout, cancellation, closure, and task failure are distinct enum outcomes.
A nonpositive Queue capacity traps before a queue is constructed. Scheduling
order among simultaneously ready tasks is not specified.

## Ownership And Evaluation Order

Call arguments are evaluated before a task can use its captured values; every
non-copy capture moves into child-owned storage and a copy capture is copied.
The child then borrows or consumes that storage according to the target's
declaration-stable parameter mode. `put` owns its offered value and returns it
inside `SendError` when no send occurs. Queue iteration captures the copyable
handle once at loop entry, produces already-owned items, and never freezes or
borrows the source binding. Task result observation
clones the stored runtime value after satisfying its clone-safety obligation;
the single-observer resource limitation below
is therefore significant.

## Diagnostics

`AU1101` reports malformed concurrency syntax, including unavailable spawn
forms. `AU2001` reports unknown concurrency types, functions, or members,
including removed `Channel` names. `AU2002` covers generic, duration, capacity,
task-vector, argument, and outcome type mismatch. `AU2004` reports invalid
constructor or method argument binding. `AU2006` reports an explicit or
inherited trait method that collides with a builtin `Queue[T]`, `Task[T]`, or
`TaskGroup` member. `AU2999` covers unsupported targets, removed method aliases,
method-reference misuse, and remaining static concurrency rejections. `AU3001`
reports use after a value moves into task or queue storage. `AU3002` reports
invalid borrowed capture/storage use and the rejected `borrow mut` task-target
boundary. `AU3003` reports a mutating call through an immutable place, and
`AU3004` reports each forbidden Queue-iteration ownership modifier. `AU3007`
reports a task-result or multi-task observation whose produced value contains
or may contain non-cloneable `random.Rng` state. Timeout,
cancellation, closure, fullness, and an observed task error are typed values,
not diagnostics. An unread child trap retains its original code. `AU4001`
reports a general runtime trap, including zero or negative Queue capacity.
`AU4001` also reports a negative, unrepresentable, or overflowing scheduler
deadline because these APIs have no typed InvalidInput carrier. `AU4002`
reports arithmetic overflow or underflow, `AU4003` a bounds or lookup
violation, `AU4004` a zero divisor, and `AU4005` a resource or I/O failure.

## Backend Support

Structured groups, task targets and captures, Queue operations and iteration,
wait helpers, sleep, cancellation, and user-trait dispatch on `Queue[T]`,
`Task[T]`, and `TaskGroup` for noncolliding method names are maintained on both
MIR execution and direct native generation. Builtin handle member names retain
builtin dispatch on both backends. The scheduler/runtime surface and primary
diagnostics are parity-pinned.
MIR traps include Aurora call-chain and task-ancestry notes; direct native
traps may omit only those supplemental notes until the deferred frame work.

## Limits And Implementation-Defined Behavior

Task execution is cooperative, single-threaded, and non-preemptive; CPU code
without a scheduler boundary can starve siblings. Scheduling order among
simultaneously ready tasks is deliberately unspecified. Each lightweight task
reserves a fixed 1 MiB coroutine stack, and the MIR/direct entry thread
reserves 64 MiB. Readiness work is linear in waiting tasks/descriptors, and
nested Aurora calls stop at 256 frames. The process-wide blocking pool uses 2
through 8 host threads selected from host parallelism; it has no 0.1
configuration or queue backpressure, so slow or stuck jobs can delay unrelated
work behind them. A result holding an exclusive runtime resource is
single-observer-only, but the checker does not yet enforce that rule.
Cancelling a blocking-worker wait cannot retract an OS side effect already in
progress. Detached lightweight tasks are unavailable.

## Status

Scheduler-backed lightweight tasks, structured `TaskGroup`, generic task
handles and outcomes, bounded and unbounded queues, bare receive iteration,
sleep, cooperative cancellation, task-result observation, and multi-task waits
plus computed Duration arithmetic are implemented for the Phase 3 surface.
The host-timer policy recorded by ADR-0019 is Accepted. Multicore Aurora task execution
is reserved for the Batch 3 runtime work. Preemptive scheduling,
mutable-borrow task targets, statically enforced single-observer resource
results, and detached task syntax are unavailable. The capacity boundary is
pinned by
`crates/aurora-compiler/tests/fixtures/run-fail/queue_zero_capacity.au` and
`crates/aurora-compiler/tests/fixtures/run-fail/queue_negative_capacity.au` on
both backends.
