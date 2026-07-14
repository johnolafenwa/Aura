# Concurrency

Aurora provides single-threaded scheduler-backed lightweight tasks, structured task groups, queues, task handles, cancellation checks, sleeping, and multi-task wait helpers.

The maintained model is structured by default: child tasks should live inside a `TaskGroup`, and leaving the group scope waits for the children. Queue and task waits participate in the scheduler so a blocked task does not block the whole runtime.

## Duration Values

Scheduler APIs use `Duration`. Duration literals include units such as:

```python
10ms
1s
2m
```

Durations are copy values.

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
| `result` | `result(timeout: Duration = ...) -> TaskResult[T]` | Waits for completion and returns a structured outcome. |
| `result_or_none` | `result_or_none(timeout: Duration = ...) -> Option[T]` | Returns `Some(value)` on success and `None` on task failure, timeout, or cancellation. Without an explicit timeout, this helper performs an immediate check. |
| `result_or` | `result_or(default: own T, timeout: Duration = ...) -> T` | Returns the task value or `default` on task failure, timeout, or cancellation. Without an explicit timeout, this helper performs an immediate check. |

`TaskResult[T]` variants:

| Variant | Meaning |
| --- | --- |
| `Ready(value: own T)` | The task returned normally. |
| `Error(message: own String)` | The task failed with a runtime error. |
| `TimedOut` | The wait timed out. |
| `Cancelled` | The wait was interrupted by cancellation. |

Use `result` when the program needs to distinguish failure, timeout, and cancellation. Use `result_or_none` or `result_or` only when those outcomes are intentionally equivalent.

The completed value is stored by the task and cloned for each observation. Repeated observation is supported for copy data and explicitly shared synchronized handles. A result containing an exclusive runtime-backed resource is single-observer-only in 0.1. That restriction is not yet enforced statically, so a second observation can alias the same host resource; transfer such a result to exactly one designated observer.

## Queue[T]

`Queue[T]` moves values between tasks. Queue handles are copy values.

```python
jobs = Queue[String]()
bounded = Queue[String](capacity=8)
```

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `Queue[T](capacity: int32 = ...)` | Creates an unbounded queue when `capacity` is omitted, or a bounded queue when supplied. |
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
The receive loop ends when the queue closes, cancellation interrupts it, or
the relevant producers in the active task group complete. Closing queues
explicitly is still the clearest program shape.

## Top-Level Concurrency Builtins

| API | Signature | Contract |
| --- | --- | --- |
| `cancelled` | `cancelled() -> bool` | Returns `true` when the current task has been asked to cancel. |
| `sleep` | `sleep(duration: Duration) -> None` | Suspends the current task for at least `duration`, unless cancellation wakes it first. |
| `wait_any` | `wait_any(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAny[T]` | Waits for the first task outcome or timeout. `wait_any([])` returns `TimedOut` immediately. |
| `wait_all` | `wait_all(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAll[T]` | Waits until every task is ready, one task errors, timeout expires, or cancellation interrupts the wait. |

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
