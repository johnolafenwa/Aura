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

Queue and task handles are cheap copy-like references. Passing the same queue into multiple tasks shares the underlying queue without requiring `.clone()` in the common case.

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

Repeated observation is supported for copy data and explicitly shared synchronized handles. A result containing an exclusive runtime resource is single-observer-only in Aurora 0.1. The checker does not enforce that restriction yet, so give such a result exactly one designated observer.

`random.Rng` is different: task observations clone the stored result, so
`Task.result`, `result_or_none`, and `result_or` reject a result containing an
`Rng` with `AU3007`. Copying the task handle remains valid.

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

`wait_any` and `wait_all` also clone successful stored results and require
clone-safe `T`. Queue receive APIs transfer ownership and can carry an `Rng`.

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

Aurora 0.1 runs task bodies on one cooperative scheduler thread, not in parallel. A CPU-bound task that never calls `cancelled()` or reaches another scheduler-aware operation can starve its siblings. Each lightweight task also reserves a fixed 1 MiB coroutine stack, and readiness checks scale linearly with the current waiting-task set.

Blocking queue/task/network waits are cancellation-aware and surface cancellation through `QueueReceive`, `TaskResult`, `WaitAny`, `WaitAll`, or `io.Error`, depending on the API.

See [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au).

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
- cancellation is still cooperative rather than preemptive
- tasks are scheduler-backed lightweight coroutines rather than one-OS-thread-per-task workers
- task arguments are owned captures; default/shared and `own` target
  parameters are supported, while `borrow mut` target parameters are rejected
