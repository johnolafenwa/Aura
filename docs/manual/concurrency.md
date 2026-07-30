# Concurrency

Aurora provides pinned-worker scheduler-backed lightweight tasks, structured
task groups, queues, task handles, cancellation checks, sleeping, and
typed single- and multi-source wait helpers. Scheduler waits use a persistent
event reactor:
descriptors stay registered, deadlines live in a timer heap, and Queue,
task-completion, and blocking-pool events notify the responsible worker
directly.

The maintained model is structured by default: child tasks should live inside a `TaskGroup`, and leaving the group scope waits for the children. Queue and task waits participate in the scheduler so a blocked task does not block the whole runtime.

The runtime creates one pinned worker per unit of available parallelism
reported by the host by default. The provisional
`AURORA_WORKERS=<positive integer>` environment override selects an explicit
worker count. A child receives a stable worker assignment when it is
spawned; its coroutine stack never migrates and the runtime performs no work
stealing. This contract is shared by MIR execution and direct native
execution. A positive override may exceed the host-reported default.
`AURORA_WORKERS=1` preserves single-worker cooperative execution through the
same worker-thread architecture.
Empty, zero, signed, whitespace-padded, nonnumeric, and overflowing values are
rejected before execution with `AU4006`.

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
| `start` | `start(function, own ...) -> Task[T]` | Requires every capture and result to be `Transfer`, starts the specialized target, and returns its handle. |
| `start_soon` | `start_soon(function, own ...) -> None` | Requires every capture and result to be `Transfer` and starts the specialized target without returning a handle. |
| `start_with_stack` | `start_with_stack(bytes: int64, function, own ...) -> Task[T]` | Applies the same Transfer rules with an explicit guarded stack-capacity request and returns the handle. |
| `start_soon_with_stack` | `start_soon_with_stack(bytes: int64, function, own ...) -> None` | Applies the same Transfer rules with an explicit guarded stack-capacity request and no returned handle. |
| `cancel` | `cancel() -> None` | Signals cancellation to child tasks. |

All four start methods accept capture-free function values, which are copy
values and satisfy `Transfer`, plus closure values whose complete captured
environment is Transfer. Existing direct named-function and
associated-method-without-`self` targets remain accepted, including explicit
generic targets written as `function[Types]` or
`Type.associated_method[Types]` in the callable slot. Associated methods do not
thereby become general first-class method values. Every target argument is
copied or moved into task-owned capture storage. A bare shared target parameter
borrows from that storage for the child call; an `own` parameter consumes it.
`mut` targets are rejected because detached mutable capture has no
caller-visible writeback.
When a function-value contract retains default availability, omitted task
arguments evaluate the runtime-selected target's own default expression. Task
start therefore follows the same default-binding rule as an ordinary indirect
call.

Ordinary `start` and `start_soon` request the 524,288-byte (512 KiB) default.
The two `_with_stack` methods take an exact `int64` byte count before the
callable target. Accepted requests are 262,144 through 67,108,864 bytes
inclusive (256 KiB through 64 MiB). Values outside that range are rejected,
not clamped. An accepted request is rounded upward to the host page size and
the platform stack allocator adds guard-page protection; guard pages are not
part of the requested writable capacity. The separate method names avoid
stealing a keyword that could belong to the target's own arguments. This
surface is Provisional under ADR-0032.

The 256 KiB lower bound is an opt-in minimum for a task whose shallow stack use
has been measured; it is not the generally safe default. During integration,
the complete compiled Aurora HTTP example faulted when 256 KiB was used as the
global task default and succeeded with the 512 KiB default. An isolated
runtime-level HTTP regression does succeed when only its protocol-calling
children are forced to 256 KiB: that test proves deep host protocol frames
stay on the service workers, but it excludes the compiled program's
MIR/direct language-execution frames. Keep the ordinary default unless
measurement of the complete task justifies a custom size.

```python
with group = TaskGroup():
    parser = group.start_with_stack(512 * 1024, parse_document, source)
    group.start_soon_with_stack(2 * 1024 * 1024, deep_worker, jobs)
```

On normal scope exit, the runtime joins children that continue making bounded
progress. It cancels a child in an unbounded group-owned wait only when the
live wait graph has no reachable waker. A queue wait therefore remains
joinable while another live task can send, receive, or close the relevant
queue; the task currently performing the join does not count as a waker for
its own children. A failure already observed through its `Task` result is not
raised a second time; an unread child failure aborts the group scope and wakes
dependent queue/task waits.

## Task[T]

`Task[T]` is a transferable handle to a child task result. Under Provisional
ADR-0033 it is copyable only when `T` is repeatable, so aliases cannot
duplicate one result right.

| API | Signature | Contract |
| --- | --- | --- |
| `result` | `result(timeout: Duration = ...) -> TaskResult[T]` | Waits for completion and returns a structured outcome; a non-repeatable `T` consumes the observation right on this call. |
| `result_or_none` | `result_or_none(timeout: Duration = ...) -> Option[T]` | Returns `Some(value)` on success and `None` on task failure, timeout, or cancellation; a non-repeatable `T` consumes the observation right even when `None` is returned. Without an explicit timeout, this helper performs an immediate check. |
| `result_or` | `result_or(default: own T, timeout: Duration = ...) -> T` | Returns the task value or `default` on task failure, timeout, or cancellation; a non-repeatable `T` consumes the observation right. Without an explicit timeout, this helper performs an immediate check. |

`TaskResult[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Ready(value: own T)` | The task returned normally. |
| `Error(message: own String)` | The task failed with a runtime error. |
| `TimedOut` | The wait timed out. |
| `Cancelled` | The wait was interrupted by cancellation. |

Use `result` when the program needs to distinguish failure, timeout, and cancellation. Use `result_or_none` or `result_or` only when those outcomes are intentionally equivalent.

The completed value is stored by the task. Under Accepted ADR-0033,
`Task[T]` is copyable only when `T` is copyable, `T` is a `Queue[...]` handle,
or `T` is a recursively repeatable `Task[...]`. For every
other transferable result, `result`, `result_or_none`, and `result_or` consume
the unique observation right on any outcome. The consumption is conservative:
timeout, cancellation, failure, and a collapsed `None` do not restore it.
`wait_any` and `wait_all` consume the whole task vector for such a `T`;
`wait_any` abandons the unchosen observation rights.

## Queue[T]

`Queue[T]` moves values between tasks. Queue handles are copy values.

```python
jobs = Queue[String]()
bounded = Queue[String](capacity=8)
```

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `Queue[T](capacity: int32 = ...)` | Creates an unbounded queue when omitted or a bounded queue for a positive capacity; requires `T: Transfer`; zero or negative capacity traps with `AU4001`. |
| `put` | `put(value: own T, timeout: Duration = ...) -> Result[None, SendError[T]]` | Sends a `Transfer` value, waiting for capacity when needed. Returns the unsent value in the error variant. |
| `try_put` | `try_put(value: own T) -> Result[None, SendError[T]]` | Attempts to send a `Transfer` value without waiting. Returns `Full(value)` when a bounded queue is full. |
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
have nothing to modify. The bare `for value in jobs` form above is accepted;
`for value in own jobs` and `for value in mut jobs` are rejected.
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
| `yield_now` | `yield_now() -> None` | Voluntarily yields the current lightweight task so other runnable work can proceed. |
| `sleep` | `sleep(duration: Duration) -> None` | Suspends the current task for at least `duration`, unless cancellation wakes it first. |
| `select` | `select(source, ...) -> SelectOutcome[Q, T]` | Waits on one or more positional `Queue[Q]`, `Task[T]`, or relative-`Duration` sources; cancellation wins, otherwise the lowest ready source index wins. |
| `wait_any` | `wait_any(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAny[T]` | Waits for the first task outcome or timeout. For non-repeatable `T`, consumes the vector and abandons unchosen observation rights. `wait_any([])` returns `TimedOut` immediately. |
| `wait_all` | `wait_all(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAll[T]` | Waits until every task is ready, one task errors, timeout expires, or cancellation interrupts the wait. For non-repeatable `T`, consumes the vector. |

### Explicit Cooperative Yielding

`yield_now()` places the current lightweight task back in the scheduler ready set
and returns `None` when that task is selected to run again. It gives other
runnable tasks assigned to the same pinned worker an opportunity to proceed,
but it does not migrate the task, search another worker for work, guarantee
that another task runs before it returns, or specify which runnable task is
selected. If there is no current schedulable lightweight task, the call
returns without effect.

The call does not sleep, wait for an event or deadline, or inspect or change
cancellation state. Use it between bounded chunks of CPU work when an explicit
cooperative scheduling point is useful. Use `cancelled()` separately when the
task must also respond to cancellation.

`WaitAny[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Ready(index: own int32, value: own T)` | Task at `index` returned normally. |
| `Error(index: own int32, message: own String)` | Task at `index` failed. |
| `TimedOut` | No task completed before the timeout. |
| `Cancelled` | Cancellation interrupted the wait. |

### Typed Heterogeneous Selection

`select(source, ...)` waits without polling over any positional mixture of
`Queue[Q]`, `Task[T]`, and relative `Duration` sources. At least one source is
required and named arguments are rejected. All Queue sources in one call use
one payload type `Q`, and all Task sources use one result type `T`; the two
categories are independent. An absent category is represented by `None`, so
selecting a `Queue[String]` with a deadline returns
`SelectOutcome[String, None]`, while selecting a `Task[int32]` with a deadline
returns `SelectOutcome[None, int32]`.

`SelectOutcome[Q, T]` variants:

| Variant | Meaning |
| --- | --- |
| `Queue(index: own int32, outcome: own QueueReceive[Q])` | The Queue at the original zero-based source index produced an item or closed outcome. |
| `Task(index: own int32, outcome: own TaskResult[T])` | The Task at the original zero-based source index produced a ready, error, or child-cancelled outcome. |
| `Deadline(index: own int32)` | The relative Duration at the original zero-based source index expired. |
| `Cancelled` | Cancellation of the selecting task interrupted the wait. |

Queue sources have no individual timeout, so `select` never produces
`QueueReceive.TimedOut`; selecting-task cancellation uses the outer
`SelectOutcome.Cancelled`. Task sources likewise never produce
`TaskResult.TimedOut`. A child task that is itself cancelled still produces
the nested `TaskResult.Cancelled` outcome.

Source expressions are evaluated exactly once from left to right. All
durations use one common base instant after evaluation and validation. Zero is
immediately ready; a negative or host-range-overflowing duration traps with
`AU4001`. Current-task cancellation has priority over every source. Otherwise,
if several sources are ready at the same arbitration point, the lowest
original argument index wins. A selected Queue removes exactly one item;
losing Queue sources remain unchanged. A closed Queue is ready, with buffered
items received before `Closed`.

Selecting a repeatable Task leaves the handle reusable. Every non-repeatable
Task observation right is consumed at call entry, even when another source
wins; losing rights are deliberately abandoned, matching `wait_any`.
Repeating the same non-repeatable Task in one call is rejected with `AU3009`.
Queue handles, repeatable Tasks, and Duration values may be repeated, and the
lowest ready occurrence wins. Selection uses one composite
check-subscribe-recheck registration and removes every losing registration
before returning, trapping, or propagating cancellation. Once a source has
been atomically claimed, that winner is committed: cancellation or another
readiness event observed later does not replace it.

Index priority is deterministic, not fair. A persistently ready lower-index
source can starve a higher-index source. Rotate argument order between calls
when round-robin service is required.

```python
def main() -> int32:
    messages = Queue[String]()
    messages.put("ready")
    print(select(messages, 0ms))
    return 0
```

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
- `yield_now()` is a scheduling point but does not itself inspect cancellation
- `sleep(...)`
- queue send and receive waits
- task result waits
- `select(...)`
- `wait_any(...)` and `wait_all(...)`
- scheduler-aware process, network, and I/O waits where supported

Long CPU loops should check `cancelled()` directly:

```python
while not cancelled():
    do_step()
```

Cancellation interrupts Aurora's wait for scheduler-aware or worker-backed
operations. For the generic blocking-I/O pool, insertion into the pending job
queue is the acceptance boundary. Cancellation or deadline expiry while a
caller is still waiting for admission prevents submission. After acceptance,
Aurora cannot forcibly stop the pending or running host operation; it executes
once, may still perform its side effect, and has any late result discarded.
A configured queue bound limits accepted pending work, but cannot guarantee
unrelated blocking-I/O progress while all blocking workers remain stuck.

Aurora 0.1 task scheduling is cooperative across pinned workers. The compiler
inserts a scheduling check on every loop backedge, including the ordinary body
tail and `continue`, so a tight loop eventually lets ready timers, Queue
operations, and socket work on the same worker proceed. `break` and `return`
leave the loop without taking that check. A single long loop body or long
straight-line CPU work can still delay siblings pinned to that worker. The
inserted check does not inspect cancellation; tasks that must stop on request
still call `cancelled()`. Each ordinary lightweight task requests a guarded
512 KiB coroutine stack; the two explicit stack-start methods may request up
to 64 MiB. When a worker has no ready task, its event reactor blocks until a
notification, descriptor event, or deadline; it does not wake on a periodic
scheduler tick.

## Detached Work

Aurora does not currently expose a `spawn detached` language form. Keep lightweight task work under `TaskGroup` so scope exit has a clear join and cleanup boundary.

For operating-system child processes, use the `process` module and decide explicitly whether the child should be supervised, waited on, or closed.

## Grammar

Concurrency introduces no `async`, `await`, or detached-spawn grammar.
`TaskGroup`, `Task`, `Queue`, `yield_now`, `sleep`, `cancelled`, `select`,
`wait_any`, and `wait_all` use ordinary construction and call syntax;
structured groups use
the ordinary `with` statement. Stack overrides are ordinary member calls, not
new task or spawn grammar. Queue iteration uses only
`for item in queue:`. Duration
literal spelling is defined in [Lexical Structure](/manual/lexical-structure)
and the relevant statement and call productions are in
[Grammar](/manual/grammar).

## Typing Rules

`Queue[T]` is a copy handle; `Task[T]` is conditionally Copy under Provisional
ADR-0033; `TaskGroup` is a managed move resource. Queue sends, fallback values,
task captures, and returned outcome payloads use the exact owned positions
shown in the API tables above. Task targets may be capture-free function or
Transfer closure values. The existing direct named-function and
associated-method-without-`self` forms remain accepted; generic targets may
infer every type argument or use explicit `function[Types]` and
`Type.associated_method[Types]` specialization in the callable slot. Bare
shared and `own` target parameters are supported, while `mut` targets are
rejected. An explicit stack capacity
must have exact type `int64`; the first callable argument and every capture
retain the same typing rules as an ordinary start. Queue
iteration yields `T` by ownership transfer: the bare form is accepted, while
the `own` and `mut` modifiers are rejected. Timeout and capacity expressions
must have the documented exact types. Queue receive operations transfer one
owned value and do not recheck payload Transfer. A supplied Queue capacity
must be greater than zero.

Every `select` source must be exactly `Queue[Q]`, `Task[T]`, or `Duration`.
Queue payload types agree with one `Q`, Task result types agree with one `T`,
and a missing category is inferred as `None`. A non-repeatable Task source is
an owned observation and is moved at call entry; a repeatable Task and every
Queue or Duration source is read without consuming its source binding.

The Provisional Phase 5.6 boundary adds a structural `Transfer` obligation to
every captured argument and target result for all four task-start methods,
after generic specialization and before scheduling. A fully concrete generic
call is checked after inference; an unresolved type parameter is rejected
rather than becoming a deferred Transfer contract. The obligation applies to
the owned capture even when the target declares a bare shared parameter and
borrows that child-owned storage during its call. Queue construction, `put`,
and `try_put` likewise require `T: Transfer`; handle copies, receive/fallback
methods, and `close` do not recheck the payload. Copy types, `String`, recursively
transferable collections/tuples/classes/enums, and `Queue`/`Task` handles pass;
capability views, `random.Rng`, `TaskGroup`, and live host resources do not.
`Transfer` is compiler-derived rather than a user trait.

Reading a Copy value through shared or mutable access for a task argument
captures an owned snapshot rather than the access capability, so that snapshot
is permitted when its type is Transfer. A non-copy access cannot be captured
this way because the child would need ownership.

Explicit generic task targets may use `function[Types]` or
`Type.associated_method[Types]` in the callable-target slot. Brackets remain
ordinary indexing elsewhere. A bare target is accepted when its declarations
and defaults already resolve complete concrete types.

## Runtime Semantics

Aurora tasks run on cooperative pinned workers. The default worker count is
the available parallelism reported by the host, while provisional
`AURORA_WORKERS=<positive integer>` selects an explicit count. Starting a child
stores its captures in task-owned storage and gives it a stable worker
assignment. The child's coroutine stack remains on that worker for its entire
lifetime; there is no migration or work stealing. Group exit observes or joins
children, cancels an unbounded group-owned wait only when the live wait graph
has no reachable waker, and propagates an unread child failure. Host elapsed
time and machine load are not evidence that a wait is unreachable. Queue send and receive
transfer one value by copy or move according to `T`; bounded queues suspend
senders when full, close wakes waiters, and bare iteration repeatedly receives
until its documented terminal condition.
Timeout, cancellation, closure, and task failure are distinct enum outcomes.
A nonpositive Queue capacity traps before a queue is constructed. Scheduling
order, completion order among independent tasks, and program-output order are
not specified. Descriptor waits use
persistent reactor registrations, deadlines use a timer heap, and Queue,
task-completion, and blocking-pool readiness is delivered by direct
notification to the responsible worker. With no ready local work, a worker
blocks until work, an event, or a deadline rather than polling on a fixed tick.

Typed selection uses the same persistent wait machinery. One waiter subscribes
to all Queue, Task, cancellation, and earliest-deadline sources, rechecks them
before parking, and re-arbitrates in source order after a wake. Notifications
do not choose the winner themselves. Committing a winner atomically consumes
only that Queue item or selected Task result, then idempotently removes every
losing subscription. Selection does not create helper tasks, migrate the
selecting task, or introduce a periodic scheduler tick.

Queue and Task handles are the maintained cross-worker communication surface.
Their runtime state is synchronized so a Queue operation or task completion
can wake a task pinned elsewhere. Every other captured argument and task result
must be owned `Transfer` data, preserving a share-nothing boundary. Live host
resources and capability views remain on their owning task. Cancellation and
diagnostic state remain isolated per task: running or trapping on one worker
does not replace another task's current cancellation or diagnostic context.

A running child may create a nested `TaskGroup`, start grandchildren, and
immediately wait on their returned handles on both backends. Child preparation
allocates the guarded stack and task state before the handle is returned; a
preparation failure is synchronous and admits no child. Successful nested
starts are transferred through the scheduler's internal admission broker
rather than mutating the scheduler through a second live reference. The broker
preserves request FIFO internally, but ready-task and child execution order
remain deliberately unspecified.

Dynamic `json.parse` uses a separate process-global codec service with two
2 MiB-stack workers and total in-flight capacity two. The runtime reserves one
of those slots before it makes the fallible owned source copy. A saturated
lightweight task parks on a scheduler-aware availability notification rather
than spinning. Once admitted, synchronous `json.parse` waits for codec
completion; cancellation is deferred to the task's next ordinary cancellation
boundary. The legacy `json.is_valid` and `json.parse_string_map` helpers remain
bounded caller-side compatibility operations and do not use the service. The
service is distinct from the protocol and generic blocking-I/O pools and lives
until process exit. The remaining stack-safety and backend rules are in
[Execution Model](/manual/execution-model) and [JSON Module](/manual/json).

## Ownership And Evaluation Order

Call arguments are evaluated before a task can use its captured values; every
non-copy capture moves into child-owned storage and a copy capture is copied.
For a stack override, the capacity expression is evaluated once before the
callable target and its captures.
The child then borrows or consumes that storage according to the target's
declaration-stable parameter mode. `put` owns its offered value and returns it
inside `SendError` when no send occurs. Queue iteration captures the copyable
handle once at loop entry, produces already-owned items, and never freezes or
borrows the source binding. Task result observation clones a stored value only
when the result is repeatable. A non-repeatable result instead carries one
statically enforced observation right, and no alias may produce a second
value. A `select(...)` call evaluates all source expressions once from left to
right. It copies Queue, repeatable Task, and Duration sources, but consumes
every non-repeatable Task observation right at call entry and deliberately
abandons any such right that loses.

## Diagnostics

`AU1101` reports malformed concurrency syntax, including unavailable spawn
forms. `AU2001` reports unknown concurrency types, functions, or members,
including removed `Channel` names. `AU2002` covers generic, duration, capacity,
task-vector, stack-byte, argument, and outcome type mismatch. `AU2004` reports invalid
constructor or method argument binding. `AU2006` reports an explicit or
inherited trait method that collides with a builtin `Queue[T]`, `Task[T]`, or
`TaskGroup` member. `AU2999` covers unsupported targets, removed method aliases,
method-reference misuse, and remaining static concurrency rejections. `AU3001`
reports use after a value moves into task or queue storage. `AU3002` reports
invalid borrowed capture/storage use and the rejected `mut` task-target
boundary. `AU3003` reports a mutating call through an immutable place, and
`AU3004` reports the forbidden `own` and `mut` Queue-iteration modifiers. `AU3007`
reports a task-result or multi-task observation whose produced value contains
or may contain non-cloneable `random.Rng` state. Timeout,
cancellation, closure, fullness, and an observed task error are typed values,
not diagnostics. An unread child trap retains its original code. `AU4001`
reports a general runtime trap, including zero or negative Queue capacity.
`AU4001` also reports a negative, unrepresentable, or overflowing scheduler
deadline because these APIs have no typed InvalidInput carrier. `AU4002`
reports arithmetic overflow or underflow, `AU4003` a bounds or lookup
violation, `AU4004` a zero divisor, and `AU4005` a resource or I/O failure.
`AU4006` reports invalid pinned-worker or blocking-I/O runtime configuration.
`AURORA_WORKERS`, `AURORA_BLOCKING_WORKERS`, and
`AURORA_BLOCKING_QUEUE_CAPACITY` each require a positive decimal integer; the
diagnostic names the setting, renders the supplied invalid value, and is issued
before user code. A non-Unicode value is displayed lossily.
`AU2002` rejects an out-of-range literal stack request during checking.
`AU4005` reports the exact same range violation for a dynamic request and
reports task-stack allocation or platform-size failure; neither path clamps or
falls back to the default.

`AU3008` is reserved by the Provisional Phase 5.6 contract for a value that
cannot cross a task or Queue boundary. The diagnostic identifies the boundary,
then names the nested field, element, or payload path to the non-transferable
leaf. Guidance recommends passing owned transferable data instead of a
capability view, or keeping a host resource or `random.Rng` on its owning task
and sending transferable input/output data. It never suggests implementing
`Transfer`, because there is no user implementation surface.

`AU3009` is different: the value has already passed the boundary, but a clone,
clone-producing collection read, or implicit aggregate copy would duplicate a
single-consumer task-result right. After a direct observation consumes that
right, a second use of the same task binding is ordinary moved-value `AU3001`;
attempting consumption through shared access is `AU3002`.

The runtime's atomic defense rejects a second claim of a non-repeatable result
with `AU4001`: `task result has already been observed; non-repeatable task
results allow exactly one observing attempt`. A correctly checked Aurora
program should be stopped earlier by the static ownership diagnostics.
For `select(...)`, `AU2004` reports an empty call or named source, `AU2002`
reports an invalid source or inconsistent Queue/Task category type, `AU3002`
reports a non-repeatable Task supplied without owned access, and `AU3009`
reports the same visible non-repeatable Task twice. Dynamic invalid deadlines
and runtime observation-claim failures remain `AU4001`.

## Backend Support

Structured groups, task targets and captures, Queue operations and iteration,
typed heterogeneous `select`, wait helpers, sleep, cancellation,
compiler-inserted loop safepoints, and user-trait dispatch on `Queue[T]`,
`Task[T]`, and `TaskGroup` for noncolliding method names are maintained on
both MIR execution and direct native generation.
Default and explicit guarded stack requests use the same scheduler allocation
path on both backends.
MIR checks each backedge and yields every 8 backedges. Native code uses 4,096
units of function-local fuel between yields when sibling tasks are possible
and elides the check when the program proves that no sibling task can exist.
Builtin handle member names
retain builtin dispatch on both backends. The scheduler/runtime surface and
complete diagnostics are parity-pinned. Both backends therefore share the
persistent reactor, timer heap, and direct runtime-event notification behavior.
MIR and direct-native traps capture the same typed Aurora call frames and task
ancestry once, before cleanup resets task-local state. Human output derives
call-chain and parent-task notes from those records, while JSON and the LSP
preserve the frame arrays directly.

## Limits And Implementation-Defined Behavior

Task execution is cooperative, pinned-worker, and non-preemptive. Loop
backedges have compiler-inserted scheduling checks, but one long loop body or
long straight-line computation can still delay siblings assigned to the same
worker. The checks do not inspect cancellation. Scheduling, independent task
completion, and output order are deliberately unspecified. The worker count
defaults to the available parallelism reported by the host and may be selected
provisionally with a positive `AURORA_WORKERS` value. Assignments never migrate
and work is not stolen.
Aurora exposes no worker-index or affinity-introspection API. Ordinary
lightweight tasks request 512 KiB of writable coroutine stack; an explicit
request is limited to 64 MiB. Requests are page-rounded and guard-protected.
The MIR/direct entry thread reserves
64 MiB. The scheduler
keeps descriptor registrations persistent and blocks until an event or
deadline when idle; it does not use a periodic readiness scan. Nested Aurora
calls stop at 256 frames. The process-wide blocking-I/O pool derives a default
of 2 through 8 host threads from host parallelism (fallback 4), or uses an
exact positive `AURORA_BLOCKING_WORKERS` value without clamping.
`AURORA_BLOCKING_QUEUE_CAPACITY` optionally bounds pending accepted jobs;
omitting it leaves the queue unbounded. The first runtime preflight reads this
configuration once without starting the pool, and the configuration remains
immutable for the process lifetime. First submission creates the complete
worker set; production reuses it until process exit and has no Aurora
shutdown/join surface. Non-repeatable transferable task results have one
statically enforced observation right. Cancelling after a blocking job is
accepted cannot retract an OS side effect. If the scheduler itself stops with
tasks still suspended, it disarms
their waits, publishes cancellation to their handles and observers, and
reclaims scheduler-owned and direct-runtime host state. That abandonment path
does not run arbitrary Aurora cleanup thunks; direct generated stacks may be
reset because they cannot safely be unwound through Cranelift frames. Detached
lightweight tasks are unavailable.

## Status

Scheduler-backed lightweight tasks, structured `TaskGroup`, generic task
handles and outcomes, bounded and unbounded queues, bare receive iteration,
sleep, cooperative cancellation, task-result observation, multi-task waits,
computed Duration arithmetic, and compiler-inserted loop-backedge safepoints
are implemented. Phase 5.1 adds persistent reactor registrations, heap-managed
deadlines, and direct Queue, task-completion, and blocking-pool wakeups; Phase
5.3 adds the automatic loop checks. Phase 5.4 moves deep HTTP, TLS, and
maintained Unix WebSocket library steps to a distinct bounded protocol service
with two named 2 MiB-stack workers and a 64-job queue, then makes ordinary
coroutine stacks guarded 512 KiB requests and adds the Accepted ADR-0032
override methods. HTTP URL/request/response construction, head parsing, and
chunk decoding; rustls construction, handshake, I/O, and close notification;
and Unix WebSocket construction, handshake, framing, and close run there.
Protocol state is owned by one bounded, nonblocking service step at a time and
is returned before the coroutine observes cancellation or resumes reactor
waiting. The process-global pool is initialized lazily, shared by all
lightweight schedulers, remains alive until process exit, and intentionally
has no 0.1 runtime shutdown/join API. Non-Unix WebSocket fallback retains its
compatibility path. Plain socket/reactor operations remain scheduler-side;
resolver, listener-bind, and file-read work uses the generic blocking-I/O pool.
TLS asset bytes are read there, while PEM parsing and rustls construction run
on protocol workers. Phase 5.4 also adds the bounded dynamic-`json.parse`
service and scheduler-aware admission described above; it does not move the
legacy JSON compatibility helpers. The host-timer policy recorded by ADR-0019
is Accepted. Phase 5.5 gives the scheduler driver unique mutable ownership,
routes nested starts through an owned internal broker, makes preparation
failure synchronous, and contains scheduler teardown across MIR and direct
tasks. Phase 5.7 makes Queue and Task handle state cross-worker safe and runs
task bodies on spawn-time pinned workers on both backends. This is a multicore
task-execution contract, not a guarantee of work stealing, preemption,
particular speedup, task/output order, or broader automatic parallelism.
Accepted ADR-0034 implements the typed heterogeneous
`select(source, ...)` builtin on both backends, using the shared persistent
wait machinery for atomic registration, deterministic one-winner arbitration,
cross-worker wakeups, and loser cleanup. It adds no statement syntax.
Preemptive scheduling, `mut` task targets, and detached task syntax are
unavailable. On the clean Mac14,9 Phase 5.10 measurement at `181204b`,
10,000 parked sleepers used 207,798,272 bytes of worst whole-process RSS and
198,787,072 bytes above their same-process pre-spawn baseline, passing the
512 MiB gate.

Aurora does not maintain a 100,000-sleeper claim. The final Phase 5.10
100,000-sleeper plus 1,000-timer repetitions peaked at 1,170,735,104,
1,921,531,904, and 2,001,305,600 bytes, so two of three exceeded the 1.5 GiB
gate. On this 16 KiB-page host, one resident page for each of the 101,000
stackful child coroutines alone requires 1,654,784,000 bytes before task
metadata or the root runtime. The earlier Phase 5.9 pass was therefore
compression- and reclaim-dependent rather than a robust capacity guarantee.
Under the ratified benchmark escape hatch, that result is retained as evidence
without becoming a product claim. The maintained scale claim is limited to
the contractual 10,000-sleeper bound plus the timer, idle, starvation, and
multicore controls, all of which pass at Phase 5.10. The four-worker control
has a `1.039673x` paired median wall-time ratio and `396.73%` median four-task
process CPU; these are measured Mac14,9 results, not portable speedup
guarantees.
The Queue capacity boundary is pinned by
`crates/aurora-compiler/tests/fixtures/run-fail/queue_zero_capacity.au` and
`crates/aurora-compiler/tests/fixtures/run-fail/queue_negative_capacity.au` on
both backends.

Accepted ADR-0033 specifies the implemented Phase 5.6 contract: structural
Transfer checks for task captures, task results, and Queue payloads, plus
static repeatable/single-consumer task results. Phase 5.7 retains that
share-nothing boundary while allowing Queue and Task handle identity to
communicate between pinned workers.
