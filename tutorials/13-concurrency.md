# Concurrency

Aurora's maintained concurrency surface is built around scheduler-backed lightweight tasks, structured task groups, typed queues, and explicit wait helpers. Queue waits, task waits, `sleep(...)`, socket waits, and the maintained HTTP helpers all share the same runtime scheduler.

The maintained user-facing model is:

- `Queue[T]()`
- `TaskGroup()`
- `TaskGroup.start(...) -> Task[T]`
- `TaskGroup.start_soon(...) -> None`
- `Task[T].result(timeout=...) -> TaskResult[T]`
- `Task[T].result_or_none(timeout=...) -> Option[T]`
- `Task[T].result_or(default, timeout=...) -> T`
- `Queue[T].get_or_none(timeout=...) -> Option[T]`
- `Queue[T].get_or(default, timeout=...) -> T`
- `wait_any(...)` and `wait_all(...)`

Aurora no longer exposes the older unstructured task forms. Every task belongs to a `TaskGroup`.

## Queues

A queue is a typed pipe for sending values between tasks:

```python
jobs = Queue[int32]()
```

Queues may also be bounded:

```python
jobs = Queue[int32](capacity=16)
```

With a bounded queue, `put(...)` waits until capacity is available instead of letting the queue grow without bound.

### Receiving Values

For ordinary code, use the convenience forms:

```python
print(jobs.get_or_none(timeout=100ms))
print(jobs.get_or(0, timeout=100ms))
```

Without a timeout, `get_or_none()` and `get_or(default)` are immediate non-blocking checks. They return `Option.None` or the fallback value when no item is ready yet.

Use `get(timeout=...)` when you need to distinguish all wait states. It returns `QueueReceive[T]`:

```python
match jobs.get():
    case QueueReceive.Item(value):
        print(value)
    case QueueReceive.Closed:
        print("closed")
    case QueueReceive.TimedOut:
        print("timed out")
    case QueueReceive.Cancelled:
        print("cancelled")
```

See [examples/concurrency/queue_timeout.au](../examples/concurrency/queue_timeout.au) and [examples/concurrency/queue_get_timeout_named.au](../examples/concurrency/queue_get_timeout_named.au).

### Sending Values

`put(value)` and `put(value, timeout=...)` return `Result[None, SendError[T]]`:

```python
match jobs.put(4, timeout=5ms):
    case Result.Ok(_):
        print("sent")
    case Result.Err(error):
        match error:
            case SendError.Closed(value):
                print(value)
            case SendError.Cancelled(value):
                print(value)
            case SendError.TimedOut(value):
                print(value)
            case SendError.Full(value):
                print(value)
```

`try_put(value)` is the non-blocking send form. It uses the same `SendError[T]` type.

See [examples/concurrency/queue_put_timeout.au](../examples/concurrency/queue_put_timeout.au), [examples/concurrency/bounded_queue.au](../examples/concurrency/bounded_queue.au), and [examples/concurrency/send_result.au](../examples/concurrency/send_result.au).

### Iterating Over A Queue

You can iterate over a queue until it is closed and empty:

```python
jobs = Queue[int32]()
jobs.put(1)
jobs.put(2)
jobs.close()

for job in jobs:
    print(job)
```

See [examples/concurrency/queue_iteration.au](../examples/concurrency/queue_iteration.au).

Queue handles are copy references. Passing the same queue into multiple tasks
shares the underlying queue without requiring `.clone()`.

The payload is checked separately. `Queue[T](...)`, `put(...)`, and
`try_put(...)` require `T` to be structurally `Transfer`: every stored field,
collection element, tuple element, or enum payload must ultimately be safe to
move to another task. `random.Rng`, `TaskGroup`, capability views, and live
file, process, or network resources are not `Transfer`. Queue receive and
handle-only operations do not duplicate or recheck a payload.

Queue iteration accepts only the bare form shown above. `for item in own
queue:` and `for item in mut queue:` are rejected because receiving already
delivers an owned item and the Queue handle itself is a copy value.

## Task Groups

Task groups tie child tasks to a lexical scope:

```python
with TaskGroup() as group:
    first = group.start(worker, jobs)
    second = group.start(worker, jobs)
    print(first.result_or(-1, timeout=50ms))
    print(second.result_or(-1, timeout=50ms))
```

When the `with` block ends, Aurora waits for child tasks to finish. If a child is still parked in a cancellation-aware wait with no deadline, Aurora cancels the group first so the scope can shut down cleanly instead of hanging forever. This keeps child work scoped to the parent block.

### Starting Tasks

Use `start(...)` when you need a handle:

```python
with TaskGroup() as group:
    task = group.start(producer, jobs)
```

Use `start_soon(...)` when you only need the side effect of starting the task:

```python
with TaskGroup() as group:
    group.start_soon(producer, jobs)
```

That is Aurora's maintained replacement for fire-and-forget task creation. Background work still belongs to a group, scope exit still waits for it, and unread task failures still surface when the group closes.

See [examples/concurrency/task_group_start.au](../examples/concurrency/task_group_start.au) and [examples/concurrency/task_group_start_soon.au](../examples/concurrency/task_group_start_soon.au).

All four start methods apply the same boundary before a task is scheduled:
every captured argument and the target's result must be structurally
`Transfer`. The compiler derives that property from the fully specialized
type; source code cannot declare or implement a `Transfer` trait. Copy data,
`String`, structurally transferable collections, tuples, classes, enums, and
Queue/Task handle identities can cross. Shared or mutable access,
`random.Rng`, `TaskGroup`, and live host resources cannot.

This is a static boundary established before multicore execution. Phase 5.7
must make the state behind Queue and Task handles cross-worker thread-safe
before more than one Aurora worker may use them.

A bare target parameter still grants shared access, but it borrows from the
child's owned capture rather than the caller's value. An `own` parameter may
consume that capture. A `mut` target remains invalid because there is no
caller-visible writeback.

Generic task targets must be concrete at the boundary. Inference and defaults
may provide the types, or the callable slot may use the narrow forms
`function[Types]` and `Type.associated_method[Types]`. Aurora rejects an
unresolved type parameter instead of assuming it can cross.

### Per-task Stack Overrides

`TaskGroup.start(...)` and `start_soon(...)` use Aurora's guarded 512 KiB
default task stack. A child with a measured task-local stack requirement can
request a custom capacity without changing its target arguments:

```python
with group = TaskGroup():
    task = group.start_with_stack(1024 * 1024, deep_worker, input)
    group.start_soon_with_stack(2 * 1024 * 1024, deep_sink, jobs)
```

Both size arguments have exact type `int64`. The accepted range is 262,144
through 67,108,864 bytes inclusive (256 KiB through 64 MiB). Aurora rejects
out-of-range requests rather than clamping them. Accepted requests are rounded
up to the host page size and protected by the platform stack allocator's guard
pages. Use the ordinary start methods unless a real workload demonstrates the
need for a custom capacity.

The lower 256 KiB bound is also available for an explicitly measured shallow
task, but it is not the generally safe default. Aurora's complete compiled
HTTP example faulted when 256 KiB was used as the global task default during
integration and succeeds with the 512 KiB default. A separate runtime-only
round trip succeeds with forced 256 KiB protocol callers because the deep
host frames execute on service workers; that narrower check does not include
the compiled program's MIR/direct execution frames.

Associated methods without `self` work too:

```python
class Worker:
    def run(value: int32) -> int32:
        return value + 1

with TaskGroup() as group:
    task = group.start(Worker.run, 4)
```

See [examples/concurrency/task_group_associated_method.au](../examples/concurrency/task_group_associated_method.au).

### Task Results

`Task[T]` is always a `Transfer` handle, but it is copyable only when `T` is
repeatable. Repeatable results are copy values, `Queue[...]` handles, and
recursively repeatable `Task[...]` handles. A task returning
`String`, `Vec[...]`, or another non-copy transferable value therefore has a
move-only task handle.

For a non-repeatable result, each of `result`, `result_or_none`, and
`result_or` consumes the task handle on its first attempt. Timeout,
cancellation, task failure, `Option.None`, and a fallback do not restore the
observation right. Use a repeatable result or a separate Queue protocol when a
program needs retries or fan-out.

Results that are not structurally `Transfer`, including `random.Rng` and live
host resources, are rejected at the task-start boundary with `AU3008`.
`AU3009` instead reports an attempted clone or collection copy that would
duplicate a valid single-consumer result right. A later use after direct
observation is the ordinary moved-value diagnostic `AU3001`.

For ordinary code, use:

```python
print(task.result_or_none(timeout=100ms))
print(task.result_or(-1, timeout=100ms))
```

These convenience forms map task failures to `Option.None` or the caller-provided fallback, alongside timeout and cancellation.

Without a timeout, `result_or_none()` and `result_or(default)` are immediate non-blocking checks. They return `Option.None` or the fallback value when the task is not ready yet.

Use `Task.result(timeout=...)` when you need to distinguish all wait states. It returns `TaskResult[T]`:

```python
match task.result():
    case TaskResult.Ready(value):
        print(value)
    case TaskResult.Error(message):
        print(message)
    case TaskResult.TimedOut:
        print("timed out")
    case TaskResult.Cancelled:
        print("cancelled")
```

## Waiting On Multiple Tasks

Use `wait_any(...)` and `wait_all(...)` instead of the removed `select` statement.

`wait_any(tasks, timeout=...)` returns `WaitAny[T]`:

```python
match wait_any(task_list, timeout=20ms):
    case WaitAny.Ready(index, value):
        print(index)
        print(value)
    case WaitAny.Error(index, message):
        print(index)
        print(message)
    case WaitAny.TimedOut:
        print("timedout")
    case WaitAny.Cancelled:
        print("cancelled")
```

`wait_any([])` returns `WaitAny.TimedOut` immediately.

`wait_all(tasks, timeout=...)` returns `WaitAll[T]`:

```python
match wait_all(task_list, timeout=20ms):
    case WaitAll.Ready(results):
        for result in results:
            print(result)
    case WaitAll.Error(index, message):
        print(index)
        print(message)
    case WaitAll.TimedOut:
        print("timedout")
    case WaitAll.Cancelled:
        print("cancelled")
```

For repeatable `T`, the task handles and observations remain reusable. For a
non-repeatable `T`, both helpers consume the complete `Vec[Task[T]]` on the
first attempt, including timeout, cancellation, and task failure.
`wait_any` deliberately abandons the observation rights of the tasks it did
not choose. Queue receive APIs always transfer one owned payload, but Queue
construction and sends admit only `Transfer` payloads.

See [examples/concurrency/task_group_wait_helpers.au](../examples/concurrency/task_group_wait_helpers.au).

## Cooperative Cancellation

Call `group.cancel()` to signal all tasks in the group to stop. Inside long-running task code, call `cancelled()` to observe the request:

```python
def worker(out: Queue[int32]):
    mut i: int32 = 0
    while i < 100:
        if cancelled():
            return
        out.put(i)
        i += 1
```

`sleep(...)` also wakes early when the group is cancelled, so task code after the sleep can call `cancelled()` and decide how to exit.

If the current `with TaskGroup()` scope is iterating a `Queue[T]` from that scope with `for value in queue:`, `group.cancel()` also wakes that queue iteration so it can finish cleanly instead of parking forever.

Cancellation is cooperative. Aurora does not forcibly kill tasks.

Aurora 0.1 runs task bodies on one cooperative scheduler thread, not in
parallel. The compiler inserts a cooperative scheduling check on every loop
backedge, including a normal body tail and `continue`, so a tight loop no
longer freezes ready timers, Queue operations, or socket work indefinitely.
`break` and `return` leave the loop without taking that check. Each ordinary
lightweight task requests a guarded 512 KiB coroutine stack; the explicit
stack-start methods accept requests through 64 MiB. Descriptor registrations
persist across waits, deadlines use a timer heap, and Queue, task-completion,
and blocking-pool events notify the scheduler directly. With nothing ready,
the scheduler blocks until an event or deadline rather than waking on a
periodic tick.

Deep HTTP, TLS, and maintained Unix WebSocket library steps run on a distinct
bounded protocol service with deep native worker stacks. Protocol state
returns to the lightweight task after each bounded, nonblocking step and
before cancellation or reactor waiting resumes. On the clean Mac14,9
measurement, 10,000 parked sleepers used 197,836,800 incremental bytes above
their same-process baseline, an amortized upper bound of 19,784 bytes
(19.32 KiB) per requested sleeper including scheduler metadata and shared
workload growth. The 100,000-sleeper plus 1,000-timer run passed its 3 ms timer
gates but reached 1,978,384,384 bytes worst RSS. Aurora makes no claim that
population fits in 1.5 GiB.

The protocol service starts lazily and lives until process exit; Aurora 0.1
does not expose a shutdown or join operation for it. File reads, resolver work,
and listener binding use the generic blocking-I/O pool. TLS asset bytes are
read there before PEM parsing and rustls construction run on protocol workers.

Blocking queue/task/network waits are cancellation-aware and surface cancellation through `QueueReceive`, `TaskResult`, `WaitAny`, `WaitAll`, or `io.Error`, depending on the API.

See [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au).

## Automatic Loop Safepoints

Loop safepoints make progress automatic at backedges; they do not make Aurora
preemptive. A single long iteration can still delay siblings until it reaches
the body tail, and long straight-line CPU work has no automatic checkpoint.
The safepoint also does not inspect cancellation. Keep calling `cancelled()`
when a task must stop on request.

MIR execution amortizes yielding with 8 units of function-local loop fuel.
Native concurrent programs use 4,096 units, while a program proven to have no
possible sibling task removes the runtime checks. Do not use these intervals
to predict output order: runnable-task selection and concurrent interleaving
remain unspecified.

## `yield_now`

Automatic safepoints are enough to keep a tight loop from starving the
scheduler indefinitely. Calling `yield_now()` between chosen bounded chunks
provides an explicit scheduling point sooner than the amortized native check
when the application wants one:

```python
def count(label: String):
    mut step: int32 = 1
    while step <= 3:
        print(f"{label}: {step}")
        step += 1
        yield_now()
```

The call returns `None` when the current task is scheduled again. It does not
sleep or guarantee that a different task runs, and runnable-task ordering is
not part of the language contract. It also does not inspect cancellation; call
`cancelled()` separately when cancellation matters.

See [examples/concurrency/yield_now.au](../examples/concurrency/yield_now.au).

## `sleep`

A simple delay:

```python
sleep(100ms)
```

Computed delays use the same signed Duration arithmetic as other expressions.
For example, a runtime attempt count can scale a base delay with
`attempt * 1ms`. A sleep or timeout must be non-negative and fit the host
deadline; invalid values fail instead of becoming unlimited waits.

See [examples/concurrency/sleep_builtin.au](../examples/concurrency/sleep_builtin.au).
For constructors, arithmetic, comparison, conversion, and sub-millisecond
rendering, see
[examples/concurrency/duration_arithmetic.au](../examples/concurrency/duration_arithmetic.au).

### Backoff Without Hidden Final Delays

Retry policy belongs in application code. The maintained
[retrying network worker](../examples/agents/retrying_network_worker.au) retries
only HTTP `503`, doubles a `Duration` backoff after each retry, and adds jitter
from `random.Rng(42)` so its trace is reproducible.

The worker checks both the response status and the final-attempt guard before
drawing randomness, printing a retry, or calling `sleep(...)`. Exhausting three
attempts therefore returns the last `503` immediately: there is no invisible
fourth attempt and no final delay. A terminal non-retryable status such as
`429` is returned immediately too.

The example places the loopback server and worker in one `TaskGroup`, gives
network and task waits explicit five-second deadlines, and scopes listeners,
exchanges, and responses with `with`. Its maintained CLI regression pins the
same seven-request trace through the MIR and forced-direct backends.

## Full Example

```python
def producer(out: Queue[int32]) -> int32:
    out.put(2)
    out.put(4)
    out.close()
    return 6

def main() -> int32:
    jobs = Queue[int32]()
    with TaskGroup() as group:
        task = group.start(producer, jobs)

        while true:
            match jobs.get_or_none(timeout=50ms):
                case Option.Some(value):
                    print(value)
                case Option.None:
                    break

        print(task.result_or(-1, timeout=50ms))
    return 0
```

See:

- [examples/concurrency/task_group_start.au](../examples/concurrency/task_group_start.au)
- [examples/concurrency/task_group_start_soon.au](../examples/concurrency/task_group_start_soon.au)
- [examples/concurrency/task_group_associated_method.au](../examples/concurrency/task_group_associated_method.au)
- [examples/concurrency/task_group_queue_sum.au](../examples/concurrency/task_group_queue_sum.au)
- [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au)
- [examples/concurrency/task_group_wait_helpers.au](../examples/concurrency/task_group_wait_helpers.au)
- [examples/concurrency/bounded_queue.au](../examples/concurrency/bounded_queue.au)
- [examples/concurrency/queue_timeout.au](../examples/concurrency/queue_timeout.au)
- [examples/concurrency/queue_put_timeout.au](../examples/concurrency/queue_put_timeout.au)
- [examples/concurrency/send_result.au](../examples/concurrency/send_result.au)
- [examples/agents/retrying_network_worker.au](../examples/agents/retrying_network_worker.au)

## Current Limits

The runtime is intentionally simple:

- queue waits, task waits, `sleep(...)`, socket waits, and HTTP waits all use the shared runtime scheduler
- the scheduler keeps descriptor registrations persistent, orders deadlines in
  a timer heap, receives direct Queue/task-completion/blocking-pool
  notifications, and blocks without a periodic idle tick
- cancellation is still cooperative rather than preemptive
- loop backedges include compiler-inserted cooperative scheduling checks, but
  a single long body can still delay siblings
- tasks are scheduler-backed lightweight coroutines rather than one-OS-thread-per-task workers
- task arguments are owned captures; bare shared and `own` target parameters
  are supported, while `mut` target parameters are rejected
