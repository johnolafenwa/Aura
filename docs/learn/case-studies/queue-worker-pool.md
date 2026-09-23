# Case Study: A Queue Worker Pool

This case study builds a small worker pool. The point is the program's shape, not parallel speed:

- A parent scope owns the task group.
- Queues move owned jobs between tasks.
- Producers close queues when they finish.
- The parent can cancel the whole pool once it has seen enough.

The program is a good template to copy.

## The Pieces

The pool needs four things:

- a **`Job`** type for the work
- a **producer** that puts jobs on a queue
- one or more **consumers** that read jobs and produce results
- a **parent** that owns the task group and decides when the pool is done

The parent lives in a `TaskGroup`. `Queue[Job]` and `Queue[str]` are the two channels. `QueueReceive[T]` describes what a receive produces. The parent uses cancellation when it stops waiting.

## Step 1: The Work Itself

A job is a small record. The worker function takes ownership of it:

```aura
class Job:
    id: int32
    payload: str

def handle(job: own Job) -> str:
    return "done " + job.id.to_string() + " " + job.payload
```

Consuming the job is the right choice. Once a worker starts on a unit of work, no other part of the program needs that owned value.

## Step 2: The Producer

A producer puts jobs on a queue and closes the queue when it has nothing more to send:

```aura fragment
def produce(jobs: Queue[Job]):
    jobs.put(Job(id=1, payload="index"))
    jobs.put(Job(id=2, payload="render"))
    jobs.put(Job(id=3, payload="upload"))
    jobs.close()
```

Closing is part of the protocol. A consumer that sees the queue close knows its receive loop ended normally, and that no more work will come.

Queue handles are copy values, so passing `jobs` to the producer does not remove it from the parent's scope.

## Step 3: The Consumer

A consumer reads jobs until the queue closes, the task group is cancelled, or all producers finish:

```aura fragment
def consume(name: str, jobs: Queue[Job], results: Queue[str]):
    for job in jobs:
        result = f"{name}: {handle(job)}"
        results.put(result)
```

The plain `for` loop performs each receive, and each item arrives already owned by `job`. Queue iteration is not a walk over a collection's places, so the `own` and `mut` modifiers are rejected here.

There is no sentinel value, magic token, or special return code. Closing the queue is the signal.

## Step 4: The Parent

The parent owns the task group, the queues, and the decision about when the pool is done:

```aura fragment
jobs = Queue[Job](capacity=8)
results = Queue[str]()

with group = TaskGroup():
    group.start_soon(produce, jobs)
    group.start_soon(consume, "worker-a", jobs, results)
    group.start_soon(consume, "worker-b", jobs, results)

    mut received = 0
    while received < 3:
        match results.get(timeout=1s):
            case QueueReceive.Item(text):
                print(text)
                received += 1
            case QueueReceive.TimedOut:
                print("worker pool timed out")
                group.cancel()
                break
            case QueueReceive.Closed:
                break
            case QueueReceive.Cancelled:
                break
```

The parent does five things, in order:

1. Starts the producer.
2. Starts the workers.
3. Collects the expected number of results.
4. Cancels the group if the pool stops making progress.
5. Leaves the `with` block, which waits for every child to finish.

## Closing Result Queues

This example knows how many results to expect, so counting is the simplest shape. When the count is unknown, you have two reasonable options:

- **Let the final worker close the result queue.** This works but needs coordination. One worker must not close the queue while another still needs to send.
- **Add a coordinator task that decides when to close.** This is usually cleaner. The workers do the work, and one extra task owns the lifecycle decision.

Do not make every worker guess. Structured concurrency pays off when you know exactly who owns each decision.

## Bounded Queues And Backpressure

`Queue[Job](capacity=8)` limits how many jobs can be in flight at once. When workers are slower than the producer, `put` waits for space. That keeps memory from growing without bound under a bursty workload.

Pass a timeout when the producer should fail fast:

```aura fragment
match jobs.put(Job(id=4, payload="notify"), timeout=100ms):
    case Result.Ok(_):
        pass
    case Result.Err(SendError.TimedOut(job)):
        print("could not queue " + job.payload)
    case Result.Err(SendError.Closed(job)):
        print("closed")
    case Result.Err(SendError.Cancelled(job)):
        print("cancelled")
    case Result.Err(SendError.Full(job)):
        print("full")
```

The unsent job comes back inside the `SendError` variant. The caller can log it, retry later, or send it to a different queue.

`try_put` is the non-blocking variant. Use it for polling-style producers.

## Why This Shape Works

- The parent scope owns the concurrency.
- Jobs are owned values, not shared mutable state.
- Queues are the only path between the producer and the workers.
- Cancellation is visible and comes from one place.
- Every task runs inside a scope that waits for it.

Ask two questions of every task in a program: which task created it, and which scope waits for it? If you can always answer both, the program is on the right track. Copy this concurrency style first.
