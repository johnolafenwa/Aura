# Passing work with queues

In the last section every task got all of its input when it started and handed
back one value when it finished. Plenty of concurrent programs don't fit that
shape. A producer reads lines from a file and wants to hand each one off as soon
as it has it. A logger receives messages from many tasks for as long as the
program runs. For work that flows between tasks while they run, Aura has
`Queue[T]`: a typed channel that one task puts values into and another takes
them out of.

Let's change the word counter so that a producer task feeds documents to the
main task one at a time:

*Filename: src/main.au*

```aura
def produce(documents: own list[str], lines: Queue[str]):
    for text in own documents:
        lines.put(text)
    lines.close()

def main():
    documents = ["the quick brown fox", "jumps over", "the lazy dog again"]
    lines = Queue[str]()
    mut total = 0
    with group = TaskGroup():
        group.start_soon(produce, documents, lines)
        for line in lines:
            words = len(line.split(" "))
            print(f"{words} words: {line}")
            total += words
    print(f"total: {total}")
```

<span class="caption">Listing 15-3: A producer task feeding the main task through a queue</span>

```console
$ aura run src/main.au
4 words: the quick brown fox
2 words: jumps over
4 words: the lazy dog again
total: 10
```

`Queue[str]()` creates an empty queue of strings. We pass it to `produce` along
with the documents. `produce` puts each document on the queue and then calls
`close()` to say that nothing more is coming. Meanwhile the main task runs a
`for` loop over the queue, which takes values out as they arrive and ends once
the queue has been closed and emptied.

Look at how `lines` is used. We passed it to `produce` with `start_soon`, which
moves its arguments, and then kept using it in the `for` loop. That works
because a queue handle is a copy value. Copying the handle doesn't copy the
queue: both copies refer to the same channel, the way two Python references to
one `queue.Queue` do. That's what makes a queue useful between tasks. The
documents list, by contrast, was moved into `produce`, and `main` can't touch it
any more.

The values going *through* the queue are moved, not shared. `put` moves its
argument into the queue, and each receive moves one value out to the receiver.
At every moment exactly one task owns each string: first the producer, then the
queue, then whoever took it. That's how two tasks on two cores can exchange data
with no locks in your code and no chance of both changing the same value.

## Closing is the signal

The `close()` call is part of the protocol between the two tasks. Without it,
how would the consumer know the producer was done? Python programmers will
recognize the problem: for most of Python's history, `queue.Queue` had no way to
close, and the usual idiom was to put a sentinel such as `None` on the queue
and have every consumer check for it. (Python 3.13 added `Queue.shutdown()` for
the same purpose.) A sentinel works, but it's a value that means "not a value",
and every consumer has to remember to test for it.

In Aura, closing is a state of the queue. After `close()`, `put` fails, and
receivers get every value that was already queued, then learn that the queue is
closed. A `for` loop over a queue ends when any of these happens:

- the queue is closed and empty
- the task running the loop is cancelled
- every task in the surrounding group that could still send to it has finished

The last condition is a safety net: even if a producer forgets to close, the
loop won't wait forever for a value no live task can send. Still, write the
`close()`. It says in one line, in the producer, that the stream is complete,
and it works no matter how the consumer is structured.

A queue loop is always written in the bare form, `for line in lines:`. Each
received value is already owned by the loop binding, and the handle is a copy
value, so the `own` and `mut` modifiers you use with lists have nothing to do
here. The compiler says so if you try:

```aura check-fail:AU3004
def main():
    jobs = Queue[str]()
    jobs.put("index")
    jobs.close()
    for job in own jobs:
        print(job)
```

```console
$ aura run src/main.au
error[AU3004]: Queue iteration receives values; each received item is already owned by the loop binding, and the Queue handle is a copy value, so ownership modifiers have nothing to modify; use the bare form `for item in queue:`
 --> src/main.au:5:5
  |
5 |     for job in own jobs:
  |     ^
```

## Bounded queues and backpressure

`Queue[str]()` is unbounded: `put` always succeeds immediately, and the queue
grows as needed. That's convenient until the producer is faster than the
consumer. Then the queue grows without limit, and a program that reads a large
file can end up with the whole file in memory, waiting in a queue.

A bounded queue fixes the problem by setting how many values may wait at once.
When it's full, `put` waits until a consumer makes room. This is called
*backpressure*: a slow consumer automatically slows the producer down. Let's
watch it with a queue that holds one value:

```aura
def produce(jobs: Queue[int64]):
    for n in range(4):
        jobs.put(n)
        print(f"put {n}")
    jobs.close()

def main():
    jobs = Queue[int64](capacity=1)
    with group = TaskGroup():
        group.start_soon(produce, jobs)
        sleep(100ms)
        for job in jobs:
            print(f"got {job}")
            sleep(50ms)
```

```console
$ aura run src/main.au
put 0
got 0
put 1
got 1
put 2
got 2
put 3
got 3
```

The producer puts `0`, which fills the queue, and then its `put(1)` has to wait.
The main task sleeps for 100ms before it starts reading, and during that time
the producer is stuck: no `put 1` appears until `got 0` has made room. From
then on the two tasks move in lockstep. With an unbounded queue, all four `put`
lines would have printed at once.

A waiting `put` doesn't block the scheduler worker it runs on. The task is
parked, and other tasks on the same worker keep running until there's room.

### Sending without waiting

Sometimes waiting is the wrong response to a full queue. A task that polls for
work, or a request handler that should shed load rather than pile it up, wants
to know right away. `try_put` sends only if there is room now. It returns a
`Result`, and when the send fails, the error hands the value back:

```aura
def offer(jobs: Queue[str], job: own str):
    match jobs.try_put(job):
        case Result.Ok(_):
            print("queued")
        case Result.Err(SendError.Full(rejected)):
            print("full, kept " + rejected)
        case Result.Err(SendError.Closed(rejected)):
            print("closed, kept " + rejected)
        case Result.Err(SendError.TimedOut(rejected)):
            print("timed out, kept " + rejected)
        case Result.Err(SendError.Cancelled(rejected)):
            print("cancelled, kept " + rejected)

def main():
    jobs = Queue[str](capacity=2)
    offer(jobs, "index")
    offer(jobs, "render")
    offer(jobs, "upload")
    print(jobs.poll())
    offer(jobs, "upload")
    jobs.close()
    offer(jobs, "notify")
    for job in jobs:
        print("draining " + job)
```

<span class="caption">Listing 15-4: `try_put` on a full queue and a closed one</span>

```console
$ aura run src/main.au
queued
queued
full, kept upload
Poll.Ready(index)
queued
closed, kept notify
draining render
draining upload
```

This listing runs on one task, which makes it easy to follow. The queue holds
two values, so the third `offer` finds it full. `jobs.poll()` takes one value
out without waiting, which is why there's room for `upload` the second time.
After `close()`, the `notify` offer fails with `SendError.Closed`, and the loop
drains the two values still in the queue before ending. Closing a queue doesn't
throw away what's already in it.

The detail to notice is `SendError.Full(rejected)`. Because `put` and `try_put`
take ownership of the value, a failed send would destroy it if the error didn't
give it back. Every `SendError` case carries the unsent value, so the caller can
retry, log it, or route it to another queue. `SendError` has four cases, one for
each way a send can fail: the queue was full, it was closed, the wait timed out,
or the task was cancelled while waiting. `try_put` never waits, so it only
produces the first two in practice, but the same type is used everywhere so one
`match` handles all sends.

## Waiting with a timeout

Both directions accept a timeout. A `put` with a timeout waits for room at most
that long, and a `get` waits for a value at most that long:

```aura
def main():
    jobs = Queue[str](capacity=1)
    jobs.put("index")
    match jobs.put("render", timeout=50ms):
        case Result.Ok(_):
            print("queued")
        case Result.Err(SendError.TimedOut(job)):
            print("no room for " + job + " after 50ms")
        case Result.Err(error):
            print("some other failure")
    match jobs.get(timeout=20ms):
        case QueueReceive.Item(job):
            print("got " + job)
        case QueueReceive.Closed:
            print("closed")
        case QueueReceive.TimedOut:
            print("nothing within 20ms")
        case QueueReceive.Cancelled:
            print("cancelled")
    match jobs.get(timeout=20ms):
        case QueueReceive.Item(job):
            print("got " + job)
        case QueueReceive.Closed:
            print("closed")
        case QueueReceive.TimedOut:
            print("nothing within 20ms")
        case QueueReceive.Cancelled:
            print("cancelled")
    print(jobs.get_or("nothing", timeout=20ms))
```

```console
$ aura run src/main.au
no room for render after 50ms
got index
nothing within 20ms
nothing
```

`get` returns a `QueueReceive[T]`, which has a case for each way a receive can
end: an item arrived, the queue is closed, the time ran out, or the task was
cancelled. Like `TaskResult`, it keeps "closed" and "timed out" apart, and that
matters. A closed queue means the stream is finished and you should stop. A
timeout means nothing arrived *yet*, and the right response depends on your
program: try again, report that a producer is slow, or give up.

For the common cases there are two shorter forms. `poll(timeout=...)` returns
`Poll.Ready(value)` or `Poll.Unavailable`, and `get_or(default, timeout=...)`
returns the value or the default you pass. Without a timeout, both check once and
return immediately, as `jobs.poll()` did in Listing 15-4.

One kind of wait is missing from that list: waiting with no timeout for a value
that can never come. We'll see in the next section that the group's exit is
careful about this case, but your own code isn't protected. If the main task
calls `jobs.get()` on a queue that no task will ever send to or close, it waits
forever. When there is any doubt about whether an answer will come, pass a
timeout.

## What can go through a queue

A queue moves values between tasks that may run on different workers, so its
payload type must be `Transfer`, the same rule as for task arguments. The
compiler checks it where the queue is created:

```aura check-fail:AU3008
import random

def main():
    generators = Queue[random.Rng]()
```

```console
$ aura run src/main.au
error[AU3008]: Queue payload `random.Rng` cannot cross a worker boundary because `random.Rng` is a stateful generator and is not Transfer
 --> src/main.au:4:18
  |
4 |     generators = Queue[random.Rng]()
  |                  ^
  = help: use a Queue payload made only from Transfer components; keep capabilities and host resources on their owning worker
```

A random number generator is mutable state that belongs to one task. If two
tasks could draw from the same generator, they would race on its internal state.
The fix is to give each task its own generator, seeded from a number you pass
it. Your own classes are fine as payloads as long as their fields are
transferable, and we'll send a `Job` class through a queue in
[Section 15.4](/book/ch15-04-worker-pool).

## When a producer fails

What happens to a consumer when the producer feeding it traps before it can
close the queue? This is where unstructured programs usually hang: the consumer
waits forever for a value that will never arrive.

```aura
def produce(jobs: Queue[int64]):
    jobs.put(1)
    jobs.put(2)
    assert false, "producer broke"
    jobs.close()

def consume(jobs: Queue[int64]):
    for job in jobs:
        print(f"got {job}")
    print("consumer done")

def main():
    jobs = Queue[int64]()
    with group = TaskGroup():
        group.start_soon(produce, jobs)
        group.start_soon(consume, jobs)
    print("after the group")
```

```console
$ aura run src/main.au
got 1
got 2
consumer done
error[AU4001]: producer broke
 --> src/main.au:4:5
  |
4 |     assert false, "producer broke"
  |     ^
  = note: Aura call chain (innermost first): produce at 1:1
  = note: Aura task entry: produce at 1:1
  = note: Aura task ancestry (youngest first): produce spawned from main at 15:15
```

The consumer received the two values that were sent, and then its loop ended,
because the only task that could have sent more is gone. The program didn't
hang. The group then raised the producer's unread failure, so the error is
reported with the line that caused it, and `after the group` never printed.
The queue and the group work together here: the group knows which tasks are
alive, so a queue loop inside it can tell "nothing yet" from "nothing ever".

We now have tasks that start, finish, fail, and talk to each other. What we
can't do yet is stop them. A slow producer, a stuck worker, an answer that
arrived from somewhere else first: all of these need a way to tell running
tasks that their work is no longer wanted, and that's cancellation.
