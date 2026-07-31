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

`Task[T]` is always safe to transfer between tasks, but it is copyable only
when `T` is repeatable: a copy value, a `Queue[...]` handle, or a
recursively repeatable `Task[...]` handle. A non-copy owned result gives the
task handle one observation right. `result`, `result_or_none`, and `result_or`
consume that right on the first attempt, even if the attempt times out, is
cancelled, fails, returns `None`, or selects a fallback.

A result that is not structurally `Transfer`, such as `random.Rng` or a live
host resource, is rejected before the task is scheduled with `AU3008`.
`AU3009` instead reports an operation that would duplicate a valid
single-consumer result right.

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

## Choosing A Custom Task Stack

Ordinary tasks use a guarded 512 KiB stack. That is the safe default for
application code and keeps large task populations economical. If measurement
shows that one child has a different task-local stack requirement, use a
collision-free stack override:

```python
def deep_worker(depth: int32) -> int32:
    return visit_tree(depth)

with group = TaskGroup():
    task = group.start_with_stack(1024 * 1024, deep_worker, 128)
```

Use `start_soon_with_stack(bytes, function, ...)` when the child does not
return a retained handle. The byte count is exact `int64` and must be from
256 KiB through 64 MiB inclusive. Aurora rejects a smaller or larger value
instead of silently clamping it. Accepted capacities are rounded upward to
the host page size and guard-protected.

Treat 256 KiB as an opt-in minimum only for a measured shallow task. It is not
the ordinary default: Aurora's complete compiled HTTP example faulted when
256 KiB was the global task default during integration and succeeds with the
512 KiB default. The lower-level runtime round trip that succeeds with
256 KiB protocol callers intentionally omits compiled Aurora execution
frames; it proves that deep protocol frames run on service workers, not that
every complete Aurora task is safe at 256 KiB.

The method name is deliberately separate from `start`: a forwarded target may
have its own parameter named `stack_size`, so Aurora does not steal a named
argument from the child. Prefer the ordinary methods until profiling
demonstrates a need; a larger reservation is not a performance hint.

## Ownership When Starting Tasks

Starting a task creates **owned captures**. Each argument moves or copies into
task-owned storage before the child can outlive the caller. The target may then
borrow that capture or consume it; it never borrows the caller's stack value.

When both the parent and the child want the same clone-safe move value, clone
before starting:

```python
def worker(label: String):
    print(label)

with group = TaskGroup():
    label = "build"
    group.start_soon(worker, label.clone())
    print(label)
```

Copy types (numbers, `bool`, `Duration`, queue handles, and task handles with
repeatable results) pass through unchanged. Bare shared parameters borrow the
task-owned capture, `own` parameters consume it, and `mut` targets are rejected
because detached capture has no caller-visible writeback.

Every captured argument and the target result must be structurally `Transfer`
after generic specialization. Copy data, `String`, structurally transferable
collections and user data, and Queue/Task handle identities can cross.
Capability views, `random.Rng`, `TaskGroup`, and live file, process, or network
resources cannot. `Transfer` is derived by the compiler rather than implemented
as a user trait. Queue and Task handle state is synchronized for cross-worker
use; all other captures and results remain owned, share-nothing `Transfer`
values.

## `Queue[T]`: Typed Channels

A queue moves values between tasks. Handles to the same queue are copy values,
so passing one to a producer does not take it away from the parent. Queue
construction, `put`, and `try_put` require a structurally `Transfer` payload;
receiving moves one admitted value to the consumer.

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
group has completed. Queue is not a place traversal, so explicit `own` and
`mut` loop modifiers are rejected. The last case means the
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

## Waiting On Queues, Tasks, And Deadlines

Use `select(...)` when one operation may become ready through different source
kinds:

```python
outcome = select(messages, task, 50ms)

match own outcome:
    case SelectOutcome.Queue(index, received):
        print(index)
        print(received)
    case SelectOutcome.Task(index, result):
        print(index)
        print(result)
    case SelectOutcome.Deadline(index):
        print(index)
    case SelectOutcome.Cancelled:
        print("cancelled")
```

The Queue sources in one call share a payload type, and the Task sources share
a result type. Missing categories use `None` in `SelectOutcome[Q, T]`.
Cancellation wins; otherwise the lowest original argument index wins a tie.
The runtime registers one composite wait and removes every loser when a source
wins. A losing Queue remains unchanged. A non-repeatable Task right is
consumed at entry and abandoned if another source wins.

This is an ordinary builtin call. Aurora still has no statement-form
`select:` syntax.

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

With repeatable `T`, the handles and observations remain reusable. With a
non-repeatable but transferable `T`, either helper consumes the complete task
vector on its first attempt, including timeout, cancellation, and failure.
`wait_any` deliberately abandons the observation rights of unchosen tasks.
Queue receives transfer one owned item rather than observing task-result
storage.

## Cancellation Is Cooperative

Calling `group.cancel()` signals child tasks. Tasks observe cancellation at
**scheduler-aware waits**: `sleep`, queue sends and receives, task-result
waits, socket waits, HTTP calls, and process waits. Compiler-inserted loop
safepoints schedule sibling work but deliberately do not inspect cancellation,
so a CPU-bound loop that must stop on request should check `cancelled()` itself:

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

Aurora 0.2 runs task bodies on cooperative pinned workers on both maintained
backends. The runtime uses the available parallelism reported by the host by
default; provisional
`AURORA_WORKERS=<positive integer>` selects an explicit count. A task receives
a stable assignment when it is spawned. Its coroutine stack never migrates,
work is not stolen, and `yield_now()` yields only to runnable work on that
worker.

Every loop backedge includes an automatic scheduling check. Normal loop tails
and `continue` take the check; `break` and `return` leave without it. This
keeps a tight loop from freezing timers, queues, and sockets assigned to the
same worker indefinitely, but one long loop body or long straight-line
computation can still delay same-worker siblings. Ordinary tasks request a
guarded 512 KiB coroutine stack, with an explicit per-child override available
through the two `_with_stack` methods. Scheduler waits are event-driven:
descriptors stay registered, deadlines are kept in a timer heap, and Queue,
task-completion, and blocking-pool events notify the responsible worker
directly. An idle worker blocks until local work, an event, or a deadline
instead of waking on a periodic tick.

Queue and Task handles are the cross-worker channels. Other captures and
results remain owned `Transfer` values, so the model stays share-nothing.
Cancellation and diagnostics remain per task. Scheduling, independent task
completion, and printed-output order are unspecified; Aurora exposes no worker
identity or affinity API. Pinned workers enable multicore task execution, but
do not promise preemption, work stealing, or speedup for every workload.

Deep HTTP, TLS, and maintained Unix WebSocket library frames run on a bounded
protocol-step service with deep native worker stacks. Each step is bounded and
nonblocking; the child gets ownership of its protocol state back before
observing cancellation or returning to reactor readiness waiting. This is why
ordinary application tasks no longer need to reserve enough coroutine stack
for the deepest maintained third-party protocol frame.

The protocol-step pool starts lazily and lives until the Aurora process exits;
there is no 0.2 shutdown or join call. File reads, resolver work, and listener
binding continue through the generic blocking-I/O pool. For TLS assets, that
generic pool reads the bytes and the protocol workers perform PEM parsing and
rustls construction.

The generic pool is a separate operational control.
`AURORA_BLOCKING_WORKERS=<positive integer>` requests an exact worker count;
without it Aurora derives a `2..=8` default from host parallelism with fallback
`4`. `AURORA_BLOCKING_QUEUE_CAPACITY=<positive integer>` optionally limits
accepted jobs still waiting in the FIFO queue. It does not count running jobs
or callers waiting for admission, and omitting it preserves an unbounded
queue. A full bounded queue parks the Aurora task without blocking its pinned
worker. Cancellation or timeout before queue insertion prevents the host job
from running. After insertion, Aurora can stop waiting but cannot retract the
host operation; its late result is discarded. The bound controls accepted
pending backlog, not admission waiters or a stuck OS call, so unrelated
blocking-I/O host work still cannot run until some worker returns when every
worker is occupied.

The clean Mac14,9 Phase 5.10 measurement at `181204b` does not support a
maintained 100,000-task memory claim. The three 100,000-sleeper plus
1,000-timer runs peaked at 1,170,735,104, 1,921,531,904, and 2,001,305,600
bytes of whole-process RSS. The Phase 5 massive-RSS escape hatch therefore
applies and the earlier “at most 1.5 GiB” claim is withdrawn. On this host, one
16 KiB resident page for each of the 101,000 stackful children alone requires
1,654,784,000 bytes before scheduler metadata or the root runtime. The lower
Phase 5.9 result depended on macOS memory compression and is not a stable
contract. The 10,000-sleeper, standalone-timer, idle-CPU, starvation, and
mandatory multicore gates all pass.

MIR execution checks every loop backedge and yields every 8 backedges. Native
concurrent programs use a function-local 4,096-iteration fuel budget, and
sequential native programs remove checks that cannot have a sibling to
schedule. These implementation
choices do not promise an interleaving or ready-task order.

`yield_now()` adds an explicit cooperative scheduling point between
application-chosen chunks:

```python
def crunch():
    mut chunk: int32 = 0
    while chunk < 100:
        process_chunk(chunk)
        chunk += 1
        yield_now()
```

It gives runnable siblings an opportunity to proceed, but does not sleep,
promise that another task runs, or check cancellation. Use `cancelled()` when
the task must also respond to a cancellation request.

## The Shape Worth Copying

Good Aurora concurrency tends to look the same across programs:

- one `with TaskGroup()` per concurrent operation
- queues owned by the parent, closed by the producers
- task results inspected through `TaskResult`, `wait_any`, or `wait_all`
- long CPU loops that check `cancelled()` when cancellation matters and use
  explicit yields when a particular chunk boundary should schedule siblings
- no detached background work; Aurora 0.2 exposes no detached task form

If you can say, for each child task, which scope created it and which scope waits for it, the program is usually on the right track.

Reference: [Concurrency](/manual/concurrency).
