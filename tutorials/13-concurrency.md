# Concurrency

Aurora's maintained concurrency surface is built around scheduler-backed lightweight tasks, structured task groups, typed queues, and explicit wait helpers. Queue waits, task waits, `sleep(...)`, socket waits, and the maintained HTTP helpers all share the same runtime scheduler.

The maintained user-facing model is:

- `Queue[T]()`
- `TaskGroup()`
- `TaskGroup.start(...) -> Task[T]`
- `TaskGroup.start_soon(...) -> None`
- `Task[T].result(timeout=...) -> TaskResult[T]`
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

`get()` returns `QueueReceive[T]`:

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

The variants are:

- `QueueReceive.Item(value)` when a value is available
- `QueueReceive.Closed` when the queue is closed and empty
- `QueueReceive.TimedOut` when a timeout expires
- `QueueReceive.Cancelled` when task-group cancellation interrupts the wait

For ordinary timeout handling, use `get(timeout=...)`:

```python
match jobs.get(timeout=100ms):
    case QueueReceive.Item(value):
        print(value)
    case QueueReceive.TimedOut:
        print("timeout")
    case _:
        pass
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

    match first.result():
        case TaskResult.Ready(value):
            print(value)
        case _:
            pass
```

When the `with` block ends, any still-running child tasks are joined. This keeps child work scoped to the parent block.

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

That is Aurora's maintained replacement for fire-and-forget task creation. Background work still belongs to a group and still shuts down with the surrounding scope.

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

`Task.result()` returns `TaskResult[T]`:

```python
match task.result():
    case TaskResult.Ready(value):
        print(value)
    case TaskResult.TimedOut:
        print("timed out")
    case TaskResult.Cancelled:
        print("cancelled")
```

`Task.result(timeout=...)` adds a timeout-aware wait without changing the result type.

## Waiting On Multiple Tasks

Use `wait_any(...)` and `wait_all(...)` instead of the removed `select` statement.

`wait_any(tasks, timeout=...)` returns `WaitAny[T]`:

```python
match wait_any(task_list, timeout=20ms):
    case WaitAny.Ready(index, value):
        print(index)
        print(value)
    case WaitAny.TimedOut:
        print("timedout")
    case WaitAny.Cancelled:
        print("cancelled")
```

`wait_all(tasks, timeout=...)` returns `WaitAll[T]`:

```python
match wait_all(task_list, timeout=20ms):
    case WaitAll.Ready(results):
        for result in results:
            print(result)
    case WaitAll.TimedOut:
        print("timedout")
    case WaitAll.Cancelled:
        print("cancelled")
```

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

Cancellation is cooperative. Aurora does not forcibly kill tasks.

Blocking queue/task/network waits are cancellation-aware and surface cancellation through `QueueReceive`, `TaskResult`, `WaitAny`, `WaitAll`, or `io.Error`, depending on the API.

See [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au).

## `sleep`

A simple delay:

```python
sleep(100ms)
```

See [examples/concurrency/sleep_builtin.au](../examples/concurrency/sleep_builtin.au).

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
            match jobs.get():
                case QueueReceive.Item(value):
                    print(value)
                case QueueReceive.Closed:
                    break
                case _:
                    pass

        match task.result():
            case TaskResult.Ready(total):
                print(total)
            case _:
                return 1
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

## Current Limits

The runtime is intentionally simple:

- queue waits, task waits, `sleep(...)`, socket waits, and HTTP waits all use the shared runtime scheduler
- cancellation is still cooperative rather than preemptive
- tasks are scheduler-backed lightweight coroutines rather than one-OS-thread-per-task workers
- borrowed task parameters are still rejected
