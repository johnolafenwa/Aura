# Cancellation, timeouts, and shutdown

Starting work is the easy half of concurrency. The hard half is stopping it:
giving up on a task that's taking too long, shutting down a pool when the job
is done, and making sure that whatever was running gets to clean up on the way
out. This section is about that half. We'll see that a timeout and a
cancellation are different things, how a task notices that it's been
cancelled, and what a task group does at the end of its block when some of its
tasks are still waiting.

## A timeout stops the waiting, not the task

Let's make the word counter slow and wait for it with a short timeout:

```aura
def slow_count(text: str) -> int64:
    sleep(2s)
    return len(text.split(" "))

def main():
    with group = TaskGroup():
        task = group.start(slow_count, "a b c")
        match task.result(timeout=100ms):
            case TaskResult.Ready(count):
                print(f"words: {count}")
            case TaskResult.Error(message):
                print("failed: " + message)
            case TaskResult.TimedOut:
                print("gave up waiting")
            case TaskResult.Cancelled:
                print("cancelled")
    print("group closed")
```

```console
$ time aura run src/main.au
gave up waiting
group closed

real	0m2.054s
user	0m0.275s
sys	0m0.029s
```

`gave up waiting` printed after 100ms, as we asked. But the program still took
two seconds. The timeout ended *our wait* for the result. It didn't touch the
task, which went on sleeping, and when the block ended, the group's exit waited
for it, as it always does.

This is the behavior you want, even if it surprises you the first time. A
timeout is a decision about the waiter's patience, and only the waiter knows
whether giving up means "stop the work" or "check back later". If timeouts
killed tasks, you couldn't poll a task every 100ms to update a progress bar.
When you do want the work to stop, you say so separately.

## Cancelling a group

`group.cancel()` asks every task in the group to stop. Let's call it when we
give up, and have `slow_count` check whether it's been cancelled:

*Filename: src/main.au*

```aura
def slow_count(text: str) -> int64:
    sleep(2s)
    if cancelled():
        print("slow_count saw the cancellation")
        return -1
    return len(text.split(" "))

def main():
    with group = TaskGroup():
        task = group.start(slow_count, "a b c")
        match task.result(timeout=100ms):
            case TaskResult.Ready(count):
                print(f"words: {count}")
            case TaskResult.Error(message):
                print("failed: " + message)
            case TaskResult.TimedOut:
                print("gave up waiting, cancelling")
                group.cancel()
            case TaskResult.Cancelled:
                print("cancelled")
    print("group closed")
```

<span class="caption">Listing 15-5: Cancelling a task after a timeout</span>

```console
$ time aura run src/main.au
gave up waiting, cancelling
slow_count saw the cancellation
group closed

real	0m0.148s
user	0m0.030s
sys	0m0.021s
```

Now the program finishes in a fraction of a second. Two things happened when
we called `group.cancel()`. The task was marked as cancelled, and its
`sleep(2s)` woke up early. `slow_count` then called `cancelled()`, which
returns `true` once the current task has been asked to stop, and returned
early.

Notice what *didn't* happen: nothing was thrown into `slow_count`. The `sleep`
returned normally, and the task ran its next line. Cancellation in Aura is
*cooperative*. It's a request that the task observes at well-defined points,
and the task decides how to wind down.

## Cooperative cancellation

To compare, think about Python again. A Python thread can't be cancelled at
all: there is no supported way to stop one from outside, so programs build their
own "stop" flags. `asyncio` can cancel a task, and it does so by raising
`CancelledError` inside the coroutine at whatever `await` it's suspended on.
That's closer to Aura, but the exception can surface from any `await`, and it
can be caught and swallowed by an over-broad `except`.

Aura's model has no exception. A cancelled task notices the request in two
ways. Any wait that the scheduler knows about wakes up and reports it: `sleep`
returns early, a queue `get` returns `QueueReceive.Cancelled`, a `put` returns
`SendError.Cancelled`, a task wait returns `TaskResult.Cancelled`, and so on for
`select`, `wait_any`, `wait_all`, and the network and process waits. And the
task can ask directly with `cancelled()`.

The `with` blocks in a cancelled task still run their exits. Here's a ticker
that runs until it's cancelled, inside a `Section` from the last chapter:

*Filename: src/main.au*

```aura
class Section:
    name: str

    def __exit__(mut self) -> None:
        print("exit " + self.name)

def ticker(name: own str):
    with section = Section(name=name.clone()):
        mut ticks = 0
        while not cancelled():
            ticks += 1
            sleep(100ms)
        print(f"{name} stopped after {ticks} ticks")

def main():
    with group = TaskGroup():
        group.start_soon(ticker, "ticker")
        sleep(350ms)
        print("cancelling")
        group.cancel()
    print("group closed")
```

<span class="caption">Listing 15-6: A loop that stops when its task is cancelled</span>

```console
$ aura run src/main.au
cancelling
ticker stopped after 4 ticks
exit ticker
group closed
```

The ticker's fourth `sleep` is cut short by the cancellation, the loop
condition sees `cancelled()`, and the function leaves its `with` block, running
the exit on the way. The group then finishes joining and `main` carries on.

### The loop that never stops

The `while not cancelled()` in Listing 15-6 isn't decoration. Let's see what
happens without it:

```aura
def ticker():
    mut ticks = 0
    while true:
        ticks += 1
        sleep(100ms)
        print(f"tick {ticks}")

def main():
    with group = TaskGroup():
        group.start_soon(ticker)
        sleep(350ms)
        group.cancel()
    print("group closed")
```

```console
$ aura run src/main.au | head -n 6
tick 1
tick 2
tick 3
tick 4
tick 5
tick 6
```

The first three ticks arrive 100ms apart. Then the group is cancelled, and
from then on every `sleep(100ms)` returns at once, because the task is
cancelled and a cancelled task's waits don't wait. Ticks 4, 5, 6 and millions
more pour out as fast as the machine can print, and `group closed` never
appears, because the group is waiting for a task that never returns. (We piped
the output through `head` to keep it short.)

So "cooperative" has a cost: a task that ignores cancellation won't stop. The
rule of thumb is that any loop that may run for a long time should either
receive from a queue (whose loop ends on cancellation by itself) or check
`cancelled()` in its condition.

The same applies to loops that do pure computation and never wait at all. The
compiler inserts a scheduling check at the end of every loop iteration so that a
busy task can't starve the other tasks on its worker forever, but that check
doesn't look at cancellation. A long calculation that should be stoppable has to
ask:

```aura
def crunch():
    mut total = 0
    mut i = 0
    while i < 20000:
        if cancelled():
            print(f"crunch stopped early at {i}")
            return
        total += i % 7
        i += 1
    print(f"crunch finished: {total}")

def main():
    with group = TaskGroup():
        group.start_soon(crunch)
        sleep(10ms)
        group.cancel()
    print("group closed")
```

```console
$ aura run src/main.au
crunch stopped early at 122
group closed
```

The number will differ from run to run. Without the `if cancelled()` check, the
same program prints `crunch finished: 59997` after running the whole loop,
cancellation or not. There's also a `yield_now()` builtin, which gives other
tasks on the same worker a chance to run, but it doesn't check cancellation
either.

Why make cancellation cooperative, when it means writing these checks? Because
the alternative, stopping a task at an arbitrary instruction, would leave its
data half-updated and its resources in an unknown state. An Aura task is only
ever cancelled at a point where it was waiting, or where it asked. Everything
between those points runs to completion, so you can reason about it as ordinary
sequential code, and cleanup always starts from a consistent state.

### Cancelling a queue loop

A `for` loop over a queue is one of those waiting points. Here a worker is
cancelled while it's in the middle of a long job:

```aura
def worker(jobs: Queue[int64]):
    for job in jobs:
        print(f"handling {job}")
        sleep(10s)
        if cancelled():
            print(f"cancelled in the middle of job {job}")
            return
    print("worker drained the queue")

def main():
    jobs = Queue[int64]()
    with group = TaskGroup():
        group.start_soon(worker, jobs)
        jobs.put(1)
        jobs.put(2)
        sleep(50ms)
        group.cancel()
    print("group closed")
```

```console
$ aura run src/main.au
handling 1
cancelled in the middle of job 1
group closed
```

The cancellation woke the ten-second `sleep`, the worker saw it and returned,
and job 2 was never started. If the worker had been waiting on the queue
instead, the loop itself would have ended.

## Computing durations

Timeouts and sleeps take a `Duration`. Literals like `10ms`, `2s`, and `1m` are
enough for fixed values. For delays computed at runtime, such as a retry
backoff that grows with each attempt, a `Duration` can be multiplied by an
integer, added, and compared, and there are constructors like `Duration.ms(n)`
and `Duration.seconds(n)`:

```aura
def main():
    base = 10ms
    for attempt in range(1, 4):
        delay = base * attempt
        print(f"attempt {attempt}: waiting {delay}")
        sleep(delay)
    print(Duration.seconds(2) > 1500ms)
```

```console
$ aura run src/main.au
attempt 1: waiting 10ms
attempt 2: waiting 20ms
attempt 3: waiting 30ms
true
```

A negative duration is never a way to say "wait forever". Passing one to
`sleep` traps with `sleep(...) must be non-negative`, and a negative timeout is
rejected the same way. The only way to wait without a limit is to leave the
timeout out.

## Waiting on more than one thing

Sometimes the next event could come from several places: a queue, a task, or a
deadline. `select` waits for whichever comes first. Here a producer sends one
result quickly and then goes quiet, and the main task gives up after 200ms of
silence:

```aura
def produce(results: Queue[str]):
    sleep(30ms)
    results.put("first result")
    sleep(500ms)
    results.put("late result")
    results.close()

def main():
    results = Queue[str]()
    with group = TaskGroup():
        group.start_soon(produce, results)
        mut waiting = true
        while waiting:
            match own select(results, 200ms):
                case SelectOutcome.Queue(index, received):
                    match own received:
                        case QueueReceive.Item(text):
                            print("got " + text)
                        case _:
                            waiting = false
                case SelectOutcome.Task(index, result):
                    pass
                case SelectOutcome.Deadline(index):
                    print("nothing for 200ms, cancelling")
                    group.cancel()
                    waiting = false
                case SelectOutcome.Cancelled:
                    waiting = false
    print("group closed")
```

```console
$ aura run src/main.au
got first result
nothing for 200ms, cancelling
group closed
```

`select` takes any mix of queues, task handles, and durations as arguments, and
returns a `SelectOutcome` saying which one won and its position in the argument
list. Our call has no task arguments, so the `Task` case can't happen here, but
`match` still asks us to write it. If several sources are ready at once, the
one with the lowest position wins, and cancellation of the current task beats
all of them. Only the winner is consumed: a queue that lost keeps its value for
the next receive. With one queue and one deadline, this is the same as
`results.get(timeout=200ms)`; `select` earns its place when you're waiting on,
say, a results queue *and* a supervisor task at the same time. The Manual's
[Concurrency](/manual/concurrency) chapter has the full rules.

## What the group does at the end of its block

We've said many times that a group's exit waits for its tasks. Now we can be
precise about what happens to a task that is still *waiting* when the block
ends.

If the task is making progress, or is waiting for something another live task
can provide, the group waits. A producer blocked on a full queue that a consumer
is draining is fine, however long it takes. The group never decides a wait is
stuck because it has gone on for a long time or the machine is busy.

But if a task is waiting with no timeout for something that no live task can
ever provide, waiting for it would hang the program. The group recognizes that
case and cancels the stuck task:

```aura
def consume(jobs: Queue[int64]):
    match jobs.get():
        case QueueReceive.Item(job):
            print(f"got {job}")
        case QueueReceive.Closed:
            print("closed")
        case QueueReceive.TimedOut:
            print("timed out")
        case QueueReceive.Cancelled:
            print("consumer cancelled: nobody can ever send")

def main():
    jobs = Queue[int64]()
    with group = TaskGroup():
        group.start_soon(consume, jobs)
        print("leaving the block")
    print("group closed")
```

```console
$ aura run src/main.au
leaving the block
consumer cancelled: nobody can ever send
group closed
```

The consumer is waiting on `jobs`, and when `main` reaches the end of the block,
no task is left that could put a value on `jobs` or close it. (The task doing
the joining doesn't count, because it's busy joining.) So the group cancels the
consumer, its `get` returns `QueueReceive.Cancelled`, and shutdown completes.

This protection applies to a group's children at the end of its block. It
doesn't cover a wait in your own code. If `main` itself calls `jobs.get()` on a
queue nobody will ever send to, it waits forever:

```aura
def main():
    jobs = Queue[int64]()
    with group = TaskGroup():
        match jobs.get():
            case QueueReceive.Item(job):
                print(f"got {job}")
            case QueueReceive.Closed:
                print("closed")
            case QueueReceive.TimedOut:
                print("timed out")
            case QueueReceive.Cancelled:
                print("cancelled")
    print("group closed")
```

```console
$ timeout 3 aura run src/main.au
$ echo $?
124
```

Nothing prints, and `timeout` has to kill the program after three seconds
(exit status 124 is `timeout` reporting that it did). The fix is the one from
the previous section: give a wait a timeout when there's any doubt that an
answer will come.

## Shutdown, in order

Putting the pieces together, here is what happens when control reaches the end
of a `with group = TaskGroup():` block, or leaves it early:

1. The group waits for each child that is running or making progress.
2. A child stuck in a wait that no live task can satisfy is cancelled, so it
   wakes and can finish.
3. Each child that ends runs its own `with` exits on the way out, whether it
   returned, trapped, or was cancelled.
4. If a child trapped and nobody observed the failure through its handle, the
   group raises that trap in the parent once the children are done.

Cancellation also reaches nested groups. A child that opens its own
`TaskGroup` and is then cancelled passes the cancellation down to its own
children, as we'll see in the worker pool next.

There is one way for a task's exits not to run, which we met at the end of
[Chapter 14](/book/ch14-02-resource-managers): if the program ends while a task
is still suspended outside any group that waits for it, the runtime abandons it.
With every group opened by `with`, that can't happen, because no block can end
while its tasks are still running. That is the payoff of structured
concurrency: shutdown is not a separate feature you have to design. It is what
the end of a block already does.
