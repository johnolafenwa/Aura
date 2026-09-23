# Structured Concurrency

This page covers structured concurrency in Aura. Every task starts inside a
scope, and leaving that scope waits for, cancels, or otherwise accounts for
its children. Child work with no parent is hard to reason about. A task
started deep inside a function might run forever, fail silently, or leak a
resource.

Three types do the work:

- `TaskGroup` is the scope that owns child tasks.
- `Task[T]` is a handle to one task's result.
- `Queue[T]` is a typed channel that moves values between tasks.

## Start One Task

Begin with a plain worker:

```aura
def double(value: int32) -> int32:
    return value * 2
```

Run it inside a task group:

```aura fragment
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

The `with` block sets the task's lifetime. Leaving the block waits for every
child the program started and accounts for any failures. Nothing keeps
running in the background after the block ends.

`TaskResult[T]` has one case for each thing that can happen to a child task:
`Ready`, `Error`, `TimedOut`, and `Cancelled`. A program can handle each one
differently.

`Task[T]` is always safe to transfer between tasks. It is copyable only when
`T` is repeatable. A repeatable type is a copy value, a `Queue[...]` handle,
or a `Task[...]` handle whose own result is repeatable.

When the result is an owned value that is not a copy type, the handle has one
observation right. The first call to `result`, `poll`, or `result_or` uses it
up. That holds even when the attempt times out, is cancelled, fails, returns
`Poll.Unavailable`, or selects a fallback.

Two diagnostics guard task results. `Transfer` is the property of values that
can move to another task, described under
[Ownership When Starting Tasks](#ownership-when-starting-tasks).

| Code | Cause | Fix |
| --- | --- | --- |
| `AU3008` | The result is not structurally `Transfer`, such as `random.Rng` or a live host resource. The compiler rejects it before the task is scheduled. | Return a value that can be transferred. |
| `AU3009` | An operation would duplicate a valid single-consumer result right. | Observe the result once. |

The timeout is a `Duration`, a signed count of nanoseconds. Literals cover
integral `ms`, `s`, and `m` values. For backoff computed at runtime, use
`Duration.ms(n)`, `Duration.seconds(n)`, checked arithmetic, and comparisons.
A negative wait, or one the host cannot represent, is invalid. It never means
"wait forever."

## Fire-And-Forget Inside A Scope

Use `start_soon` when the program does not need the child's result:

```aura
def say(label: str):
    print(label)

with group = TaskGroup():
    group.start_soon(say, "parse")
    group.start_soon(say, "check")
```

A fire-and-forget child still has a parent. The local code forgets it, but
the runtime stays responsible for it.

## Choosing A Custom Task Stack

Ordinary tasks use a guarded 768 KiB stack. This default is safe for
application code and keeps large numbers of tasks economical. If measurement
shows that one child needs a different stack size, give that child its own
stack:

```aura fragment
def deep_worker(depth: int32) -> int32:
    return visit_tree(depth)

with group = TaskGroup():
    task = group.start_with_stack(1024 * 1024, deep_worker, 128)
```

Use `start_soon_with_stack(bytes, function, ...)` when you do not need a
handle to the child. The byte count is an exact `int64` from 256 KiB through
64 MiB inclusive. Aura rejects smaller and larger values. It rounds an
accepted size up to the host page size and guard-protects it.

Treat 256 KiB as an opt-in minimum for a measured shallow task only. It is
not a safe default. In testing, Aura's complete compiled HTTP example faulted
with a 256 KiB global task default and succeeded with 512 KiB.

A lower-level runtime round trip does succeed with 256 KiB protocol callers.
That test leaves out compiled Aura execution frames on purpose. It shows that
deep protocol frames run on service workers. It does not show that every
complete Aura task is safe at 256 KiB.

These methods are named separately from `start` because a forwarded target
may have its own parameter named `stack_size`. Aura does not take a named
argument away from the child. Keep the ordinary methods until profiling shows
a need. A larger reservation is not a performance hint.

## Ownership When Starting Tasks

Starting a task creates owned captures. Each argument moves or copies into
task-owned storage before the child can outlive the caller. The target may
borrow that capture or consume it. It never borrows a value on the caller's
stack.

When the parent and the child both need the same clone-safe move value, clone
it before starting the task:

```aura
def worker(label: str):
    print(label)

with group = TaskGroup():
    label = "build"
    group.start_soon(worker, label.clone())
    print(label)
```

Each kind of argument passes differently:

- Copy types pass through unchanged. These include numbers, `bool`,
  `Duration`, queue handles, and task handles with repeatable results.
- A bare shared parameter borrows the task-owned capture.
- An `own` parameter consumes the capture.
- A `mut` parameter is rejected, because a detached capture has no writeback
  the caller could see.

Every captured argument and the target's result must be structurally
`Transfer` after generic specialization. The compiler derives `Transfer`.
User code cannot implement it as a trait.

| Can cross into a task | Cannot cross |
| --- | --- |
| Copy data, `str`, structurally transferable collections and user data, Queue and Task handle identities | Capability views, `random.Rng`, `TaskGroup`, live file, process, and network resources |

Queue and Task handle state is synchronized for use across workers. Every
other capture and result is an owned, share-nothing `Transfer` value.

## `Queue[T]`: Typed Channels

A queue moves values between tasks. Handles to one queue are copy values, so
passing a handle to a producer does not take it away from the parent.
Creating a queue, `put`, and `try_put` all require a structurally `Transfer`
payload. Each receive moves one value to the consumer.

```aura
def producer(jobs: Queue[int64]):
    for value in range(5):
        jobs.put(value)
    jobs.close()

jobs = Queue[int64]()

with group = TaskGroup():
    group.start_soon(producer, jobs)

    for job in jobs:
        print(job)
```

The `for` loop receives each value already owned. The loop ends when any of
these is true:

- the queue is closed
- cancellation interrupts the loop
- every producer in the surrounding task group has completed

Because of the last case, a program can often rely on normal exit to drain
the queue. Calling `close()` is still the clearest signal.

A queue loop receives values. It does not walk elements stored in place, so
the explicit `own` and `mut` loop modifiers are rejected.

## Bounded Queues And Backpressure

`Queue[T]()` creates an unbounded queue. It is convenient, but a fast
producer and a slow consumer let memory grow without limit. A bounded queue
sets how many values may be in flight:

```aura
jobs = Queue[str](capacity=2)
```

When a bounded queue is full, `put` waits until space frees up, a timeout
expires, the queue closes, or the task is cancelled. A failure is a
`SendError[T]`, which hands the unsent value back to the caller:

```aura fragment
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

`try_put` does not wait. Use it when waiting would be wrong, such as in a
polling loop that has other work to do while the queue is full.

## A Worker Pool

A common shape has one producer and several workers. Every worker reads the
same queue until the producer closes it:

```aura
def worker(name: str, jobs: Queue[int64]):
    for job in jobs:
        print(f"{name}: {job}")

def produce(jobs: Queue[int64]):
    for job in range(8):
        jobs.put(job)
    jobs.close()

jobs = Queue[int64](capacity=3)

with group = TaskGroup():
    group.start_soon(produce, jobs)
    group.start_soon(worker, "a", jobs)
    group.start_soon(worker, "b", jobs)
```

Each part owns one decision:

- The parent sets the shape of the system: how many workers to start and the
  queue's capacity.
- The producer decides when to close the queue.
- Each worker owns only the job it is processing.

Leaving the `with` block waits for the producer to finish and for each worker
to drain the queue.

## Waiting On Queues, Tasks, And Deadlines

Use `select(...)` when the next event may come from sources of different
kinds. It is an ordinary builtin call:

```aura fragment
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

- All Queue sources in one call share a payload type. All Task sources share
  a result type.
- A category with no sources uses `None` in `SelectOutcome[Q, T]`.
- Cancellation always wins. Otherwise, on a tie, the lowest original argument
  index wins.
- The runtime registers one composite wait. When a source wins, the runtime
  removes every loser.
- A losing Queue stays unchanged. A non-repeatable Task right is consumed on
  entry and abandoned if another source wins.

## Waiting For Several Tasks

Sometimes a program needs to wait on a batch of tasks. `wait_any` returns when
the first one finishes:

```aura fragment
mut tasks: list[Task[int32]] = []

with group = TaskGroup():
    tasks.append(group.start(double, 10))
    tasks.append(group.start(double, 20))

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

`wait_all` returns when every task has produced a value, or when one has
failed:

```aura fragment
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

`Error(index, message)` reports which task failed. That is usually more
useful than a bare error.

With a repeatable `T`, the handles and their observations stay reusable. With
a `T` that is transferable but not repeatable, either helper consumes the
whole task list on its first attempt. That includes an attempt that times
out, is cancelled, or fails. `wait_any` abandons the observation rights of
the tasks it does not choose. A Queue receive moves one owned item. It never
reads task-result storage.

## Cancellation Is Cooperative

`group.cancel()` signals the child tasks. A task observes cancellation only at
scheduler-aware waits:

- `sleep`
- queue sends and receives
- task-result waits
- socket waits
- HTTP calls
- process waits

The compiler inserts safepoints in loops. They schedule sibling work, but they
do not check for cancellation. A CPU-bound loop that must stop on request
checks `cancelled()` itself:

```aura
def ticker():
    while not cancelled():
        print("tick")
        sleep(100ms)

with group = TaskGroup():
    group.start_soon(ticker)
    sleep(350ms)
    group.cancel()
```

Cancellation is not an exception that can land anywhere in the code. It is a
request that tasks observe at well-defined points. That makes cancelled code
easy to reason about and easy to test.

## How Tasks Are Scheduled

Aura 0.3 runs task bodies on cooperative pinned workers on both maintained
backends.

- By default, the runtime uses the available parallelism that the host
  reports. The provisional `AURA_WORKERS=<positive integer>` setting selects
  an explicit count.
- A task gets a stable worker assignment when it is spawned. Its coroutine
  stack never migrates, and no worker steals work.
- `yield_now()` yields only to runnable work on the same worker.
- Scheduling, the order in which independent tasks complete, and the order of
  printed output are unspecified.
- Aura exposes no worker identity or affinity API.
- Cancellation and diagnostics stay per task.

Pinned workers let tasks run on several cores. Preemption and work stealing
are not available, and any speedup depends on the workload.

Queue and Task handles are the channels between workers. All other captures
and results stay owned `Transfer` values, so the model stays share-nothing.

### Loop Scheduling Checks

Every loop backedge includes an automatic scheduling check. A backedge is the
jump from the end of a loop body back to its start. Normal loop tails and
`continue` take the check. `break` and `return` leave without it.

The check keeps a tight loop from freezing the timers, queues, and sockets on
its worker forever. One long loop body, or a long stretch of straight-line
code, can still delay siblings on the same worker.

How often a check yields depends on how the program runs. MIR is Aura's
mid-level intermediate representation, which the default `aura run` runtime
executes.

| Execution | Behavior |
| --- | --- |
| MIR | Checks every loop backedge and yields every 8 backedges. |
| Native, concurrent program | Uses a function-local budget of 4,096 iterations. |
| Native, sequential program | Removes checks that cannot have a sibling to schedule. |

Interleaving and ready-task order stay unspecified.

`yield_now()` adds an explicit cooperative scheduling point between chunks
that the application chooses:

```aura fragment
def crunch():
    mut chunk: int32 = 0
    while chunk < 100:
        process_chunk(chunk)
        chunk += 1
        yield_now()
```

`yield_now()` gives runnable siblings a chance to proceed. It does not sleep,
does not promise that another task runs, and does not check cancellation. Use
`cancelled()` when the task must also respond to a cancellation request.

### Event-Driven Waits

Scheduler waits are event-driven:

- Descriptors stay registered.
- Deadlines live in a timer heap.
- Queue, task-completion, and blocking-pool events notify the responsible
  worker directly.

An idle worker sleeps until local work, an event, or a deadline is ready. The
scheduler uses no periodic tick.

### Protocol And Blocking-I/O Pools

Deep HTTP, TLS, and maintained Unix WebSocket library frames run on a bounded
protocol-step service with deep native worker stacks. Each step is bounded and
nonblocking. The child gets its protocol state back before it observes
cancellation or returns to waiting on reactor readiness.

Ordinary application tasks keep the guarded 768 KiB default stack. The
protocol workers carry the deepest maintained third-party library frames.

The protocol-step pool starts lazily and lives until the Aura process exits.
There is no 0.2 shutdown or join call.

File reads, resolver work, and listener binding go through the generic
blocking-I/O pool. For TLS assets, the generic pool reads the bytes, and the
protocol workers perform PEM parsing and rustls construction.

The blocking-I/O pool has its own controls:

| Variable | Effect |
| --- | --- |
| `AURA_BLOCKING_WORKERS=<positive integer>` | Requests an exact worker count. Without it, Aura derives a `2..=8` default from host parallelism, with fallback `4`. |
| `AURA_BLOCKING_QUEUE_CAPACITY=<positive integer>` | Limits the accepted jobs still waiting in the first-in, first-out queue. Without it, the queue is unbounded. |

The queue capacity does not count running jobs or callers waiting for
admission. When the bounded queue is full, the Aura task parks without
blocking its pinned worker.

- If the task is cancelled or times out before its job enters the queue, the
  host job never runs.
- After the job enters the queue, Aura can stop waiting, but it cannot
  retract the host operation. Aura discards the late result.

The bound controls the accepted pending backlog. It does not control
admission waiters or a stuck operating system call. When every worker is
busy, unrelated blocking-I/O host work still cannot run until some worker
returns.

### Memory Capacity

The runtime accepts larger task counts, but 10,000 sleepers is the maintained
memory-capacity bound. The 10,000-sleeper, standalone-timer, idle-CPU,
starvation, and mandatory multicore tests all pass.

In a clean measurement on a Mac14,9 at
revision `181204b`, three runs of 100,000 sleepers plus 1,000 timers peaked at
1,170,735,104, 1,921,531,904, and 2,001,305,600 bytes of whole-process
resident set size. Two runs exceeded the proposed 1.5 GiB bound.

On this host, one 16 KiB resident page for each of the 101,000 stackful
children needs 1,654,784,000 bytes by itself. That is before scheduler
metadata or the root runtime. An earlier, lower result depended on macOS
memory compression.

## The Shape Worth Copying

Good Aura concurrency tends to look the same across programs:

- one `with TaskGroup()` per concurrent operation
- queues owned by the parent and closed by the producers
- task results inspected through `TaskResult`, `wait_any`, or `wait_all`
- long CPU loops that check `cancelled()` when cancellation matters, and use
  explicit yields where a chunk boundary should schedule siblings
- no detached background work, since Aura 0.3 has no detached task form

For each child task, you should be able to say which scope created it and
which scope waits for it. If you can, the program is usually on the right
track.

Reference: [Concurrency](/manual/concurrency).
