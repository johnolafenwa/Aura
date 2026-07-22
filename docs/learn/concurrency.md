# Structured Concurrency

Concurrent programs get hard to reason about when child work has no parent. A task started deep inside a function might run forever, fail silently, or leak a resource. The fix Aurora builds into the language is called **structured concurrency**: every task is created within a scope, and leaving that scope waits for, cancels, or otherwise accounts for the children.

This chapter introduces that scope — the `TaskGroup` — and the two other primitives that make it useful: `Task[T]`, a handle to a task's result, and `Queue[T]`, a typed channel for moving values between tasks.

## Start One Task

Begin with a plain worker:

```python
def double(value: int32) -> int32:
    return value * 2
```

Run it inside a task group:

```python
with group = TaskGroup():
    task = group.start(double, 21)

    match task.result(timeout=1s):
        case TaskResult.Ready(value):
            print(value)
        case TaskResult.Error(message):
            print(message)
        case TaskResult.TimedOut:
            print("timeout")
        case TaskResult.Cancelled:
            print("cancelled")
```

The `with` block defines the task's lifetime. Leaving the block waits for children the program started and accounts for any failures. Nothing is hidden, and nothing keeps running in the background after the block ends.

`TaskResult[T]` has four cases — `Ready`, `Error`, `TimedOut`, and `Cancelled` — because those are the four things that can happen to a child task, and a reasonable program might want different behaviour for each.

Repeated observation is supported for copy data and explicitly shared synchronized handles. A result containing an exclusive runtime resource is single-observer-only in Aurora 0.1. The checker does not enforce that restriction yet, so give such a result exactly one designated observer.

The timeout is a signed nanosecond `Duration`. Literals cover integral `ms`,
`s`, and `m` values; `Duration.ms(n)`, `Duration.seconds(n)`, checked
arithmetic, and comparisons handle runtime-computed backoff. A negative or
host-unrepresentable wait is invalid and never means “wait forever.”

## Fire-And-Forget Inside A Scope

When the program does not need a handle to a child's result, use `start_soon`:

```python
def say(label: String):
    print(label)

with group = TaskGroup():
    group.start_soon(say, "parse")
    group.start_soon(say, "check")
```

"Fire and forget" still has a parent here. The children are only forgotten by the local code; the runtime is still responsible for them.

## Ownership When Starting Tasks

Starting a task creates **owned captures**. Each argument moves or copies into
task-owned storage before the child can outlive the caller. The target may then
borrow that capture or consume it; it never borrows the caller's stack value.

When both the parent and the child want the same move value, clone before starting:

```python
def worker(label: String):
    print(label)

with group = TaskGroup():
    label = "build"
    group.start_soon(worker, label.clone())
    print(label)
```

Copy types (numbers, `bool`, `Duration`, queue handles, task handles) pass
through unchanged. Bare/default and explicit shared parameters borrow the
task-owned capture, `own` parameters consume it, and `borrow mut` targets are
rejected because detached capture has no caller-visible writeback.

## `Queue[T]`: Typed Channels

A queue moves values between tasks. Handles to the same queue are copy values, so passing one to a producer does not take it away from the parent.

```python
def producer(jobs: Queue[int32]):
    for value in range(5):
        jobs.put(value)
    jobs.close()

jobs = Queue[int32]()

with group = TaskGroup():
    group.start_soon(producer, jobs)

    for job in jobs:
        print(job)
```

Two things are happening in that `for` loop. The consumer receives each value
already owned until one of three things is true: the queue is closed,
cancellation interrupts the loop, or every producer in the surrounding task
group has completed. Queue is not a place traversal, so explicit `own`,
`borrow`, and `borrow mut` loop modifiers are rejected. The last case means the
program can often rely on normal exit to drain the queue; explicitly calling
`close()` is still the clearest signal.

## Bounded Queues And Backpressure

`Queue[T]()` creates an unbounded queue. An unbounded queue is convenient but risky: a fast producer and a slow consumer will let memory grow without limit. A **bounded** queue says how many values are allowed in flight:

```python
jobs = Queue[String](capacity=2)
```

When a bounded queue is full, `put` waits until space is available, a timeout expires, the queue closes, or the task is cancelled. The failure shape is `SendError[T]`, which carries the unsent value back to the caller:

```python
match jobs.put("compile", timeout=50ms):
    case Result.Ok(_):
        print("queued")
    case Result.Err(SendError.Full(job)):
        print("full")
    case Result.Err(SendError.TimedOut(job)):
        print("timeout")
    case Result.Err(SendError.Closed(job)):
        print("closed")
    case Result.Err(SendError.Cancelled(job)):
        print("cancelled")
```

`try_put` is the non-waiting variant. Use it when waiting would be wrong — for example, when a polling loop has other work to do if the queue is full.

## A Worker Pool

A common shape has one producer and several workers. Each worker reads the same queue until the producer closes it:

```python
def worker(name: String, jobs: Queue[int32]):
    for job in jobs:
        print(f"{name}: {job}")

def produce(jobs: Queue[int32]):
    for job in range(8):
        jobs.put(job)
    jobs.close()

jobs = Queue[int32](capacity=3)

with group = TaskGroup():
    group.start_soon(produce, jobs)
    group.start_soon(worker, "a", jobs)
    group.start_soon(worker, "b", jobs)
```

The parent owns the shape of the system: it decides how many workers to spawn and what capacity the queue has. The producer owns the decision to close the queue. Each worker owns only the job it is currently processing.

Leaving the `with` block waits for the producer to finish and for each worker to drain the queue.

## Waiting For Several Tasks

Sometimes a program needs to wait on a batch of tasks at once. `wait_any` returns when the first one finishes:

```python
tasks: Vec[Task[int32]] = []

with group = TaskGroup():
    tasks.push(group.start(double, 10))
    tasks.push(group.start(double, 20))

    match wait_any(tasks, timeout=1s):
        case WaitAny.Ready(index, value):
            print(f"task {index}: {value}")
        case WaitAny.Error(index, message):
            print(message)
        case WaitAny.TimedOut:
            print("timeout")
        case WaitAny.Cancelled:
            print("cancelled")
```

`wait_all` returns when every task has either produced a value or one has failed:

```python
match wait_all(tasks, timeout=1s):
    case WaitAll.Ready(values):
        for value in values:
            print(value)
    case WaitAll.Error(index, message):
        print(message)
    case WaitAll.TimedOut:
        print("timeout")
    case WaitAll.Cancelled:
        print("cancelled")
```

The `Error(index, message)` variant reports **which** task failed. That is usually more useful than a bare error.

## Cancellation Is Cooperative

Calling `group.cancel()` signals child tasks. Tasks observe cancellation at **scheduler-aware waits**: `sleep`, queue sends and receives, task-result waits, socket waits, HTTP calls, and process waits. A CPU-bound loop that never reaches one of those waits should check `cancelled()` itself:

```python
def ticker():
    while not cancelled():
        print("tick")
        sleep(100ms)

with group = TaskGroup():
    group.start_soon(ticker)
    sleep(350ms)
    group.cancel()
```

Cancellation is not an exception that lands at arbitrary points in the code. It is a request that tasks observe at well-defined boundaries. That makes cancelled code easy to reason about — and easy to test.

Aurora 0.1 runs Aurora task bodies on one cooperative scheduler thread, not in parallel. CPU code without a scheduler boundary can starve sibling tasks. Each task reserves a fixed 1 MiB coroutine stack, and readiness scanning is linear in the waiting-task set.

## The Shape Worth Copying

Good Aurora concurrency tends to look the same across programs:

- one `with TaskGroup()` per concurrent operation
- queues owned by the parent, closed by the producers
- task results inspected through `TaskResult`, `wait_any`, or `wait_all`
- long CPU loops that check `cancelled()`
- no detached background work; Aurora 0.1 exposes no detached task form

If you can say, for each child task, which scope created it and which scope waits for it, the program is usually on the right track.

Reference: [Concurrency](/manual/concurrency).
