# Concurrency

Aurora's maintained concurrency surface is built around lightweight tasks, structured task groups, typed queues, and `select`. The runtime is still thread-based, but the primary user-facing model is:

- `Queue[T]` and `queue()`
- `spawn ...` for one-off tasks
- `with tasks() as group:` for structured child tasks
- `Task[T].result()` for waiting on a task result
- `Queue.get(timeout=...)` for ordinary timeout cases

Aurora now exposes only the maintained queue/task surface shown in this chapter.

## Queues

A queue is a typed pipe for sending values between tasks. Create one with `queue()` and an explicit type annotation:

```python
jobs: Queue[int32] = queue()
```

The bootstrap compiler still requires the expected type when you call bare `queue()`. If you want the type inline, use `queue[int32]()` or `Queue[int32]()`.

Send and receive:

```python
jobs: Queue[int32] = queue()
jobs.put(42)
msg = jobs.get()    # returns Option[int32]
```

`get()` returns `Option[T]`:

- `Option.Some(value)` when a value is available
- `Option.None` when the queue is closed and empty

`put(value)` returns `Result[None, SendError[T]]`:

- `Result.Ok(None)` on success
- `Result.Err(SendError.Closed(value))` when the queue is already closed, returning the unsent value

Close a queue with `jobs.close()`. After closing, no more values can be sent, but existing values can still be received.

Queue handles are cheap copy-like references. Passing the same queue into multiple tasks shares the underlying queue without requiring `.clone()` in the common case.

### Iterating Over A Queue

You can iterate over a queue with `for`. The loop runs until the queue is closed and empty:

```python
jobs: Queue[int32] = queue()
jobs.put(1)
jobs.put(2)
jobs.close()

for job in jobs:
    print(job)
```

See [examples/concurrency/queue_iteration.au](../examples/concurrency/queue_iteration.au).

### Timeout-Friendly Reads

For everyday timeout handling, prefer `get(timeout=...)` over `select`:

```python
match jobs.get(timeout=100ms):
    case Option.Some(value):
        print(value)
    case Option.None:
        print("timeout")
```

See [examples/concurrency/queue_timeout.au](../examples/concurrency/queue_timeout.au).

## Spawning Tasks

Use `spawn` with a named function call or an associated method without `self` to run work concurrently:

```python
def producer(out: Queue[int32]) -> int32:
    out.put(2)
    out.put(4)
    out.close()
    return 6

jobs: Queue[int32] = queue()
task = spawn producer(jobs)
```

`spawn` returns a `Task[T]`. Call `.result()` to wait for the task to complete and read its return value.

`Task[T]` also supports `.clone()` as a compatibility helper, but plain assignment and parameter passing are already cheap for task handles.

Associated methods without `self` work too:

```python
class Worker:
    def run(value: int32) -> int32:
        return value + 1

task = spawn Worker.run(4)
```

### Fire-And-Forget With `spawn detached`

Use `spawn detached` when you do not need a result handle:

```python
spawn detached producer(jobs)
```

Detached tasks do not return a `Task[T]`.

## Structured Task Groups

Task groups tie child tasks to a lexical scope using `with`:

```python
with tasks() as group:
    first = group.start(worker, jobs)
    second = group.start(worker, jobs)

    print(first.result())
    print(second.result())
```

When the `with` block ends, any still-running children are joined. This keeps child work scoped to the parent block.

### Cooperative Cancellation

Call `group.cancel()` to signal all tasks in the group to stop. Inside a task, call `cancelled()` to check whether cancellation was requested:

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

See [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au).

## `select`

`select` is still available for multi-arm coordination. It waits on multiple queue operations or a timer, executing whichever is ready first:

```python
select:
    case value = inbox.get():
        match value:
            case Option.Some(message):
                print(message)
            case Option.None:
                print("closed")
    case after(100ms):
        print("timeout")
```

### Supported Arms

- `case binding = queue.get():` -- receive and bind
- `case queue.get():` -- receive and discard
- `case binding = queue.put(value):` -- send and bind the result
- `case queue.put(value):` -- send and discard the result
- `case after(100ms):` -- timeout after a duration
- `case after(duration=100ms):` -- named form

When a `select` mixes `get()` arms with an `after(...)` arm, a closed-and-empty queue does not starve the timer. The timeout arm still fires as an escape path.

Duration literals include `ms` (milliseconds), `s` (seconds), and `m` (minutes).

## `sleep`

A simple blocking delay:

```python
sleep(100ms)
```

See [examples/concurrency/sleep_builtin.au](../examples/concurrency/sleep_builtin.au).

## Full Example: Producer-Consumer

```python
def producer(out: Queue[int32]) -> int32:
    out.put(2)
    out.put(4)
    out.close()
    return 6

def main() -> int32:
    jobs: Queue[int32] = queue()
    task = spawn producer(jobs)

    while true:
        match jobs.get():
            case Option.Some(value):
                print(value)
            case Option.None:
                break

    print(task.result())
    return 0
```

See:

- [examples/concurrency/queues_spawn.au](../examples/concurrency/queues_spawn.au)
- [examples/concurrency/queue_timeout.au](../examples/concurrency/queue_timeout.au)
- [examples/concurrency/send_result.au](../examples/concurrency/send_result.au)
- [examples/concurrency/spawn_detached.au](../examples/concurrency/spawn_detached.au)
- [examples/concurrency/select_send.au](../examples/concurrency/select_send.au)
- [examples/concurrency/task_group_select.au](../examples/concurrency/task_group_select.au)
- [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au)
- [examples/concurrency/select_timeout.au](../examples/concurrency/select_timeout.au)

## Current Limits

The bootstrap concurrency runtime is still intentionally simple:

- blocking queue and socket waits are not evented
- cancellation is still cooperative rather than preemptive
- detached-task ownership restrictions from the full proposal are not implemented yet
- borrowed spawn parameters are still rejected
