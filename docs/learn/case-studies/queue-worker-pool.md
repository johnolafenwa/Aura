# Case Study: A Queue Worker Pool

This case study builds a small worker pool. The interesting part is not parallel speed; it is the shape of the program. A parent scope owns the task group, queues move owned jobs between tasks, producers close queues when they are finished, and the parent can cancel the whole thing deliberately when it has seen enough.

The program is a good template to copy.

## The Pieces

The pool needs four things:

- a **`Job`** type for the work being done
- a **producer** that supplies jobs to a queue
- one or more **consumers** that read jobs and produce results
- a **parent** that owns the task group and decides when the pool has finished

`TaskGroup` is where the parent lives. `Queue[Job]` and `Queue[String]` are the two channels. `QueueReceive[T]` describes what a receive produces. Cancellation is the tool the parent uses when it stops waiting.

## Step 1: The Work Itself

A job is a small record, and the worker function that processes it takes one by value:

```python
class Job:
    id: int32
    payload: String

def handle(job: Job) -> String:
    return "done " + job.id.to_string() + " " + job.payload
```

`handle` consuming the job is the right choice: once a worker starts on a unit of work, no other part of the program needs that owned value.

## Step 2: The Producer

A producer puts jobs into a queue and closes the queue when it has nothing more to send:

```python
def produce(jobs: Queue[Job]):
    jobs.put(Job(id=1, payload="index"))
    jobs.put(Job(id=2, payload="render"))
    jobs.put(Job(id=3, payload="upload"))
    jobs.close()
```

Closing is part of the protocol. A consumer that sees the queue close knows its receive loop is finished normally, not "maybe more work later." Queue handles are copy values, so passing `jobs` into the producer does not remove it from the parent's scope.

## Step 3: The Consumer

A consumer reads jobs until the queue closes, the surrounding task group is cancelled, or all producers finish:

```python
def consume(name: String, jobs: Queue[Job], results: Queue[String]):
    for job in jobs:
        result = name + ": " + handle(job)
        results.put(result)
```

The `for` loop does the receive structurally. There is no sentinel value, no magic token, no special return code — closing the queue is the signal.

## Step 4: The Parent

The parent owns the task group, the queues, and the decision about when the pool has finished:

```python
jobs = Queue[Job](capacity=8)
results = Queue[String]()

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
4. Cancels the group if the pool has stopped making progress.
5. Leaves the `with` block, which waits for every child to finish.

## Closing Result Queues

This example knew how many results to expect, so counting is the simplest shape. When the number is not known ahead of time, there are two reasonable options:

- Let the final worker close the result queue. This works, but it requires coordination: one worker must not close while another still needs to send.
- Introduce a coordinator task that owns the decision to close. This is usually cleaner; the worker pool does its work and an extra task owns the lifecycle question.

Avoid making every worker guess. The benefit of structured concurrency is knowing exactly who is responsible for each decision.

## Bounded Queues And Backpressure

`Queue[Job](capacity=8)` limits how many jobs can be in flight at once. When workers are slower than the producer, `put` waits for space — which stops memory from growing without bound when the workload is bursty.

When the producer should fail fast instead of waiting, use a timeout:

```python
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

The unsent job comes back inside the `SendError` variant, so the caller can log it, retry later, or feed it to a different queue.

`try_put` is the non-blocking variant — useful for polling-style producers.

## Why This Shape Works

The parent scope owns the concurrency. Jobs are owned values, not shared mutable state. Queues are the only communication path between the producer and the workers. Cancellation is visible and comes from one place. No task runs without a scope that will eventually wait for it.

If you can answer "which task created this, and which scope waits for it" for every task in the program, the program is on the right track. This is the concurrency style to copy first.
