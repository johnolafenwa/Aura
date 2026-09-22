# Concurrency

Aura runs concurrent code as lightweight tasks. Every task belongs to a `TaskGroup`, and tasks pass values to each other through typed queues. This chapter covers queues, task groups, task results, waiting on several sources, cancellation, and how the scheduler runs tasks.

The concurrency API:

| API | Returns |
| --- | --- |
| `Queue[T]()` | a new queue |
| `TaskGroup()` | a new task group |
| `TaskGroup.start(...)` | `Task[T]` |
| `TaskGroup.start_soon(...)` | `None` |
| `Task[T].result(timeout=...)` | `TaskResult[T]` |
| `Task[T].poll(timeout=...)` | `Poll[T]` |
| `Task[T].result_or(default, timeout=...)` | `T` |
| `Queue[T].poll(timeout=...)` | `Poll[T]` |
| `Queue[T].get_or(default, timeout=...)` | `T` |
| `select(queue_or_task_or_duration, ...)` | `SelectOutcome[Q, T]` |
| `wait_any(...)` and `wait_all(...)` | see [Waiting On Multiple Tasks](#waiting-on-multiple-tasks) |

Queue waits, task waits, `sleep(...)`, socket waits, and the HTTP helpers all run on the same scheduler. The scheduler uses pinned workers: each task stays on one worker for its whole life.

## Queues

A queue is a typed pipe that carries values between tasks:

```aura check-pass
jobs = Queue[int32]()
```

A queue can have a capacity bound:

```aura check-pass
jobs = Queue[int32](capacity=16)
```

On a bounded queue, `put(...)` waits until there is room. The queue never grows past its bound.

Queue handles are copy references. Passing one queue to several tasks shares the same underlying queue, with no `.clone()` needed.

The payload type is checked separately. `Queue[T](...)`, `put(...)`, and `try_put(...)` require `T` to be structurally `Transfer`. That means every stored field, collection element, tuple element, and enum payload must be safe to move to another task. These are not `Transfer`:

- `random.Rng`
- `TaskGroup`
- capability views
- live file, process, or network resources

Receiving and handle-only operations do not copy or recheck a payload.

### Receiving Values

For most code, use the convenience forms:

```aura fragment
print(jobs.poll(timeout=100ms))
print(jobs.get_or(0, timeout=100ms))
```

A ready item is `Poll.Ready(value)`. When no item is ready, `poll()` returns `Poll.Unavailable` and `get_or(default)` returns the fallback. A queued `None` item therefore stays distinct from an empty queue. Without a timeout, both forms check once and return immediately.

To tell every wait state apart, use `get(timeout=...)`. It returns `QueueReceive[T]`:

```aura fragment
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

```aura fragment
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

`try_put(value)` sends without waiting. It returns the same `SendError[T]` type. [10-results-and-options.md](10-results-and-options.md) lists which call returns each variant.

See [examples/concurrency/queue_put_timeout.au](../examples/concurrency/queue_put_timeout.au), [examples/concurrency/bounded_queue.au](../examples/concurrency/bounded_queue.au), and [examples/concurrency/send_result.au](../examples/concurrency/send_result.au).

### Iterating Over A Queue

A `for` loop over a queue runs until the queue is closed and empty. This prints `1` and `2`:

```aura check-pass
jobs = Queue[int32]()
jobs.put(1)
jobs.put(2)
jobs.close()

for job in jobs:
    print(job)
```

Only the bare form `for item in queue:` is allowed. The checker rejects `for item in own queue:` and `for item in mut queue:`. Receiving already delivers an owned item, and the queue handle is a copy value.

See [examples/concurrency/queue_iteration.au](../examples/concurrency/queue_iteration.au).

## Task Groups

A task group ties child tasks to a lexical scope:

```aura fragment
with TaskGroup() as group:
    first = group.start(worker, jobs)
    second = group.start(worker, jobs)
    print(first.result_or(-1, timeout=50ms))
    print(second.result_or(-1, timeout=50ms))
```

When the `with` block ends, Aura waits for the child tasks to finish. A wait with no deadline is cancelled only when no live task can wake it. Elapsed time and host load never make a reachable queue wait count as a deadlock. So a true deadlock shuts down cleanly, while ordinary producer and consumer backpressure stays inside the parent block.

### Starting Tasks

Use `start(...)` when you need a handle to the task:

```aura fragment
with TaskGroup() as group:
    task = group.start(producer, jobs)
```

Use `start_soon(...)` when you only need the task to run:

```aura fragment
with TaskGroup() as group:
    group.start_soon(producer, jobs)
```

Every started task belongs to its group, and scope exit waits for it. If a task fails and nothing reads its result, the failure surfaces when the group closes. This applies to `start_soon(...)` too.

See [examples/concurrency/task_group_start.au](../examples/concurrency/task_group_start.au) and [examples/concurrency/task_group_start_soon.au](../examples/concurrency/task_group_start_soon.au).

A target can be an associated method without `self`:

```aura check-pass
class Worker:
    def run(value: int32) -> int32:
        return value + 1

with TaskGroup() as group:
    task = group.start(Worker.run, 4)
```

See [examples/concurrency/task_group_associated_method.au](../examples/concurrency/task_group_associated_method.au).

All four start methods check the same boundary before they schedule a task. The four are `start`, `start_soon`, and the two stack-size variants below. Every captured argument and the target's result must be structurally `Transfer`.

- The compiler derives `Transfer` from the fully specialized type. Source code cannot declare or implement a `Transfer` trait.
- These can cross: copy data, `str`, transferable collections, tuples, classes, enums, and Queue and Task handles.
- These cannot cross: shared or mutable access, `random.Rng`, `TaskGroup`, and live host resources.

This compile-time check is Aura's share-nothing rule for multicore execution. Queue and Task handles are the cross-worker channels, and their state is synchronized. Every other capture and result crosses as owned `Transfer` data. Cancellation and diagnostic context stay separate for each task.

Target parameters follow these rules:

- A bare parameter grants shared access. It borrows from the child's owned capture, never from the caller's value.
- An `own` parameter may consume the capture.
- A `mut` parameter is rejected, because no change could be written back to the caller.

A generic target must be concrete at the task boundary. Inference and defaults can supply the types. The callable slot can also use the forms `function[Types]` and `Type.associated_method[Types]`. Aura rejects an unresolved type parameter at the boundary.

### Per-task Stack Overrides

`TaskGroup.start(...)` and `start_soon(...)` give each task Aura's default 768 KiB guarded stack. A child whose measured stack use needs more can request a custom size without changing its target arguments:

```aura fragment
with group = TaskGroup():
    task = group.start_with_stack(1024 * 1024, deep_worker, input)
    group.start_soon_with_stack(2 * 1024 * 1024, deep_sink, jobs)
```

The size rules:

- The size argument has exact type `int64`.
- The accepted range is 262,144 through 67,108,864 bytes inclusive, which is 256 KiB through 64 MiB.
- Aura rejects a size outside the range. It never clamps it.
- An accepted size is rounded up to the host page size, and the platform stack allocator protects it with guard pages.

Use the ordinary start methods unless a real workload shows that it needs a custom size.

The 256 KiB minimum is available for a shallow task whose stack use you have measured. It is not a safe general default. During integration, Aura's complete compiled HTTP example faulted with 256 KiB as the global task default and succeeded with 512 KiB. A separate runtime-only round trip succeeds with forced 256 KiB protocol callers, because the deep host frames run on service workers. That narrower check does not cover the compiled program's MIR or direct execution frames.

### Task Results

A task's result can be read once or many times, depending on its type. A result type is repeatable when it is:

- a copy value
- a `Queue[...]` handle
- a `Task[...]` handle whose own result is repeatable

`Task[T]` is always a `Transfer` handle, but it is copyable only when `T` is repeatable. A task that returns `str`, `list[...]`, or another non-copy value has a move-only handle.

For a non-repeatable result, the first call to `result`, `poll`, or `result_or` consumes the handle. That holds even when the call ends in a timeout, a cancellation, a task failure, `Poll.Unavailable`, or a fallback. The right to observe the result is not restored. If a program needs retries or fan-out, use a repeatable result or send the value through a Queue.

| Code | Cause |
| --- | --- |
| `AU3008` | The task's result is not structurally `Transfer`, for example `random.Rng` or a live host resource. Reported at the task-start boundary. |
| `AU3009` | A clone or collection copy would duplicate the single-consumer right to a result. |
| `AU3001` | The handle is used again after a call already observed its result. This is the ordinary moved-value diagnostic. |

For most code, use the convenience forms:

```aura fragment
print(task.poll(timeout=100ms))
print(task.result_or(-1, timeout=100ms))
```

A finished task gives `Poll.Ready(value)`. A task failure, a timeout, or a cancellation gives `Poll.Unavailable` or the fallback you passed. Without a timeout, both forms check once and return immediately.

To tell every wait state apart, use `Task.result(timeout=...)`. It returns `TaskResult[T]`:

```aura fragment
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

Use the builtin `select(...)` to wait on a mix of queues, tasks, and a relative deadline. It is an ordinary variadic call. There is no `select` statement.

```aura fragment
outcome = select(messages, worker_task, 20ms)

match own outcome:
    case SelectOutcome.Queue(index, received):
        print(index)
        match own received:
            case QueueReceive.Item(message):
                print(message)
            case _:
                pass
    case SelectOutcome.Task(index, result):
        print(index)
        match own result:
            case TaskResult.Ready(value):
                print(value)
            case _:
                pass
    case SelectOutcome.Deadline(index):
        print(index)
    case SelectOutcome.Cancelled:
        print("cancelled")
```

The `select` rules:

- All Queue sources share one payload type, and all Task sources share one result type.
- If a call has no Queue sources or no Task sources, that parameter of `SelectOutcome[Q, T]` is `None`.
- Source expressions are evaluated once, left to right.
- Cancellation wins. Otherwise, when several sources are ready together, the lowest argument index wins.
- A selected Queue gives up one item. Queues that lose are unchanged.
- Every non-repeatable Task's observation right is consumed when the call starts, even when a Queue or the deadline wins.

For an existing `list[Task[T]]` of one result type, use `wait_any(...)` or `wait_all(...)`.

`wait_any(tasks, timeout=...)` returns `WaitAny[T]`:

```aura fragment
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

```aura fragment
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

When `T` is repeatable, the task handles and their results stay reusable. When `T` is not repeatable, both helpers consume the whole `list[Task[T]]` on the first attempt. That includes an attempt that ends in a timeout, a cancellation, or a task failure. `wait_any` gives up the observation rights of the tasks it did not choose.

See [examples/concurrency/task_group_wait_helpers.au](../examples/concurrency/task_group_wait_helpers.au).

## Cooperative Cancellation

Call `group.cancel()` to ask every task in the group to stop. A long-running task calls `cancelled()` to check for the request:

```aura check-pass
def worker(out: Queue[int32]):
    mut i: int32 = 0
    while i < 100:
        if cancelled():
            return
        out.put(i)
        i += 1
```

Cancellation is cooperative. Aura does not forcibly kill tasks. These operations respond to it:

- `sleep(...)` wakes early when the group is cancelled. The code after it can call `cancelled()` and decide how to exit.
- A `for value in queue:` loop over a queue from the current `with TaskGroup()` scope wakes on `group.cancel()` and finishes cleanly.
- Blocking queue, task, and network waits report cancellation through `QueueReceive`, `TaskResult`, `WaitAny`, `WaitAll`, or `io.Error`, depending on the API.

See [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au).

## Automatic Loop Safepoints

The compiler inserts a scheduling check, called a safepoint, on every loop backedge. A backedge is the jump back to the top of a loop, at the end of the body or at `continue`. At a safepoint, ready timers, Queue operations, and socket work on the same worker get a chance to run. `break` and `return` leave the loop without a check.

Safepoints do not make Aura preemptive:

- One long loop iteration can still delay other tasks on the same worker until it reaches the end of the body.
- Long straight-line computation has no automatic check.
- A safepoint does not check for cancellation. Keep calling `cancelled()` when a task must stop on request.

Checks are spread out with a per-function loop fuel counter:

| Case | Loop fuel |
| --- | --- |
| MIR execution | 8 units |
| native concurrent programs | 4,096 units |
| a program proven to have no possible sibling task | runtime checks removed |

Do not use these intervals to predict output order. Which runnable task goes next, and how concurrent tasks interleave, is unspecified.

## `yield_now`

`yield_now()` is an explicit scheduling point. Safepoints already keep a tight loop from starving the scheduler forever. Call `yield_now()` between bounded chunks of work when you want a scheduling point sooner than the native safepoint would give one:

```aura check-pass
def count(label: str):
    mut step: int32 = 1
    while step <= 3:
        print(f"{label}: {step}")
        step += 1
        yield_now()
```

`yield_now()` returns `None` when the current task is scheduled again. It yields only to runnable work on the same worker. It does not:

- sleep
- guarantee that a different task runs
- define an order for runnable tasks
- check for cancellation, so call `cancelled()` separately when that matters

See [examples/concurrency/yield_now.au](../examples/concurrency/yield_now.au).

## `sleep`

`sleep` pauses the current task:

```aura check-pass
sleep(100ms)
```

A delay can be computed with ordinary signed `Duration` arithmetic. For example, `attempt * 1ms` scales a base delay by a runtime attempt count. A sleep or timeout must be non-negative and must fit the host deadline. Invalid values fail. Only leaving out the timeout gives an unlimited wait.

See [examples/concurrency/sleep_builtin.au](../examples/concurrency/sleep_builtin.au). For `Duration` constructors, arithmetic, comparison, conversion, and sub-millisecond rendering, see [examples/concurrency/duration_arithmetic.au](../examples/concurrency/duration_arithmetic.au).

### Backoff Without Hidden Final Delays

Retry policy belongs in application code. The [retrying network worker](../examples/agents/retrying_network_worker.au) example shows one policy:

- It retries only HTTP `503`.
- It doubles a `Duration` backoff after each retry.
- It adds jitter from `random.Rng(42)`, so its trace is reproducible.

The worker checks the response status and the final-attempt guard before it draws randomness, prints a retry, or calls `sleep(...)`. After three attempts it returns the last `503` immediately. There is no hidden fourth attempt and no final delay. A non-retryable status such as `429` also returns immediately.

The example runs the loopback server and the worker in one `TaskGroup`. Network and task waits have explicit five-second deadlines. Listeners, exchanges, and responses are scoped with `with`. A CLI regression test checks the same seven-request trace on the MIR and forced-direct backends.

## Full Example

The producer sends two values and closes the queue. The main task prints each value, then the producer's result. This prints `2`, `4`, and `6`:

```aura check-pass
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
            match jobs.poll(timeout=50ms):
                case Poll.Ready(value):
                    print(value)
                case Poll.Unavailable:
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

## How The Runtime Schedules Tasks

This section describes the runtime underneath the API. You do not need it to write concurrent code.

### Workers

Aura 0.3 runs task bodies on cooperative pinned scheduler workers, on both backends.

- The default worker count is the available parallelism the host reports.
- The provisional `AURA_WORKERS=<positive integer>` environment variable sets an explicit count.
- A task gets a fixed worker when it is spawned. Its coroutine stack never moves to another worker.
- The runtime does not steal work, and it does not preempt tasks.
- Tasks are lightweight coroutines, so a task does not need its own OS thread.

Pinned workers let tasks run on several cores. How much faster a program runs depends on the workload. Scheduling, the order in which tasks complete, and printed-output order are unspecified. Aura has no API that reports a task's worker index or affinity.

The scheduler waits efficiently:

- Descriptor registrations persist across waits.
- Deadlines are kept in a timer heap.
- Queue, task-completion, and blocking-pool events notify the responsible worker directly.
- A worker with nothing ready blocks until work, an event, or a deadline arrives. It does not wake on a periodic tick.

If a child task traps, its diagnostic keeps the typed Aura call frames. It also lists the task's ancestry, youngest first, naming the task entry and the parent spawn site. Both backends print the same call and task notes. Tooling receives the same records without parsing those notes.

### Protocol Service

Deep HTTP, TLS, and Unix WebSocket library steps run on a separate, bounded protocol service with deep native worker stacks. After each bounded, nonblocking step, protocol state returns to the lightweight task. This happens before cancellation or reactor waiting resumes.

The protocol service starts the first time it is needed and lives until the process exits. Aura 0.3 has no operation to shut it down or join it.

### Blocking-I/O Pool

File reads, resolver work, and listener binding run on a generic blocking-I/O pool. TLS asset bytes are read there. PEM parsing and rustls construction then run on protocol workers.

Two environment variables configure the pool:

| Variable | Effect |
| --- | --- |
| `AURA_BLOCKING_WORKERS` | An exact positive worker count, not clamped. Without it, Aura uses host parallelism, with fallback `4` and a derived `2..=8` clamp. |
| `AURA_BLOCKING_QUEUE_CAPACITY` | A positive value bounds accepted pending jobs only. Without it, the queue is unbounded. |

When the pool's queue is full:

- Admission is first in, first out. The waiting Aura task parks through the scheduler.
- Cancellation or a timeout before the job enters the queue prevents submission.
- Accepted work cannot be withdrawn, and its late result is discarded.
- A bounded queue does not guarantee progress for unrelated work while every blocking worker is stuck.

### Memory Measurements

Aura tests task memory against a 512 MiB gate. The gate covers 10,000 parked sleepers. The runtime accepts more tasks than that.

| Workload on a clean Mac14,9 | Peak whole-process RSS | Timer behavior |
| --- | --- | --- |
| 10,000 parked sleepers, three runs | 207,798,272, 206,946,304, and 206,831,616 bytes, all below 512 MiB | not applicable |
| 1,000 standalone timers | not applicable | stable, 6 ms maximum arm span, 1 ms worst p99 overshoot |
| 100,000 sleepers plus 1,000 timers, three runs | 1,170,735,104, 1,921,531,904, and 2,001,305,600 bytes. Two runs exceeded 1.5 GiB | stable, 3 ms maximum arm span, 2 ms worst p99 overshoot |

RSS is resident set size, the memory the process holds in RAM. Mac14,9 uses 16 KiB pages, so 101,000 stackful tasks need at least 1,654,784,000 bytes for one page each. That floor excludes scheduler, program, and process metadata. An earlier sample that came in below the gate depended on nondeterministic memory compression.

The same report passes the four-worker scaling gate, with a `1.039673x` paired median wall-time ratio and `396.73%` median four-task process CPU.

## Current Limits

- Queue waits, task waits, `sleep(...)`, socket waits, and HTTP waits all run on the pinned-worker scheduler.
- Cancellation is cooperative. Preemptive cancellation is not available.
- Loop backedges have compiler-inserted scheduling checks, but one long loop body can still delay other tasks.
- Task arguments are owned captures. Bare shared and `own` target parameters work. `mut` target parameters are rejected.
