# A worker pool

We now have every piece we need for the most common shape in concurrent
programs: a pool of workers. One task produces jobs. Several workers take jobs
from a shared queue, process them, and send results to a second queue. The
parent collects the results and decides when the whole thing is finished. The
same shape runs build systems, web crawlers, and image converters, and in
[Chapter 19](/book/ch19-00-final-project) we'll use it for a job runner.

Our jobs are the word-counting jobs from earlier in the chapter. Counting words in
five sentences won't keep three cores busy, so watch the structure rather than
the speed: who owns each decision, and what happens when something goes
wrong.

## The pieces

Here is the whole first version. We'll walk through it from the top.

*Filename: src/main.au*

```aura
class Job:
    id: int64
    text: str

class Count:
    id: int64
    words: int64
    worker: str

def count_words(job: own Job) -> int64:
    return len(job.text.split(" "))

def produce(documents: own list[str], jobs: Queue[Job]):
    mut id = 0
    for text in own documents:
        jobs.put(Job(id=id, text=text))
        id += 1
    jobs.close()

def work(name: str, jobs: Queue[Job], results: Queue[Count]):
    for job in jobs:
        id = job.id
        words = count_words(job)
        results.put(Count(id=id, words=words, worker=name.clone()))

def run_workers(count: int64, jobs: Queue[Job], results: Queue[Count]):
    with workers = TaskGroup():
        for n in range(count):
            workers.start_soon(work, f"worker-{n}", jobs, results)
    results.close()

def main():
    documents = [
        "the quick brown fox",
        "jumps over the lazy dog",
        "structured concurrency keeps tasks inside a scope",
        "queues move owned values",
        "every task has a parent",
    ]
    jobs = Queue[Job](capacity=2)
    results = Queue[Count]()
    mut total = 0
    with group = TaskGroup():
        group.start_soon(produce, documents, jobs)
        group.start_soon(run_workers, 3, jobs, results)
        for result in results:
            print(f"document {result.id}: {result.words} words ({result.worker})")
            total += result.words
    print(f"total: {total} words")
```

<span class="caption">Listing 15-7: A worker pool with a producer, three workers, and a results queue</span>

```console
$ aura run src/main.au
document 0: 4 words (worker-0)
document 1: 5 words (worker-1)
document 2: 7 words (worker-1)
document 3: 4 words (worker-1)
document 4: 5 words (worker-1)
total: 25 words
```

Your output will probably differ in which worker handled which document, and
possibly in the order of the lines, because both depend on scheduling. The
total is always 25.

`Job` and `Count` are ordinary classes. Both are made of integers and strings,
so the compiler derives `Transfer` for them, and they can travel through queues.
We didn't have to declare anything.

`produce` owns the documents (they were moved into it when it started), turns
each into a `Job` with an id, and puts it on the `jobs` queue. When it runs out
of documents, it closes the queue. That `close()` is the producer's one
decision: it is the only task that knows when the input is finished.

`work` is the worker. It loops over the shared `jobs` queue. Because each
receive moves one job out of the queue, two workers can never get the same job,
and no locking is needed to share the work out. Each worker counts the words and
sends a `Count` to `results`, tagged with its own name.

Look at the order of the first two lines of the loop body. We copy `job.id`
into a binding *before* calling `count_words(job)`. `count_words` takes the job
as `own Job`, so after the call the worker no longer has it. If we write the
obvious version instead, reading `job.id` after the call, the compiler stops us
with an `AU3001`, "use of moved value `job`", pointing at `job.id` and at the
call where the job moved. Consuming the job is the right design: once a worker
has started on a unit of work, nothing else in the program needs it. We only
have to take the bits we still want before handing it over.

### Who closes the results queue?

The `results` queue is harder than `jobs`. Three workers send to it, and none of
them knows when the *others* are finished, so no single worker can safely close
it. If the first worker to finish closed it, the other two would find their
`put` failing with `SendError.Closed`.

The answer is `run_workers`, a small coordinator task whose only job is the
lifecycle of the workers. It opens its own `TaskGroup`, starts the workers in
it, and lets the block end. Because the block's exit waits for every worker, the
line after it, `results.close()`, runs exactly when the last worker has sent its
last result. The rule that a group waits for its tasks turns "close it when
everyone is done" from a coordination problem into a line of code.

This is a nested group: `main`'s group contains `run_workers`, and
`run_workers`'s group contains the three workers. Every task still has an
answer to the two questions from the start of the chapter. The workers were
started by `run_workers`'s block and are waited for by it. `produce` and
`run_workers` were started by `main`'s block and are waited for by it.

(The `for` loop in `main` would actually end even without the `close()`,
because once every task that could send to `results` has finished, a queue loop
inside the group stops waiting, as we saw in
[Section 15.2](/book/ch15-02-queues). Write the `close()` anyway. It states the
protocol in one place, and it keeps working if you later move the collecting
loop somewhere that can't see those tasks.)

### The parent sets the shape

`main` decides the things that are about the system as a whole: how many
workers to start, and how big the jobs queue is. `capacity=2` means the
producer can get at most two jobs ahead of the workers. With five short
sentences that makes no difference, but with a million lines from a file, it's
the difference between a fixed amount of memory and the whole file sitting in a
queue.

`results` is left unbounded, and that's deliberate. If it were bounded and the
parent were slow to read, workers would block on `results.put`, stop taking
jobs, and the producer would block on `jobs.put`: backpressure all the way up,
which is often what you want. Here the parent reads results as fast as they
come, so a bound would only add waiting.

## When a job fails

Let's see what happens when one document breaks a worker. Add an assertion as
the first line of `count_words`, and replace the fourth document with an empty
string:

```aura fragment
def count_words(job: own Job) -> int64:
    assert len(job.text) > 0, f"job {job.id} is empty"
    return len(job.text.split(" "))
```

```console
$ aura run src/main.au
document 1: 5 words (worker-0)
document 2: 7 words (worker-0)
document 0: 4 words (worker-1)
document 4: 5 words (worker-1)
error[AU4001]: job 3 is empty
 --> src/main.au:11:5
   |
11 |     assert len(job.text) > 0, f"job {job.id} is empty"
   |     ^
  = note: left = 0
  = note: right = 0
  = note: Aura call chain (innermost first): count_words at 10:1 -> work at 21:1
  = note: Aura task entry: work at 21:1
  = note: Aura task ancestry (youngest first): work spawned from run_workers at 30:21 -> run_workers spawned from main at 46:15
```

The other workers carried on and handled the remaining documents. Then the
failure surfaced: the worker's trap was unread in `run_workers`'s group, so it
was raised there, which made `run_workers` fail, which was unread in `main`'s
group, so it was raised in `main`. The program stopped with the diagnostic, and
`total` was never printed.

Read the last note. The task ancestry traces the failing task back through
every group: a `work` task started by `run_workers` at line 30, which was
itself started by `main` at line 46. In a program with hundreds of tasks, that
chain is how you find the one that failed. It exists because every task has a
parent; a detached thread has no ancestry to report.

Whether a bad job should stop the pool is a design decision. Here it's a bug in
the input, and stopping loudly is reasonable. If bad jobs are expected, the
better design is to make `count_words` return a `Result` and have the worker
send the error along as a result, so the parent can report it and keep going.
[Chapter 9](/book/ch09-00-error-handling) has the trade-offs between trapping and
returning errors, and they apply to tasks unchanged.

## Giving up on a stuck pool

The last thing our pool needs is a deadline. If a job hangs (a network call that
never answers, a file on a dead disk), the pool should notice and shut down
instead of waiting forever. We'll simulate a hang with a document whose text is
`stall`, and have the parent cancel everything if no result arrives for a
second:

*Filename: src/main.au*

```aura
class Job:
    id: int64
    text: str

class Count:
    id: int64
    words: int64
    worker: str

def count_words(job: own Job) -> int64:
    if job.text == "stall":
        sleep(10s)
    return len(job.text.split(" "))

def produce(documents: own list[str], jobs: Queue[Job]):
    mut id = 0
    for text in own documents:
        jobs.put(Job(id=id, text=text))
        id += 1
    jobs.close()

def work(name: str, jobs: Queue[Job], results: Queue[Count]):
    for job in jobs:
        id = job.id
        words = count_words(job)
        if cancelled():
            return
        results.put(Count(id=id, words=words, worker=name.clone()))

def run_workers(count: int64, jobs: Queue[Job], results: Queue[Count]):
    with workers = TaskGroup():
        for n in range(count):
            workers.start_soon(work, f"worker-{n}", jobs, results)
    results.close()

def collect(results: Queue[Count], group: TaskGroup) -> int64:
    mut total = 0
    while true:
        match results.get(timeout=1s):
            case QueueReceive.Item(result):
                print(f"document {result.id}: {result.words} words ({result.worker})")
                total += result.words
            case QueueReceive.TimedOut:
                print("no result for 1s, cancelling the pool")
                group.cancel()
                return total
            case QueueReceive.Closed:
                return total
            case QueueReceive.Cancelled:
                return total
    return total

def main():
    documents = [
        "the quick brown fox",
        "jumps over the lazy dog",
        "stall",
        "queues move owned values",
        "every task has a parent",
    ]
    jobs = Queue[Job](capacity=2)
    results = Queue[Count]()
    with group = TaskGroup():
        group.start_soon(produce, documents, jobs)
        group.start_soon(run_workers, 3, jobs, results)
        total = collect(results, group)
        print(f"total: {total} words")
    print("pool closed")
```

<span class="caption">Listing 15-8: A worker pool that cancels itself when it stops making progress</span>

```console
$ time aura run src/main.au
document 0: 4 words (worker-0)
document 1: 5 words (worker-0)
document 3: 4 words (worker-0)
document 4: 5 words (worker-0)
no result for 1s, cancelling the pool
total: 18 words
pool closed

real	0m1.071s
user	0m0.209s
sys	0m0.016s
```

Three things changed from Listing 15-7.

The collecting loop moved into a function, `collect`, and uses
`results.get(timeout=1s)` instead of a `for` loop, so that it can tell "the
pool is finished" (`Closed`) apart from "the pool has gone quiet" (`TimedOut`).
On a timeout, it calls `group.cancel()`. `collect` only borrows the group, the
same way `start_reports` did in [Section 15.1](/book/ch15-01-tasks-and-task-groups);
the group still belongs to `main`'s block. (The `return total` after the
`while true` loop can never run, but the current compiler reports
``function `collect` is missing a return`` without it, so we keep it.)

The workers check `cancelled()` after each job, so a worker whose job was cut
short by cancellation doesn't send a meaningless result.

And `count_words` stalls on one document, with a ten-second `sleep` standing in
for a hung operation.

Now read the output. Four documents were counted, all by `worker-0` in this
run, while another worker sat on the `stall` job. After a second of silence,
`collect` cancelled the group. The program finished in a little over a second, not
ten, even though the stalled `sleep` was in a task two groups down: cancelling
`main`'s group cancelled `run_workers`, whose group passed the cancellation on
to its workers. The stalled worker's `sleep` woke early, it saw `cancelled()`,
and it returned without sending. `run_workers`'s block ended, `main`'s block
ended, and `pool closed` printed.

Every decision in the program has exactly one owner. `main` decides the shape
of the pool and when to give up. `produce` decides when the input ends.
`run_workers` decides when the output ends. Each worker owns the one job it's
working on. There is no shared mutable state anywhere, and every task is
waited for by the block that started it. If you take one pattern from this
chapter into your own programs, take this one.

## Summary

Concurrency in Aura is structured. A task is started by a `TaskGroup`, a group
is a resource managed by `with`, and a group's exit waits for every task it
started, so no task outlives its block. The compiler keeps the group from
escaping that block, and a failure in a task always reaches either the code that
reads its result or the block that owns it. Tasks share nothing except the
channels between them: arguments and results move into and out of tasks, queues
move owned values from one task to another, and the compiler checks that
everything crossing those boundaries is `Transfer`.

You can now start tasks with `start` and `start_soon`, wait for them with
`result`, `wait_all`, and `wait_any`, pass work through bounded and unbounded
queues, apply backpressure, and handle every outcome of a wait: a value, a
failure, a closed queue, a timeout, a cancellation. You know that a timeout
stops waiting but not the task, that cancellation is a cooperative request that
long-running loops must check, and that a group's shutdown waits for progress
and cancels only waits that nothing can ever satisfy.

So far all our tasks have talked to each other. In the next chapter they start
talking to the world: reading and writing files, running other programs, and
making network requests. Those operations are where real programs wait the
most, and everything from this chapter about timeouts, cancellation, and `with`
blocks applies to them directly.
