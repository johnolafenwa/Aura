# Concurrency

This page covers Aura's lightweight tasks. A task runs on a scheduler with
pinned workers. The surface includes structured task groups, task handles,
queues, cancellation checks, sleeping, and typed waits on one or several
sources.

Concurrency is structured by default. Child tasks should live inside a
`TaskGroup`, and leaving the group's scope waits for its children. Queue and
task waits go through the scheduler, so a blocked task does not block the
whole runtime.

## Workers And Scheduling

The runtime creates one pinned worker per unit of available parallelism that
the host reports. Set `AURA_WORKERS=<positive integer>` to choose an explicit
worker count. This override is provisional. A positive override may exceed the
host-reported default. `AURA_WORKERS=1` gives single-worker cooperative
execution through the same worker-thread architecture.

The runtime rejects an empty, zero, signed, whitespace-padded, nonnumeric, or
overflowing value with `AU4006` before execution starts.

Each child gets a stable worker assignment when it is spawned. Its coroutine
stack never migrates, and the runtime performs no work stealing. MIR execution
and direct native execution share this contract. MIR is the compiler's
mid-level intermediate representation, which `aura run` executes.

Aura 0.3 task scheduling is cooperative across pinned workers:

- The compiler inserts a scheduling check on every loop backedge. This
  includes the ordinary end of the loop body and `continue`. A tight loop
  therefore lets ready timers, Queue operations, and socket work on the same
  worker proceed.
- `break` and `return` leave the loop without taking that check.
- A single long loop body or long straight-line CPU work can still delay
  sibling tasks pinned to the same worker.
- The inserted check does not inspect cancellation. A task that must stop on
  request calls `cancelled()`.

Scheduler waits use a persistent event reactor. Descriptors stay registered,
deadlines live in a timer heap, and Queue, task-completion, and blocking-pool
events notify the responsible worker directly. A worker with no ready task
blocks until a notification, a descriptor event, or a deadline. It does not
wake on a periodic scheduler tick.

## Duration Values

Scheduler APIs take a `Duration`. This example covers every literal unit and
the computed operations:

```aura
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

A `Duration` is a signed copy value that counts i128 nanoseconds.

| Operation | Forms |
| --- | --- |
| Literals | `10ms`, `1s`, `2m` |
| Construction from an `int64` expression | `Duration.ms(value)`, `Duration.seconds(value)`, `Duration.minutes(value)` |
| Checked arithmetic | `+`, `-`, multiplication by an `int64` in either order, `// int64` |
| Comparison | all comparison operators |
| Conversion | `to_ms()`, `to_seconds()` |

Use the constructors when the count is a runtime `int64` rather than a
literal. The arithmetic covers computed backoff and deadline selection. For
example, a runtime attempt count can use `attempt * 1ms`.

`to_ms()` and `to_seconds()` convert the exact rational value to the nearest
representable IEEE-754 binary64 value, with ties to even. They may round.
Printing and f-string interpolation instead render the exact decimal
millisecond value with at most six fractional digits and an `ms` suffix.

A negative `Duration` is representable, but it is not a valid sleep, timeout,
or backoff. The scheduler APIs on this page have no `io.Error` or
`process.Error` carrier. A negative value, host-timer overflow, or deadline
overflow therefore traps with `AU4001`. Overflow never turns the operation
into an unlimited wait.

## TaskGroup

Construct a group with `TaskGroup()` and normally bind it with `with`:

```aura
with group = TaskGroup():
    task = group.start(work, 1)
```

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `TaskGroup()` | Creates a task group resource. |
| `start` | `start(function, own ...) -> Task[T]` | Requires every capture and the result to be `Transfer`. Starts the specialized target and returns its handle. |
| `start_soon` | `start_soon(function, own ...) -> None` | Requires every capture and the result to be `Transfer`. Starts the specialized target without returning a handle. |
| `start_with_stack` | `start_with_stack(bytes: int64, function, own ...) -> Task[T]` | Same `Transfer` rules, with an explicit guarded stack-capacity request. Returns the handle. |
| `start_soon_with_stack` | `start_soon_with_stack(bytes: int64, function, own ...) -> None` | Same `Transfer` rules, with an explicit guarded stack-capacity request. Returns no handle. |
| `cancel` | `cancel() -> None` | Signals cancellation to child tasks. |

`Transfer` is the compiler-derived property of a value that may cross a task
or Queue boundary. [Typing Rules](#typing-rules) lists which types have it.

### Task Targets

All four start methods accept these targets:

- **Capture-free function values**, which are copy values and satisfy
  `Transfer`.
- **Closure values** whose complete captured environment is `Transfer`.
- **Stored `TaskCallable[...]` values**, whose packing proved every capture
  is `Transfer`. See [Closures](/manual/closures#typing-rules).
- **Direct named functions and associated methods without `self`**,
  including explicit generic targets written as `function[Types]` or
  `Type.associated_method[Types]` in the callable slot.
- **Bound methods** such as `receiver.method`. A bound method is an ordinary
  closure target with its own call kind.

A stored target moves into the start for one child call, whatever its call
kind: Shared, Mutable, or Consuming. A Mutable target's captures become
child-owned state with no writeback to the parent.

The compiler rejects these targets:

- An ordinary erased `Callable` fails with `AU3008`, because its environment
  is hidden.
- A `TaskCallable[...]` contract that returns a view fails with `AU3008`. The
  child's result must be an owned value. A named function or function value
  whose result is `view ... from` a parameter is refused for the same reason.
- A `mut` target is rejected. Detached mutable capture has no writeback the
  caller could see.

Every target argument is copied or moved into task-owned capture storage. A
bare shared target parameter borrows from that storage for the child call. An
`own` parameter consumes it.

Every argument slot crosses the boundary with its declared type. A slot bound
by name or filled by an omitted default must be `Transfer` exactly like a
positional one, or the start fails with `AU3008`. The shared MIR validator
also refuses, on its own, any start whose contract carries a host resource, a
returned view, or a non-`Transfer` capture.

When a function-value contract keeps default availability, an omitted task
argument evaluates the default expression of the target selected at run time.
Task start follows the same default-binding rule as an ordinary indirect call.

### Task Stacks

`start` and `start_soon` request the 786,432-byte (768 KiB) default stack. The
two `_with_stack` methods take an exact `int64` byte count before the callable
target:

```aura
with group = TaskGroup():
    parser = group.start_with_stack(512 * 1024, parse_document, source)
    group.start_soon_with_stack(2 * 1024 * 1024, deep_worker, jobs)
```

- Accepted requests are 262,144 through 67,108,864 bytes inclusive, which is
  256 KiB through 64 MiB.
- A value outside that range is rejected, not clamped.
- An accepted request is rounded up to the host page size.
- The platform stack allocator adds guard-page protection. Guard pages are not
  part of the requested writable capacity.

The stack request uses separate method names rather than a keyword argument.
A keyword name could collide with the target's own arguments. The stack-size
methods are provisional.

The 256 KiB lower bound is an opt-in minimum for a task whose shallow stack
use you have measured. It is not a generally safe default. In testing, the
complete compiled Aura HTTP example faulted with 256 KiB as the global task
default and succeeded with a 512 KiB default. An isolated runtime-level HTTP
regression test does succeed when only its protocol-calling children use
256 KiB. That test shows that deep host protocol frames stay on the service
workers. It excludes the compiled program's MIR and direct language-execution
frames. Keep the default unless measurement of the complete task justifies a
custom size.

The MIR interpreter probes the running task's writable coroutine stack before
every Aura call:

- Headroom is measured from the first writable byte above the allocator's
  guard page, not from the start of the mapping. The guard page never counts
  as usable space.
- A call that would leave less than the interpreter's headroom reserve traps
  with `AU4005` ("task stack exhausted while calling ..."). The message names
  the callee and the writable bytes that remain. The trap happens before any
  frame can reach the guard page, so the task reports a diagnostic instead of
  faulting.
- The reserve exceeds the stack that one interpreter call transition uses.
  That cost depends on how the interpreter itself was compiled. An optimized
  build keeps 128 KiB in reserve. A debug-assertion build keeps 224 KiB,
  because an unoptimized interpreter spends roughly 170-190 KiB of stack per
  Aura call.
- Both reserves stay below the first-call headroom of the 256 KiB minimum, so
  that minimum can always run one call. How many nested calls a given capacity
  holds beyond that also depends on the build profile.

The root task that runs the program entry reserves a lazily mapped 16 MiB
coroutine stack and is probed the same way. Deep root recursion reports either
`AU4005` or the 256-call depth diagnostic, depending on the per-call cost.
Both are diagnostics, not faults. A program that starts no task runs its entry
on the 64 MiB runtime thread, where only the depth limit applies.

The direct backend has only its 256-call depth guard. It performs no headroom
probe and never reports the `AU4005` stack-exhaustion diagnostic.

### Scope Exit

On normal scope exit, the runtime joins children that keep making bounded
progress. It cancels a child in an unbounded group-owned wait only when the
live wait graph has no reachable waker.

A queue wait therefore stays joinable while another live task can send to,
receive from, or close that queue. The task performing the join does not count
as a waker for its own children.

A failure already observed through its `Task` result is not raised a second
time. An unread child failure aborts the group scope and wakes dependent queue
and task waits.

## Task[T]

`Task[T]` is a transferable handle to a child task's result. It is copyable
only when `T` is repeatable, so aliases cannot duplicate one result. `T` is
repeatable when it is copyable, a `Queue[...]` handle, or a recursively
repeatable `Task[...]`.

| API | Signature | Contract |
| --- | --- | --- |
| `result` | `result(timeout: Duration = ...) -> TaskResult[T]` | Waits for completion and returns a structured outcome. A non-repeatable `T` consumes the observation right on this call. |
| `poll` | `poll(timeout: Duration = ...) -> Poll[T]` | Returns `Poll.Ready(value)` on success and `Poll.Unavailable` on task failure, timeout, or cancellation. A non-repeatable `T` consumes the observation right even when `Unavailable` is returned. Without an explicit timeout, this is an immediate check. |
| `result_or` | `result_or(default: own T, timeout: Duration = ...) -> T` | Returns the task value, or `default` on task failure, timeout, or cancellation. A non-repeatable `T` consumes the observation right. Without an explicit timeout, this is an immediate check. |

`TaskResult[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Ready(value: own T)` | The task returned normally. |
| `Error(message: own str)` | The task failed with a runtime error. |
| `TimedOut` | The wait timed out. |
| `Cancelled` | The wait was interrupted by cancellation. |

Use `result` when the program must tell failure, timeout, and cancellation
apart. Use `poll` or `result_or` only when those outcomes mean the same thing
to the program. `Poll[T]` keeps a ready `None` payload distinct from
unavailability: `Poll.Ready(None)` differs from `Poll.Unavailable`.

The task stores its completed value. For a non-repeatable result, `result`,
`poll`, and `result_or` consume the unique observation right on any outcome.
The consumption is conservative. Timeout, cancellation, failure, and a
collapsed `Unavailable` do not restore it. `wait_any` and `wait_all` consume
the whole task list for such a `T`, and `wait_any` abandons the observation
rights it does not choose.

## Queue[T]

`Queue[T]` moves values between tasks. Queue handles are copy values.

```aura
jobs = Queue[str]()
bounded = Queue[str](capacity=8)
```

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `Queue[T](capacity: int32 = ...)` | Creates an unbounded queue when `capacity` is omitted, or a bounded queue for a positive capacity. Requires `T: Transfer`. A zero or negative capacity traps with `AU4001`. |
| `put` | `put(value: own T, timeout: Duration = ...) -> Result[None, SendError[T]]` | Sends a `Transfer` value, waiting for capacity when needed. Returns the unsent value in the error variant. |
| `try_put` | `try_put(value: own T) -> Result[None, SendError[T]]` | Tries to send a `Transfer` value without waiting. Returns `Full(value)` when a bounded queue is full. |
| `get` | `get(timeout: Duration = ...) -> QueueReceive[T]` | Receives one structured queue outcome. |
| `poll` | `poll(timeout: Duration = ...) -> Poll[T]` | Returns `Poll.Ready(value)` for an item and `Poll.Unavailable` for a closed, timed-out, or cancelled receive. A queued `None` item stays distinct from absence. Without an explicit timeout, this is an immediate check. |
| `get_or` | `get_or(default: own T, timeout: Duration = ...) -> T` | Returns an item, or `default` for a closed, timed-out, or cancelled receive. Without an explicit timeout, this is an immediate check. |
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

A `for` loop over a queue receives values:

```aura
for value in jobs:
    print(value)
```

- Every `Item(value)` arrives already owned by the loop binding.
- Only the bare form `for value in jobs` is accepted. The Queue handle is a
  copy value, so ownership modifiers have nothing to modify:
  `for value in own jobs` and `for value in mut jobs` are rejected.
- The loop evaluates and copies the Queue handle once, at loop entry. It does
  not freeze the source binding. The body may rebind `jobs`, but later
  receives keep using the captured handle rather than the newly bound Queue.
- The loop ends when the queue closes, when cancellation interrupts it, or
  when the relevant producers in the active task group complete. Closing
  queues explicitly is the clearest program shape.

## Top-Level Concurrency Builtins

| API | Signature | Contract |
| --- | --- | --- |
| `cancelled` | `cancelled() -> bool` | Returns `true` when the current task has been asked to cancel. |
| `yield_now` | `yield_now() -> None` | Yields the current lightweight task so other runnable work can proceed. |
| `sleep` | `sleep(duration: Duration) -> None` | Suspends the current task for at least `duration`, unless cancellation wakes it first. |
| `select` | `select(source, ...) -> SelectOutcome[Q, T]` | Waits on one or more positional `Queue[Q]`, `Task[T]`, or relative-`Duration` sources. Cancellation wins. Otherwise the lowest ready source index wins. |
| `wait_any` | `wait_any(tasks: list[Task[T]], timeout: Duration = ...) -> WaitAny[T]` | Waits for the first task outcome or the timeout. For a non-repeatable `T`, consumes the list and abandons the unchosen observation rights. `wait_any([])` returns `TimedOut` immediately. |
| `wait_all` | `wait_all(tasks: list[Task[T]], timeout: Duration = ...) -> WaitAll[T]` | Waits until every task is ready, one task errors, the timeout expires, or cancellation interrupts the wait. For a non-repeatable `T`, consumes the list. |

`WaitAny[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Ready(index: own int64, value: own T)` | Task at `index` returned normally. |
| `Error(index: own int64, message: own str)` | Task at `index` failed. |
| `TimedOut` | No task completed before the timeout. |
| `Cancelled` | Cancellation interrupted the wait. |

`WaitAll[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Ready(values: own list[T])` | Every task returned normally. Values are in the same order as the input tasks. |
| `Error(index: own int64, message: own str)` | Task at `index` failed before all tasks completed. |
| `TimedOut` | Not every task completed before the timeout. |
| `Cancelled` | Cancellation interrupted the wait. |

### Explicit Cooperative Yielding

`yield_now()` puts the current lightweight task back in the scheduler's ready
set. It returns `None` when the scheduler selects that task to run again.

The call gives other runnable tasks on the same pinned worker a chance to
proceed. It does not:

- migrate the task or search another worker for work
- guarantee that another task runs before it returns
- specify which runnable task is selected
- sleep, or wait for an event or deadline
- inspect or change cancellation state

With no current schedulable lightweight task, the call returns without effect.

Use `yield_now()` between bounded chunks of CPU work when you want an explicit
cooperative scheduling point. Call `cancelled()` separately when the task must
also respond to cancellation.

### Typed Heterogeneous Selection

`select(source, ...)` waits, without polling, on any positional mix of
`Queue[Q]`, `Task[T]`, and relative `Duration` sources:

```aura
def main() -> int32:
    messages = Queue[str]()
    messages.put("ready")
    print(select(messages, 0ms))
    return 0
```

- At least one source is required. Named arguments are rejected.
- All Queue sources in one call share one payload type `Q`. All Task sources
  share one result type `T`. The two categories are independent.
- An absent category is `None`. Selecting a `Queue[str]` with a deadline
  returns `SelectOutcome[str, None]`. Selecting a `Task[int32]` with a
  deadline returns `SelectOutcome[None, int32]`.

`SelectOutcome[Q, T]` variants:

| Variant | Meaning |
| --- | --- |
| `Queue(index: own int64, outcome: own QueueReceive[Q])` | The Queue at the original zero-based source index produced an item or closed outcome. |
| `Task(index: own int64, outcome: own TaskResult[T])` | The Task at the original zero-based source index produced a ready, error, or child-cancelled outcome. |
| `Deadline(index: own int64)` | The relative Duration at the original zero-based source index expired. |
| `Cancelled` | Cancellation of the selecting task interrupted the wait. |

Queue sources have no individual timeout, so `select` never produces
`QueueReceive.TimedOut`. Task sources likewise never produce
`TaskResult.TimedOut`. Cancellation of the selecting task uses the outer
`SelectOutcome.Cancelled`. A child task that was itself cancelled still
produces the nested `TaskResult.Cancelled` outcome.

**Evaluation.** Source expressions are evaluated exactly once, left to right.
All durations share one base instant, taken after evaluation and validation.
A zero duration is immediately ready. A negative or host-range-overflowing
duration traps with `AU4001`.

**Arbitration.** Cancellation of the current task has priority over every
source. Otherwise, when several sources are ready at the same arbitration
point, the lowest original argument index wins. A selected Queue gives up
exactly one item, and losing Queue sources stay unchanged. A closed Queue is
ready, and its buffered items are received before `Closed`.

**Task rights.** Selecting a repeatable Task leaves the handle reusable. Every
non-repeatable Task observation right is consumed at call entry, even when
another source wins. Losing rights are deliberately abandoned, as with
`wait_any`. Repeating the same non-repeatable Task in one call fails with
`AU3009`. Queue handles, repeatable Tasks, and Duration values may repeat, and
the lowest ready occurrence wins.

**Registration.** Selection uses one composite check, subscribe, and recheck
registration. It removes every losing registration before it returns, traps,
or propagates cancellation. Once a source is atomically claimed, that winner
is committed. A later cancellation or readiness event does not replace it.

Index priority is deterministic, not fair. A lower-index source that stays
ready can starve a higher-index source. Rotate the argument order between
calls when you need round-robin service.

## Cancellation Semantics

Cancellation is cooperative. `group.cancel()` marks child tasks as cancelled.
A task observes that state through:

- `cancelled()`, which is also a cooperative scheduler yield point
- `sleep(...)`
- queue send and receive waits
- task result waits
- `select(...)`
- `wait_any(...)` and `wait_all(...)`
- scheduler-aware process, network, and I/O waits where supported

`yield_now()` is a scheduling point, but it does not inspect cancellation.

A long CPU loop should check `cancelled()` directly:

```aura
while not cancelled():
    do_step()
```

Cancellation interrupts Aura's wait for scheduler-aware or worker-backed
operations. For the generic blocking-I/O pool, the acceptance boundary is
insertion into the pending job queue:

- Cancellation or deadline expiry while the caller still waits for admission
  prevents submission.
- After acceptance, Aura cannot forcibly stop the pending or running host
  operation. It runs once, may still perform its side effect, and any late
  result is discarded.
- A configured queue bound limits accepted pending work. It cannot guarantee
  progress for unrelated blocking I/O while all blocking workers stay stuck.

## Detached Work

Aura does not expose a `spawn detached` language form. Keep lightweight task
work under a `TaskGroup` so that scope exit gives a clear join and cleanup
boundary.

For operating-system child processes, use the `process` module. Decide
explicitly whether the child should be supervised, waited on, or closed.

## Grammar

Concurrency adds no `async`, `await`, or detached-spawn grammar:

- `TaskGroup`, `Task`, `Queue`, `yield_now`, `sleep`, `cancelled`, `select`,
  `wait_any`, and `wait_all` use ordinary construction and call syntax.
- Structured groups use the ordinary `with` statement.
- Stack overrides are ordinary member calls, not new task or spawn grammar.
- Queue iteration uses only `for item in queue:`.

[Lexical Structure](/manual/lexical-structure) defines Duration literal
spelling. [Grammar](/manual/grammar) has the statement and call productions.

## Typing Rules

`Queue[T]` is a copy handle. `Task[T]` is Copy only when `T` is repeatable.
`TaskGroup` is a managed move resource.

Queue sends, fallback values, task captures, and returned outcome payloads use
the exact owned positions shown in the API tables above. Timeout and capacity
expressions must have the documented exact types. A supplied Queue capacity
must be greater than zero.

**Task targets.** A target may be a capture-free function value or a
`Transfer` closure value. Direct named functions and associated methods
without `self` are also accepted. A generic target may infer every type
argument, or use explicit `function[Types]` or `Type.associated_method[Types]`
specialization in the callable slot. Brackets stay ordinary indexing
everywhere else. A bare target is accepted when its declarations and defaults
already resolve complete concrete types. Bare shared and `own` target
parameters are supported. `mut` targets are rejected.

**Stack capacity.** An explicit stack capacity must have exact type `int64`.
The callable argument that follows it and every capture keep the same typing
rules as an ordinary start.

**Transfer.** All four start methods place a structural `Transfer` obligation
on every captured argument and on the target's result. This obligation is
provisional.

- The check runs after generic specialization and before scheduling.
- A fully concrete generic call is checked after inference. An unresolved type
  parameter is rejected rather than becoming a deferred `Transfer` contract.
- The obligation applies to the owned capture even when the target declares a
  bare shared parameter and borrows that child-owned storage during its call.
- Queue construction, `put`, and `try_put` require `T: Transfer`. Handle
  copies, the receive and fallback methods, and `close` do not recheck the
  payload.
- Types that pass: Copy types, `str`, recursively transferable collections,
  tuples, classes, and enums, and `Queue` and `Task` handles.
- Types that fail: capability views, `random.Rng`, `TaskGroup`, and live host
  resources.
- `Transfer` is derived by the compiler. It is not a user trait.

Reading a Copy value through shared or mutable access for a task argument
captures an owned snapshot, not the access capability. That snapshot is
permitted when its type is `Transfer`. A non-copy access cannot be captured
this way, because the child would need ownership.

**Queue receive.** Queue iteration yields `T` by ownership transfer. The bare
form is accepted, and the `own` and `mut` modifiers are rejected. Receive
operations transfer one owned value and do not recheck payload `Transfer`.

**Selection.** Every `select` source must be exactly `Queue[Q]`, `Task[T]`, or
`Duration`. Queue payload types must agree on one `Q`, and Task result types
must agree on one `T`. A missing category is inferred as `None`. A
non-repeatable Task source is an owned observation and moves at call entry. A
repeatable Task and every Queue or Duration source is read without consuming
its source binding.

## Runtime Semantics

Tasks run on cooperative pinned workers, as described in
[Workers And Scheduling](#workers-and-scheduling). Starting a child stores its
captures in task-owned storage and gives it a stable worker assignment for its
whole lifetime.

**Group exit** observes or joins children, as described in
[Scope Exit](#scope-exit). It propagates an unread child failure. Host elapsed
time and machine load are not evidence that a wait is unreachable.

**Queues** transfer one value per send and receive, by copy or move according
to `T`. A bounded queue suspends senders when full. `close` wakes waiters.
Bare iteration receives repeatedly until one of its terminal conditions. A
nonpositive capacity traps before a queue is constructed.

**Outcomes.** Timeout, cancellation, closure, and task failure are distinct
enum outcomes. Scheduling order, completion order among independent tasks, and
program-output order are not specified.

**Selection** uses the same persistent wait machinery:

- One waiter subscribes to all Queue, Task, cancellation, and
  earliest-deadline sources. It rechecks them before parking and re-arbitrates
  in source order after a wake.
- Notifications do not choose the winner themselves.
- Committing a winner atomically consumes only that Queue item or selected
  Task result. It then idempotently removes every losing subscription.
- Selection creates no helper tasks, does not migrate the selecting task, and
  adds no periodic scheduler tick.

**Cross-worker communication.** Queue and Task handles are the maintained way
for tasks on different workers to communicate. Their runtime state is
synchronized, so a Queue operation or task completion can wake a task pinned
elsewhere. Every other captured argument and task result must be owned
`Transfer` data, which keeps a share-nothing boundary. Live host resources and
capability views stay on their owning task. Cancellation and diagnostic state
stay isolated per task. Running or trapping on one worker does not replace
another task's current cancellation or diagnostic context.

**Nested groups.** A running child may create a nested `TaskGroup`, start
grandchildren, and immediately wait on their returned handles, on both
backends. Child preparation allocates the guarded stack and task state before
the handle is returned. A preparation failure is synchronous and admits no
child. Successful nested starts pass through the scheduler's internal
admission broker rather than changing the scheduler through a second live
reference. The broker keeps requests in first-in, first-out order
internally, but ready-task and child execution order stay deliberately
unspecified.

**JSON parsing.** Dynamic `json.parse` uses a separate process-global codec
service with two workers, each with a 2 MiB stack, and a total in-flight
capacity of two.

- The runtime reserves one of those slots before it makes the fallible owned
  copy of the source.
- A lightweight task that finds the service saturated parks on a
  scheduler-aware availability notification rather than spinning.
- Once admitted, synchronous `json.parse` waits for the codec to finish.
  Cancellation is deferred to the task's next ordinary cancellation boundary.
- The bounded `json.is_valid` and `json.parse_string_map` operations run on the
  caller and do not use the service.
- The service is separate from the protocol and generic blocking-I/O pools and
  lives until process exit.

[Execution Model](/manual/execution-model) and [JSON Module](/manual/json)
have the remaining stack-safety and backend rules.

## Ownership And Evaluation Order

- Call arguments are evaluated before a task can use its captured values.
  Every non-copy capture moves into child-owned storage, and a copy capture is
  copied.
- For a stack override, the capacity expression is evaluated once, before the
  callable target and its captures.
- The child then borrows or consumes that storage according to the target's
  declared parameter mode, which does not change.
- `put` owns the value it offers and returns it inside `SendError` when no
  send occurs.
- Queue iteration captures the copyable handle once at loop entry and produces
  already-owned items. It never freezes or borrows the source binding.
- Observing a task result clones the stored value only when the result is
  repeatable. A non-repeatable result carries one statically enforced
  observation right, and no alias may produce a second value.
- A `select(...)` call evaluates all source expressions once, left to right.
  It copies Queue, repeatable Task, and Duration sources. It consumes every
  non-repeatable Task observation right at call entry and deliberately
  abandons any such right that loses.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU1101` | Malformed concurrency syntax, including unavailable spawn forms. |
| `AU2001` | Unknown concurrency type, function, or member. |
| `AU2002` | Generic, duration, capacity, task-list, stack-byte, argument, or outcome type mismatch. An out-of-range literal stack request, rejected during checking. An invalid `select` source or an inconsistent Queue or Task category type. |
| `AU2004` | Invalid constructor or method argument binding. An empty `select` call or a named `select` source. |
| `AU2006` | An explicit or inherited trait method collides with a builtin `Queue[T]`, `Task[T]`, or `TaskGroup` member. |
| `AU2999` | Unsupported targets, method-reference misuse, and remaining static concurrency rejections. |
| `AU3001` | Use after a value moves into task or queue storage. A second use of a task binding after a direct observation consumed its right. |
| `AU3002` | Invalid borrowed capture or storage use. The rejected `mut` task target. Consuming a task-result right through shared access. A non-repeatable Task passed to `select` without owned access. |
| `AU3003` | A mutating call through an immutable place. |
| `AU3004` | The forbidden `own` or `mut` modifier on Queue iteration. |
| `AU3007` | A task-result or multi-task observation whose value contains, or may contain, non-cloneable `random.Rng` state. |
| `AU3008` | A value that cannot cross a task or Queue boundary. |
| `AU3009` | A clone, clone-producing collection read, or implicit aggregate copy that would duplicate a single-consumer task-result right. The same visible non-repeatable Task passed twice to `select`. |
| `AU4001` | General runtime trap. Includes zero or negative Queue capacity. Also a negative, unrepresentable, or overflowing scheduler deadline, and a second claim of a non-repeatable task result. |
| `AU4002` | Arithmetic overflow or underflow. |
| `AU4003` | A bounds or lookup violation. |
| `AU4004` | A zero divisor. |
| `AU4005` | A resource or I/O failure. Includes task stack exhaustion, a dynamic stack request outside the accepted range, and task-stack allocation or platform-size failure. |
| `AU4006` | Invalid pinned-worker or blocking-I/O runtime configuration. |

Timeout, cancellation, closure, fullness, and an observed task error are typed
values, not diagnostics. An unread child trap keeps its original code.

**Deadlines.** Scheduler deadlines trap with `AU4001` because these APIs have
no typed InvalidInput carrier.

**Stack requests.** A dynamic stack request outside the range reports the same
range violation as the literal check, but with `AU4005`. Neither path clamps
or falls back to the default.

**Runtime configuration.** `AURA_WORKERS`, `AURA_BLOCKING_WORKERS`, and
`AURA_BLOCKING_QUEUE_CAPACITY` each require a positive decimal integer. The
`AU4006` diagnostic names the setting, shows the invalid value, and is issued
before user code runs. A non-Unicode value is displayed lossily.

**AU3008.** The diagnostic identifies the boundary, then names the nested
field, element, or payload path that leads to the non-transferable leaf. To
fix it, pass owned transferable data instead of a capability view. Keep a host
resource or `random.Rng` on its owning task and send transferable input and
output data instead. The guidance never suggests implementing `Transfer`,
because there is no user implementation surface. This diagnostic belongs to
the provisional `Transfer` contract.

**AU3009 versus AU3001.** `AU3009` means the value has already passed the
boundary, but a copy would duplicate its single-consumer right. After a direct
observation consumes that right, a second use of the same task binding is an
ordinary moved-value `AU3001`.

**Second observation.** The runtime's atomic defense rejects a second claim of
a non-repeatable result with `AU4001`: `task result has already been observed; non-repeatable task results allow exactly one observing attempt`.
The static ownership diagnostics should stop a correctly checked Aura program
earlier. For `select(...)`, dynamic invalid deadlines and runtime
observation-claim failures also stay `AU4001`.

## Backend Support

Both MIR execution and direct native generation support:

- structured groups, task targets, and captures
- Queue operations and iteration
- typed heterogeneous `select`
- wait helpers, sleep, and cancellation
- compiler-inserted loop safepoints
- user-trait dispatch on `Queue[T]`, `Task[T]`, and `TaskGroup` for method
  names that do not collide with builtin members

Builtin handle member names keep builtin dispatch on both backends. Default
and explicit guarded stack requests use the same scheduler allocation path on
both backends.

The loop safepoints differ in frequency. MIR checks each backedge and yields
every 8 backedges. Native code uses 4,096 units of function-local fuel between
yields when sibling tasks are possible. It skips the check when the program
proves that no sibling task can exist.

The scheduler and runtime surface and the complete diagnostics are pinned by
parity tests. Both backends therefore share the persistent reactor, the timer
heap, and direct runtime-event notification.

MIR and direct-native traps capture the same typed Aura call frames and task
ancestry once, before cleanup resets task-local state. Human output derives
call-chain and parent-task notes from those records. JSON output and the
language server keep the frame arrays directly.

## Limits And Implementation-Defined Behavior

**Scheduling.** Task execution is cooperative, pinned-worker, and
non-preemptive. [Workers And Scheduling](#workers-and-scheduling) covers the
worker count, loop checks, and idle behavior. Scheduling, independent task
completion, and output order are deliberately unspecified. Aura exposes no
worker-index or affinity-introspection API.

**Stacks and depth.** Ordinary lightweight tasks request 768 KiB of writable
coroutine stack. An explicit request is limited to 64 MiB. Requests are
page-rounded and guard-protected. The MIR and direct entry thread reserves
64 MiB. Nested Aura calls stop at 256 frames.

**Blocking-I/O pool.** The process-wide pool works as follows:

- It derives a default of 2 through 8 host threads from host parallelism, with
  a fallback of 4.
- An exact positive `AURA_BLOCKING_WORKERS` value sets the count without
  clamping.
- `AURA_BLOCKING_QUEUE_CAPACITY` optionally bounds pending accepted jobs.
  Without it, the queue is unbounded.
- The first runtime preflight reads this configuration once without starting
  the pool. The configuration stays fixed for the process lifetime.
- The first submission creates the complete worker set. Production reuses it
  until process exit, and Aura has no shutdown or join surface for it.
- Cancelling after a blocking job is accepted cannot retract an operating
  system side effect.

**Scheduler teardown.** If the scheduler itself stops with tasks still
suspended, it disarms their waits, publishes cancellation to their handles and
observers, and reclaims scheduler-owned and direct-runtime host state. This
abandonment path does not run arbitrary Aura cleanup thunks. Direct generated
stacks may be reset, because they cannot safely be unwound through Cranelift
frames.

**Detached tasks.** Detached lightweight tasks are unavailable.

## Status

These features are implemented:

- scheduler-backed lightweight tasks and structured `TaskGroup`
- generic task handles and outcomes
- bounded and unbounded queues, and bare receive iteration
- sleep and cooperative cancellation
- task-result observation and multi-task waits
- computed Duration arithmetic
- compiler-inserted loop-backedge safepoints
- persistent reactor registrations, heap-managed deadlines, and direct Queue,
  task-completion, and blocking-pool wakeups
- guarded 768 KiB coroutine stacks and the stack-override methods
- typed heterogeneous `select(source, ...)`, with atomic registration,
  deterministic one-winner arbitration, cross-worker wakeups, and loser
  cleanup. It adds no statement syntax.

Task execution is multicore. Queue and Task handle state is safe across
workers, and task bodies run on the worker pinned at spawn time on both
backends. Speedup depends on the workload. The share-nothing `Transfer`
boundary holds while Queue and Task handles communicate between workers.

The `Transfer` contract covers structural checks for task captures, task
results, and Queue payloads, plus static repeatable or single-consumer task
results.

The scheduler driver has unique mutable ownership. Nested starts go through an
owned internal broker, preparation failure is synchronous, and scheduler
teardown is contained across MIR and direct tasks.

**Protocol service.** Deep HTTP, TLS, and maintained Unix WebSocket library
steps run on a separate bounded protocol service. The service has two named
workers with 2 MiB stacks and a 64-job queue. These steps run there:

- HTTP URL, request, and response construction, head parsing, and chunk
  decoding
- rustls construction, handshake, I/O, and close notification
- Unix WebSocket construction, handshake, framing, and close

One bounded, nonblocking service step at a time owns the protocol state. The
state is returned before the coroutine observes cancellation or resumes
reactor waiting. The process-global pool starts lazily, is shared by all
lightweight schedulers, and lives until process exit. It intentionally has no
0.3 runtime shutdown or join API. The non-Unix WebSocket fallback keeps its
compatibility path.

Plain socket and reactor operations stay on the scheduler side. Resolver,
listener-bind, and file-read work uses the generic blocking-I/O pool. TLS asset
bytes are read there, while PEM parsing and rustls construction run on
protocol workers. The dynamic `json.parse` service and its scheduler-aware
admission are described under [Runtime Semantics](#runtime-semantics). The
JSON flat-dictionary operations stay on the caller.

**Unavailable.** Preemptive scheduling, work stealing, `mut` task targets, and
detached task syntax are unavailable.

**Memory measurements.** These results come from a clean Mac14,9 measurement
at `181204b`:

- 10,000 parked sleepers used 207,798,272 bytes of worst whole-process
  resident set size, or RSS. That is 198,787,072 bytes above their same-process
  pre-spawn baseline, which passes the 512 MiB gate.
- The runtime accepts larger task counts, but 10,000 sleepers is the
  maintained memory-capacity bound.
- Three repetitions of 100,000 sleepers plus 1,000 timers peaked at
  1,170,735,104, 1,921,531,904, and 2,001,305,600 bytes. Two of the three
  exceeded the 1.5 GiB gate.
- On this 16 KiB-page host, one resident page for each of the 101,000
  stackful child coroutines alone needs 1,654,784,000 bytes, before task
  metadata or the root runtime. An earlier run that passed this gate depended
  on memory compression and reclaim behavior.
- The contractual 10,000-sleeper bound and the timer, idle, starvation, and
  multicore controls all pass.
- The four-worker control has a `1.039673x` paired median wall-time ratio and
  `396.73%` median four-task process CPU on the measured Mac14,9 host.

**Tests.** The Queue capacity boundary is pinned on both backends by
`crates/aura-compiler/tests/fixtures/run-fail/queue_zero_capacity.au` and
`crates/aura-compiler/tests/fixtures/run-fail/queue_negative_capacity.au`.

**Design record.** The rationale for this page is in these decision records:

- `architecture_docs/decisions/0006-parameter-and-loop-ownership-defaults.md`:
  Queue receive ownership and the loop-modifier carve-out.
- `architecture_docs/decisions/0017-iteration-source-selection.md`: when Queue
  iteration captures its source.
- `architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md`:
  Duration conversion and host-timer classification. Accepted.
- `architecture_docs/decisions/0032-guarded-lightweight-task-stacks.md`:
  guarded task stacks and the stack-override methods. Accepted.
- `architecture_docs/decisions/0033-structural-transfer-and-task-results.md`:
  structural `Transfer` and repeatable task results. Accepted.
- `architecture_docs/decisions/0034-typed-heterogeneous-select.md`: typed
  heterogeneous `select`. Accepted.
