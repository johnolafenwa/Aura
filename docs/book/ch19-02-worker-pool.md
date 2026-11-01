# Running jobs in parallel with a worker pool

The single-task runner spends most of its life waiting. While `nap` sleeps,
the runner sits in `process.run` doing nothing, and every job behind it in the
file waits too. Let's make that cost visible before we fix it. Here is a job
file with three one-second naps:

*Filename: jobs.txt*

```text
# Each line is `name: command`. Blank lines and # comments are skipped.
greet: echo hello from the job runner
count: ls /etc | wc -l
nap-1: sleep 1; echo rested
nap-2: sleep 1; echo rested
nap-3: sleep 1; echo rested
broken: echo about to fail >&2; exit 3
```

Running Listing 19-4 against it, under the shell's `time` command:

```console
$ time aura run src/main.au
[ok  ] greet (6 ms) hello from the job runner
[ok  ] count (7 ms) 155
[ok  ] nap-1 (1009 ms) rested
[ok  ] nap-2 (1009 ms) rested
[ok  ] nap-3 (1007 ms) rested
[FAIL] broken (7 ms) ExitStatus.Exited(3) about to fail
6 jobs, 1 failed

real	0m3.124s
user	0m0.193s
sys	0m0.024s
```

Three seconds of wall-clock time, of which the CPU was busy for about a fifth
of a second. The jobs are independent, so there is no reason the naps could
not overlap. Let's run them through a worker pool: a fixed
number of tasks that each pull jobs from a shared queue, run them with the
`run_job` function we already have, and send reports back.

## The shape of the pool

Chapter 15 built a worker pool out of three kinds of participants, and ours
has the same three:

1. A *producer* task puts each job on a `Queue[Job]`, then closes the queue.
2. Several *worker* tasks read jobs from that queue until it is closed. Each
   worker runs a job, then puts the `Report` on a second queue,
   `Queue[Report]`.
3. The *parent*, `main`, owns the task group. It starts the producer and the
   workers, collects one report per job, and prints them.

Every piece of this has exactly one owner. A job is owned by the producer
until it goes on the queue, then by whichever single worker receives it. A
report is owned by the worker that built it until it goes on the reports
queue, then by the parent. No two tasks ever touch the same job or the same
report, so there is nothing to lock. That is the share-nothing model Aura's
tasks are built around: values cross between tasks by moving through queues
and task arguments, and the compiler checks that whatever crosses is safe to
move.

## The producer and the workers

Let's write the two task functions first and leave `main` alone for a moment.
The producer takes the whole list of jobs and puts each one on the queue:

```aura fragment
def produce(jobs: own list[Job], queue: Queue[Job]):
    for job in jobs:
        queue.put(job)
    queue.close()


def work(queue: Queue[Job], reports: Queue[Report]):
    for job in queue:
        reports.put(run_job(job))
```

The worker is short because all the real work already lives in `run_job`. A
`for` loop over a queue receives one item at a time, waits when the queue is
empty, and ends when the queue is closed. The producer is where the checker
has something to say:

```console
$ aura check src/main.au
error[AU3002]: cannot move borrowed value `job`
 --> src/main.au:58:19
   |
58 |         queue.put(job)
   |                   ^
  = related src/main.au:57:9: value is borrowed here
  = help: take `job` as `own Job` when ownership of this non-cloneable value is required
```

We declared `jobs: own list[Job]`, so `produce` owns the list. But a plain
`for job in jobs:` loop does not take the elements out of the list. It lends
each one to the loop body in turn, and the list keeps them, the same way a
Python `for` loop leaves the list intact. `queue.put` takes its value as
`own T`, because the job is about to leave for another task, and you cannot
give away something you have only borrowed.

What we want is a loop that consumes the list, handing us each job as an
owned value. Aura spells that by putting `own` on the iterable:

```aura fragment
def produce(jobs: own list[Job], queue: Queue[Job]):
    for job in own jobs:
        queue.put(job)
    queue.close()
```

After `for job in own jobs`, the list is used up, and that is fine because the
producer has no further use for it. The two spellings make the choice visible
at the loop: borrow each element, or take them all.

The `queue.close()` at the end matters as much. Workers loop until the
queue is closed, so closing it is how the producer says "that was the last
job". There is no sentinel value and no special "stop" job.

## The parent

Now let's replace the sequential loop in `main` with the pool. We create the
two queues, open a task group, start one producer and three workers, and then
receive exactly as many reports as there were jobs:

```aura fragment
        case Result.Ok(text):
            jobs = parse_jobs(text)
            queue = Queue[Job](capacity=4)
            reports = Queue[Report]()
            mut failed = 0
            with group = TaskGroup():
                group.start_soon(produce, jobs, queue)
                for _ in range(3):
                    group.start_soon(work, queue, reports)
                for _ in range(len(jobs)):
                    match reports.get():
                        case QueueReceive.Item(report):
                            print_report(report)
                            if not report.ok:
                                failed += 1
                        case _:
                            break
            print(f"{len(jobs)} jobs, {failed} failed")
```

The checker points out a problem we created by moving things around:

```console
$ aura check src/main.au
error[AU3001]: use of moved value `jobs`
 --> src/main.au:86:36
   |
86 |                 for _ in range(len(jobs)):
   |                                    ^
  = related src/main.au:83:43: value moved here
  = help: pass shared access when ownership is not needed, or call `.copy()` at the move site when an independent value is required
  = fix: replace src/main.au:83:47-83:47 with `.copy()`
```

`group.start_soon(produce, jobs, queue)` moved `jobs` into the producer task.
That move is not optional: the producer will run on its own schedule, possibly
on another core, and could still be using the list long after this line. If
the parent could keep reading `jobs` too, two tasks would share one list with
no coordination, which is the data race that structured concurrency is meant
to rule out. So starting a task takes ownership of its arguments, and `jobs`
is gone from `main` afterward.

The help line suggests `.copy()`, which would make a second list of jobs. That
compiles, but it copies every job to learn a single number. All the parent
needs is the count, and the count is a plain `int64` we can read before the
move:

```aura fragment
            jobs = parse_jobs(text)
            total = len(jobs)
```

Then `range(total)` and `print(f"{total} jobs, ...")` use the copy value, and
the list goes to the producer untouched. The queues are a different story.
We passed `queue` to the producer and to all three workers, and passed
`reports` three times as well, and the checker did not object. Queue handles
are copy values: each task gets its own handle, and every handle refers to the
same underlying queue. That is what makes a queue useful as a channel.

With `total` in place, let's run it:

```console
$ time aura run src/main.au
[ok  ] greet (4 ms) hello from the job runner
[ok  ] count (6 ms) 155
[ok  ] nap-1 (1005 ms) rested
[ok  ] nap-2 (1006 ms) rested
[FAIL] broken (7 ms) ExitStatus.Exited(3) about to fail
[ok  ] nap-3 (1007 ms) rested
6 jobs, 1 failed

real	0m1.105s
user	0m0.204s
sys	0m0.020s
```

The total time dropped from three seconds to a little over one. Each nap still
took a second, but the three of them ran at the same time on three workers.
The order of the reports changed too: `broken` finished before `nap-3`
reported. Reports arrive in the order jobs finish, not the order they appear
in the file, and if you run this a few times you will see that order vary.
For a job runner that is the behavior we want, since it lets us print each
result the moment it is ready.

Why do the naps overlap so cleanly? A worker
calling `process.run` is waiting on a child, and process waits are one of the
points where Aura's scheduler can switch to other runnable tasks. Even with
`AURA_WORKERS=1`, which pins every task to a single OS thread, the three naps
still overlap, because none of the workers is using the CPU while it waits.

## Why the parent counts reports

The workers never close the `reports` queue, and the parent's receive loop
does not wait for it to close. It counts instead: it knows how many jobs
there are, so it calls `reports.get()` exactly `total` times. This is the
simplest correct way to end the collection loop, and it avoids a subtle
problem. If each worker closed `reports` when it ran out of jobs, the first
worker to finish would close the queue while the others still had reports to
send. Deciding when a shared queue is finished belongs to one owner, and here
the parent is the only participant that knows the total.

`reports.get()` returns a `QueueReceive[Report]`. `Item` carries a report; the
other variants, `Closed`, `TimedOut`, and `Cancelled`, cannot happen in this
version of the program, so a single `case _: break` covers them. We will
start caring about `TimedOut` in the next section, when the parent gets a
deadline.

When the loop ends, the parent leaves the `with group = TaskGroup():` block,
and leaving the block waits for every task in the group to finish. By then
the producer has closed `queue`, so each worker's `for` loop ends and its task
returns. No task outlives the block. That is the guarantee structured
concurrency gives us, and it is why the runner needs no "join" calls and no
bookkeeping of which tasks are still alive.

## Bounded queues and backpressure

We created the job queue with `Queue[Job](capacity=4)`. A bounded queue holds
at most that many items. When it is full, `put` waits until a worker takes
something out. Our producer is much faster than our workers, since it only
moves values around, so with an unbounded queue it would push the entire job
list onto the queue before the first job finished. That is harmless with six
jobs, but a runner fed a file with a million lines would end up holding a
million jobs in the queue at once. With a bound, the producer runs at most a
few jobs ahead of the workers. This is called backpressure: a slow consumer
slows the producer down, instead of letting work pile up in memory.

The reports queue is unbounded. Reports only accumulate if the parent stops
reading, and the parent's whole job is to read them, so a bound would buy us
nothing there.

## Choosing the number of workers

Three workers is an arbitrary number. The right count depends on the jobs:
CPU-heavy jobs want roughly one worker per core, while jobs that mostly wait
on the network or on the disk can usefully run many more at once. That is the
user's decision, so let's make it a command-line option. Here is the finished
version of this section's program. `parse_jobs`, `make_report`, `run_job`, and
`print_report` are unchanged from Listing 19-4, so the listing elides them:

*Filename: src/main.au*

```aura fragment
import fs
import process
import sys


class Job:
    name: str
    command: str


class Report:
    name: str
    ok: bool
    detail: str
    elapsed_ms: int64


# parse_jobs, make_report, run_job, and print_report are
# unchanged from Listing 19-4.


def produce(jobs: own list[Job], queue: Queue[Job]):
    for job in own jobs:
        queue.put(job)
    queue.close()


def work(queue: Queue[Job], reports: Queue[Report]):
    for job in queue:
        reports.put(run_job(job))


class Options:
    path: str
    workers: int32


def parse_options(args: list[str]) -> Result[Options, str]:
    mut options = Options(path="jobs.txt", workers=3)
    mut index = 0
    while index < len(args):
        arg = args[index]
        if arg == "--workers":
            if index + 1 >= len(args):
                return Result.Err("--workers needs a number")
            options.workers = try parse_int32(args[index + 1])
            index += 2
        else:
            options.path = arg.clone()
            index += 1
    if options.workers < 1:
        return Result.Err("--workers must be at least 1")
    return Result.Ok(options)


def run_all(jobs: own list[Job], workers: int32) -> int32:
    total = len(jobs)
    queue = Queue[Job](capacity=workers)
    reports = Queue[Report]()
    mut failed = 0
    with group = TaskGroup():
        group.start_soon(produce, jobs, queue)
        for _ in range(workers):
            group.start_soon(work, queue, reports)
        for _ in range(total):
            match reports.get():
                case QueueReceive.Item(report):
                    print_report(report)
                    if not report.ok:
                        failed += 1
                case _:
                    break
    print(f"{total} jobs, {failed} failed")
    return 1 if failed > 0 else 0


def main() -> int32:
    match parse_options(sys.args()):
        case Result.Ok(options):
            match fs.read_to_string(options.path):
                case Result.Ok(text):
                    return run_all(parse_jobs(text), options.workers)
                case Result.Err(error):
                    print(f"cannot read {options.path}: {error}")
                    return 2
        case Result.Err(message):
            print(f"jobrunner: {message}")
            return 2
```

<span class="caption">Listing 19-5: The worker-pool runner, with a
`--workers` option</span>

The pool moved out of `main` into `run_all`, which takes the jobs as `own`
because it passes them on to the producer. `main` is now only about
configuration: parse the options, read the file, hand over. `parse_options`
returns a `Result` so that a bad flag produces a message instead of a trap,
and it uses `try` on `parse_int32`, whose error type is also `str`, so a
non-numeric value passes straight up to `main`. The queue's capacity now
follows the worker count, which keeps the producer about one job ahead of
each worker.

Running with one worker brings back the sequential timing, and confirms that
the pool and not something else was responsible for the speedup:

```console
$ time aura run src/main.au -- --workers 1
[ok  ] greet (6 ms) hello from the job runner
[ok  ] count (7 ms) 155
[ok  ] nap-1 (1005 ms) rested
[ok  ] nap-2 (1008 ms) rested
[ok  ] nap-3 (1004 ms) rested
[FAIL] broken (7 ms) ExitStatus.Exited(3) about to fail
6 jobs, 1 failed

real	0m3.158s
user	0m0.251s
sys	0m0.028s
```

And the option parser rejects the inputs it should:

```console
$ aura run src/main.au -- --workers 0
jobrunner: --workers must be at least 1
$ aura run src/main.au -- --workers many
jobrunner: invalid digit found in string
$ aura run src/main.au -- --workers
jobrunner: --workers needs a number
```

We now have a parallel job runner in a little over a hundred lines, and the
part that does the work, `run_job`, did not change at all. But it is still a
trusting program. A job that fails is reported and forgotten, even if trying
again a moment later would have worked. A job that hangs keeps its worker
forever, and a runner with a hung job never exits. In the next section we will
teach it to retry, to give up on slow jobs, and to stop the whole pool when
time runs out.
