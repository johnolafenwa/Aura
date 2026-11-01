# Supervision, restarts, and graceful shutdown

The pool from the last section runs jobs quickly, but it assumes every job
behaves. Real jobs do not. A deploy step fails because a server was briefly
unreachable, and would have passed a second later. A test hangs on a lock
and never exits. Somebody kicks off the runner at the end of the day and
needs it to stop after ten minutes no matter what. A runner that cannot
handle these cases is one you end up babysitting.

We will give `jobrunner` the three habits of a supervisor:
it retries failures according to a per-job policy, it kills jobs that run
past their timeout, and it shuts the whole pool down when an overall deadline
passes, without leaving any child processes behind. At the end we will look at
`process.supervisor()`, which handles restarts for long-running services.

## Per-job options

Each job needs to say how it wants to be treated, so the job file grows a
little. Between the name and the colon, a job may now list options as
`key=value` words: `retries=N` allows up to N restarts after a failure, and
`timeout=SECONDS` limits how long a single attempt may run.

*Filename: jobs.txt*

```text
# name [retries=N] [timeout=SECONDS]: command
greet: echo hello from the job runner
count: ls /etc | wc -l
flaky retries=3: sh flaky.sh
slow timeout=2: sleep 5; echo too late
broken retries=1: echo about to fail >&2; exit 3
```

<span class="caption">Listing 19-6: A job file with per-job restart and
timeout options</span>

To have something that fails and then recovers, we need a job with memory.
`flaky.sh` keeps a counter in a file and only succeeds on its third run:

*Filename: flaky.sh*

```sh
#!/bin/sh
# Fails on the first two runs and succeeds on the third.
# Delete .flaky-count to start over.
count=$(cat .flaky-count 2>/dev/null || echo 0)
count=$((count + 1))
echo "$count" > .flaky-count
if [ "$count" -lt 3 ]; then
    echo "not yet (run $count)" >&2
    exit 1
fi
echo "worked on run $count"
```

We will model the restart rule as an enum, because "never restart" and
"restart on failure, up to a limit" are different kinds of policy, not two
values of one number. `Job` gains a `restart` field and a `timeout_s` field
which the parser will fill in with thirty seconds when a job does not say otherwise:

```aura
enum Restart:
    Never
    OnFailure(limit: int32)


class Job:
    name: str
    command: str
    restart: Restart
    timeout_s: int64
```

Parsing gets stricter at the same time. So far, a line without a colon was
skipped with a message. Now that lines carry options, a typo like
`retries=lots` or `colour=red` would be silently misread if we skipped
lines, so a bad line becomes an error that stops the run. `parse_job` handles
one header, and `parse_jobs` now returns a `Result` and counts lines so the
error can say where the problem is:

```aura fragment
def parse_job(header: str, command: str) -> Result[Job, str]:
    words = header.split_whitespace()
    if len(words) == 0:
        return Result.Err("missing job name")
    mut job = Job(
        name=words[0].clone(),
        command=command.trim(),
        restart=Restart.Never,
        timeout_s=30,
    )
    for word in words[1:]:
        pair = word.split_once("=")
        if pair is None:
            return Result.Err(f"expected key=value, found `{word}`")
        key, value = pair
        if key == "retries":
            job.restart = Restart.OnFailure(try parse_int32(value))
        elif key == "timeout":
            job.timeout_s = try parse_int64(value)
        else:
            return Result.Err(f"unknown option `{key}`")
    return Result.Ok(job)


def parse_jobs(text: str) -> Result[list[Job], str]:
    mut jobs: list[Job] = []
    mut line_number = 0
    for raw in text.lines():
        line_number += 1
        line = raw.trim()
        if line == "" or line.starts_with("#"):
            continue
        pair = line.split_once(":")
        if pair is None:
            return Result.Err(f"line {line_number}: expected `name: command`")
        header, command = pair
        match own parse_job(header, command):
            case Result.Ok(job):
                jobs.append(job)
            case Result.Err(message):
                return Result.Err(f"line {line_number}: {message}")
    return Result.Ok(jobs)
```

<span class="caption">Listing 19-7: Parsing job options into a `Job`</span>

`parse_job` builds the job with defaults first, as a `mut` binding, and then
lets each option overwrite one field. Both number parsers return
`Result[..., str]`, the same error type as `parse_job`, so `try` can pass a
bad number straight up. Notice `match own parse_job(...)` in `parse_jobs`. The
`Ok` arm appends the job to the list, which moves it, and you can only move a
value out of a match that owns its scrutinee. We will run into the same rule
in `main` in a moment.

`main` has to handle the new `Result` from `parse_jobs`. The obvious change
is a nested match:

```aura fragment
                case Result.Ok(text):
                    match parse_jobs(text):
                        case Result.Ok(jobs):
                            return run_all(jobs, options.workers)
                        case Result.Err(message):
                            print(f"{options.path}: {message}")
                            return 2
```

```console
$ aura check src/main.au
error[AU3002]: cannot move `jobs` out of a shared match on `parse_jobs(...)`
 --> src/main.au:161:44
    |
161 |                             return run_all(jobs, options.workers)
    |                                            ^
  = related src/main.au:160:40: value is borrowed here
  = help: write `match own parse_jobs(...)` to consume the scrutinee, or call `.copy()` to consume an independent copy
  = fix: replace src/main.au:161:48-161:48 with `.copy()`
```

A plain `match` looks at its scrutinee through shared access, so `jobs` in the
`Ok` arm is a borrowed view of the list inside the `Result`. `run_all` wants
`own list[Job]`, so it needs the list itself. The help line offers two fixes.
Copying the list would work but would duplicate every job for no reason. The
`Result` is a temporary that nothing else will ever look at, so the right
answer is to let the match consume it with `match own parse_jobs(text):`.
In Listing 19-5 we wrote `run_all(parse_jobs(text), ...)` and never hit this,
because there the call's result went straight into an `own` parameter without
a match in between.

With `match own` in place, a malformed file now stops the runner before
anything starts:

```console
$ aura run src/main.au -- bad.txt
bad.txt: line 2: invalid digit found in string
$ aura run src/main.au -- bad2.txt
bad2.txt: line 2: unknown option `colour`
```

Here `bad.txt` has `bad retries=lots: echo x` on its second line, and
`bad2.txt` has `bad colour=red: echo x`. The options are parsed, but nothing
uses them yet:

```console
$ aura run src/main.au
[ok  ] greet (5 ms) hello from the job runner
[ok  ] count (8 ms) 155
[FAIL] flaky (11 ms) ExitStatus.Exited(1) not yet (run 1)
[FAIL] broken (7 ms) ExitStatus.Exited(3) about to fail
[ok  ] slow (5010 ms) too late
5 jobs, 2 failed
```

`flaky` failed on its first try and was not retried, and `slow` ran its full
five seconds despite asking for two. Let's fix both.

## Restarting failed jobs

A retry loop needs to run a single attempt many times, so let's split the
attempt out of `run_job`. `run_once` runs the command and returns
`Result[str, str]`: the output on success, a description of the failure
otherwise. That is all the retry logic needs to know.

```aura fragment
def run_once(job: Job) -> Result[str, str]:
    match process.run(["/bin/sh", "-c", job.command.clone()]):
        case Result.Ok(completed):
            if completed.success():
                return Result.Ok(completed.stdout().trim())
            errors = completed.stderr().trim()
            if errors == "":
                return Result.Err(f"{completed.status()}")
            return Result.Err(f"{completed.status()}: {errors}")
        case Result.Err(error):
            return Result.Err(f"{error}")


def allowed_restarts(policy: Restart) -> int32:
    match policy:
        case Restart.Never:
            return 0
        case Restart.OnFailure(limit):
            return limit
```

`allowed_restarts` turns the policy into a number. The `match` has to cover
both variants, so if we later add, say, an `Always` policy, the compiler will
send us back here to decide what it means.

`Report` gets an `attempts: int32` field so the summary can show how hard the
runner tried. Then `run_job` becomes a loop: run once, return on success,
return on failure if the policy allows no more restarts, and otherwise wait
and go again. Waiting between attempts is important. A job that failed
because a service was overloaded will probably fail again if we retry at
once, and retrying in a tight loop adds to the load. So we wait 100
milliseconds before the first restart and double the delay each time, which
is called exponential backoff. Here is the first attempt at the loop:

```aura fragment
def run_job(job: Job) -> Report:
    started = sys.monotonic_time_ms()
    limit = allowed_restarts(job.restart)
    mut attempts: int32 = 0
    mut backoff = 100ms
    while true:
        attempts += 1
        outcome = run_once(job)
        elapsed = sys.monotonic_time_ms() - started
        match own outcome:
            case Result.Ok(output):
                return Report(
                    name=job.name.clone(),
                    ok=true,
                    detail=output,
                    attempts=attempts,
                    elapsed_ms=elapsed,
                )
            case Result.Err(message):
                if attempts > limit:
                    return Report(
                        name=job.name.clone(),
                        ok=false,
                        detail=message,
                        attempts=attempts,
                        elapsed_ms=elapsed,
                    )
                print(
                    f"  {job.name}: attempt {attempts} failed ({message}), retrying in {backoff}"
                )
        sleep(backoff)
        backoff = backoff * 2
```

```console
$ aura check src/main.au
error[AU2999]: function `run_job` is missing a return
 --> src/main.au:91:1
   |
91 | def run_job(job: Job) -> Report:
   | ^
```

We can see that the loop never ends except through one of the two `return`
statements, but the compiler does not treat `while true` as special. It sees
a loop whose condition could, in principle, become false, and a path after
the loop that falls off the end of a function that promised a `Report`. We
could argue with that, but the error points at a real weakness in the code:
the "give up" decision is buried inside a match arm, two levels down. It reads
better as the loop's own exit. Let's remember the last error, `break` when we
are out of attempts, and build the failure report once, after the loop:

```aura fragment
def run_job(job: Job) -> Report:
    started = sys.monotonic_time_ms()
    limit = allowed_restarts(job.restart)
    mut attempts: int32 = 0
    mut backoff = 100ms
    mut last_error = ""
    while true:
        attempts += 1
        match own run_once(job):
            case Result.Ok(output):
                elapsed = sys.monotonic_time_ms() - started
                return Report(
                    name=job.name.clone(),
                    ok=true,
                    detail=output,
                    attempts=attempts,
                    elapsed_ms=elapsed,
                )
            case Result.Err(message):
                last_error = message
        if attempts > limit:
            break
        print(
            f"  {job.name}: attempt {attempts} failed ({last_error}), retrying in {backoff}"
        )
        sleep(backoff)
        backoff = backoff * 2
    elapsed = sys.monotonic_time_ms() - started
    return Report(
        name=job.name.clone(),
        ok=false,
        detail=last_error,
        attempts=attempts,
        elapsed_ms=elapsed,
    )
```

<span class="caption">Listing 19-8: Retrying a job with exponential
backoff</span>

`backoff` starts as the `Duration` literal `100ms`, and `backoff * 2` is
`Duration` arithmetic, so the type follows the value through the loop and
prints itself as `100ms` or `200ms` in the retry message. Note that `sleep`
here suspends only the worker task that called it. The other workers carry on
running jobs while this one waits. We also changed `print_report` to show the
attempt count when it is more than one:

```aura fragment
def print_report(report: Report):
    mark = "ok  " if report.ok else "FAIL"
    tries = "" if report.attempts == 1 else f", {report.attempts} attempts"
    print(f"[{mark}] {report.name} ({report.elapsed_ms} ms{tries}) {report.detail}")
```

Let's reset the counter and run it:

```console
$ rm -f .flaky-count
$ aura run src/main.au
[ok  ] greet (6 ms) hello from the job runner
  flaky: attempt 1 failed (ExitStatus.Exited(1): not yet (run 1)), retrying in 100ms
[ok  ] count (9 ms) 155
  broken: attempt 1 failed (ExitStatus.Exited(3): about to fail), retrying in 100ms
  flaky: attempt 2 failed (ExitStatus.Exited(1): not yet (run 2)), retrying in 200ms
[FAIL] broken (115 ms, 2 attempts) ExitStatus.Exited(3): about to fail
[ok  ] flaky (325 ms, 3 attempts) worked on run 3
[ok  ] slow (5004 ms) too late
5 jobs, 1 failed
```

`flaky` failed twice, waited 100 and then 200 milliseconds, and passed on its
third run, within its budget of three restarts. `broken` used its single
restart and failed for good, and its report says so. The restart policy is a
decision about each job, which is why it lives in the job file and not in a
flag for the whole run: retrying a flaky network call makes sense, while
retrying a job with a syntax error only delays the failure.

## Timeouts, and the process group

`slow` still ran for five seconds. `process.run` accepts a `timeout` as a
`Duration`. When it expires, the runtime terminates the child, waits for it
to exit, and returns `Err` with a timed-out error. Let's pass the job's
timeout in `run_once`:

```aura fragment
def run_once(job: Job) -> Result[str, str]:
    command = ["/bin/sh", "-c", job.command.clone()]
    timeout = Duration.seconds(job.timeout_s)
    match process.run(command, timeout=timeout):
        case Result.Ok(completed):
            if completed.success():
                return Result.Ok(completed.stdout().trim())
            errors = completed.stderr().trim()
            if errors == "":
                return Result.Err(f"{completed.status()}")
            return Result.Err(f"{completed.status()}: {errors}")
        case Result.Err(error):
            return Result.Err(f"{error}")
```

`Duration.seconds` builds a `Duration` from a number we only know at run time.
The literals like `2s` are only for values written in the source. Let's see
what this buys us:

```console
$ rm -f .flaky-count
$ time aura run src/main.au
[ok  ] greet (10 ms) hello from the job runner
  flaky: attempt 1 failed (ExitStatus.Exited(1): not yet (run 1)), retrying in 100ms
[ok  ] count (10 ms) 155
  broken: attempt 1 failed (ExitStatus.Exited(3): about to fail), retrying in 100ms
  flaky: attempt 2 failed (ExitStatus.Exited(1): not yet (run 2)), retrying in 200ms
[FAIL] broken (115 ms, 2 attempts) ExitStatus.Exited(3): about to fail
[ok  ] flaky (332 ms, 3 attempts) worked on run 3
[FAIL] slow (5005 ms) process.Error.TimedOut
5 jobs, 2 failed

real	0m5.143s
user	0m0.235s
sys	0m0.036s
```

That is a strange result. `slow` is now reported as timed out, which means
the timeout did fire, yet the job still took five seconds.

The reason lies in what our job really is. `process.run` started `/bin/sh`, and
the shell in turn started `sleep 5` as its own child. At two seconds the
runtime terminated the process it knew about, the shell. But `sleep` was a
separate process, and it kept running, holding on to the output pipe it had
inherited from the shell. The runtime was waiting to finish reading that pipe,
and the pipe only closed when `sleep` exited, three seconds later. Killing a
process does not kill its children. Any program that runs shell commands, in
any language, has to deal with this.

The fix is the `group=true` option. It starts the child as the leader of a new
process group, and when the runtime terminates the child, it signals the whole
group: the shell, `sleep`, and anything else they started. On Unix hosts that
is a guarantee Aura maintains; on hosts without process groups, asking for one
returns a typed error rather than quietly doing less. A job runner should
always ask for it:

```aura fragment
    match process.run(command, timeout=timeout, group=true):
```

```console
$ rm -f .flaky-count
$ time aura run src/main.au
[ok  ] greet (7 ms) hello from the job runner
[ok  ] count (13 ms) 155
  flaky: attempt 1 failed (ExitStatus.Exited(1): not yet (run 1)), retrying in 100ms
  broken: attempt 1 failed (ExitStatus.Exited(3): about to fail), retrying in 100ms
  flaky: attempt 2 failed (ExitStatus.Exited(1): not yet (run 2)), retrying in 200ms
[FAIL] broken (118 ms, 2 attempts) ExitStatus.Exited(3): about to fail
[ok  ] flaky (333 ms, 3 attempts) worked on run 3
[FAIL] slow (2215 ms) process.Error.TimedOut
5 jobs, 2 failed

real	0m2.355s
user	0m0.215s
sys	0m0.057s
```

Now `slow` is stopped after two seconds, plus the short time it takes to
terminate the group and collect it, and the run finishes in under two and a
half. A timed-out attempt is a failure like any other, so a job with both
`retries` and `timeout` will be restarted after a timeout too.

## A deadline for the whole run

Per-job timeouts bound each attempt, but not the run. A file of a hundred
jobs that each take twenty-nine seconds stays inside every timeout and still
takes a long time, and retries multiply that. The person running the tool
needs one more control: "whatever happens, stop after this many seconds."

The parent is the right place for this decision, because it is the only part
of the program that sees the whole run. It already waits on the reports
queue; now it will wait with a timeout, computed from a deadline. If the
deadline passes before every report has arrived, the parent cancels the task
group. Here is the new `run_all`, which takes the whole `Options` because it
needs both the worker count and the new `deadline_s` field:

```aura fragment
def run_all(jobs: own list[Job], options: Options) -> int32:
    total = len(jobs)
    queue = Queue[Job](capacity=options.workers)
    reports = Queue[Report]()
    deadline = sys.monotonic_time_ms() + options.deadline_s * 1000
    mut finished = 0
    mut failed = 0
    with group = TaskGroup():
        group.start_soon(produce, jobs, queue)
        for _ in range(options.workers):
            group.start_soon(work, queue, reports)
        while finished < total:
            remaining = deadline - sys.monotonic_time_ms()
            match reports.get(timeout=Duration.ms(remaining)):
                case QueueReceive.Item(report):
                    print_report(report)
                    finished += 1
                    if not report.ok:
                        failed += 1
                case QueueReceive.TimedOut:
                    print(
                        f"deadline reached, cancelling {total - finished} unfinished jobs"
                    )
                    group.cancel()
                    break
                case _:
                    break
    unfinished = total - finished
    print(f"{total} jobs, {failed} failed, {unfinished} cancelled")
    return 1 if failed + unfinished > 0 else 0
```

`group.cancel()` does not kill anything by itself. It sets a flag on every
task in the group, and each task notices at its next scheduler-aware wait: a
queue receive or send, a `sleep`, a task-result wait, or a process wait. That
last one is what makes this work for us. A worker blocked in `process.run`
when the group is cancelled gets an error back from `process.run`, and the
runtime terminates the child, and with `group=true` the child's whole process
group, before it returns. A worker sleeping between retries wakes up. A worker
waiting for its next job sees its `for job in queue:` loop end.

There is one more place to look. After a cancelled attempt fails, the retry
loop in `run_job` would happily decide to try again. The attempt would fail at
once, since the task is cancelled, but there is no reason to go through the
motions. So the loop checks `cancelled()`, which reports whether the current
task has been asked to stop:

```aura fragment
        if attempts > limit or cancelled():
            break
```

Cancellation in Aura is cooperative on purpose. A task is never interrupted
in the middle of arbitrary code, so it can never be stopped halfway through
updating something. It stops at the points where it was waiting anyway, and
long CPU-bound loops that need to stop promptly check `cancelled()` for
themselves.

`parse_options` learns a `--deadline` flag, with a default of 60 seconds.
Here is a job file designed to blow through a short deadline: one job that
finishes at once, one that would sleep for thirty seconds, and one that keeps
failing and retrying:

*Filename: long.txt*

```text
quick: echo done quickly
forever: sleep 30; echo never printed
patient retries=5: sleep 1; exit 1
```

Before running it for real, let's try the most extreme setting, a deadline of
zero:

```console
$ aura run src/main.au -- long.txt --deadline 0
error[AU4001]: get(timeout=...) must be non-negative
 --> src/main.au
  = note: Aura call chain (innermost first): run_all at 175:1 -> main at 207:1
```

This is a trap, not a compile error: the program stopped at run time, and the
note shows the chain of calls that led there. By the time the parent computed
`remaining`, the deadline had already passed, so `remaining` was a few
milliseconds below zero. Aura refuses a negative timeout rather than guessing
what it means. Some APIs treat negative values as "wait forever", which is
exactly the wrong behavior for a deadline that has already passed. The same
thing would happen to a longer run whenever the deadline expired while the
parent was busy printing a report, so this is a real bug that the zero
deadline merely made easy to hit. The fix is to clamp the value:

```aura fragment
            remaining = max(deadline - sys.monotonic_time_ms(), 0)
```

A zero timeout is valid and means "do not wait at all", which is exactly
right:

```console
$ aura run src/main.au -- long.txt --deadline 0
deadline reached, cancelling 3 unfinished jobs
3 jobs, 0 failed, 3 cancelled
```

Now with a three-second deadline:

```console
$ time aura run src/main.au -- long.txt --deadline 3
[ok  ] quick (11 ms) done quickly
  patient: attempt 1 failed (ExitStatus.Exited(1)), retrying in 100ms
  patient: attempt 2 failed (ExitStatus.Exited(1)), retrying in 200ms
deadline reached, cancelling 2 unfinished jobs
3 jobs, 0 failed, 2 cancelled

real	0m3.340s
user	0m0.355s
sys	0m0.054s
$ echo $?
1
$ ps -eo args | grep -c '^sleep 30$'
0
```

`quick` finished. At three seconds, `forever` was still sleeping and `patient`
was in the middle of its third attempt. The parent cancelled the group, both
workers stopped, and the `with` block waited for them before the summary
printed. The last command checks the process table for the `sleep 30` that
`forever` started, and finds none: it was terminated along with its shell. The
exit status is 1, because cancelled jobs count as not done.

This is graceful shutdown in the structured-concurrency sense. Every child
process belongs to a `process.run` call, every call belongs to a worker task,
and every task belongs to the task group in `run_all`. Cancelling the group
reaches all of them, and leaving the `with` block does not happen until all of
them have finished. There is no list of process IDs to walk at exit and no
cleanup handler to forget. If `run_all` returned early or a trap unwound
through it, the same `with` block would still wait for the tasks.

## The complete runner

Here is the whole program, with every change from this section in place:

*Filename: src/main.au*

```aura
import fs
import process
import sys


enum Restart:
    Never
    OnFailure(limit: int32)


class Job:
    name: str
    command: str
    restart: Restart
    timeout_s: int64


class Report:
    name: str
    ok: bool
    detail: str
    attempts: int32
    elapsed_ms: int64


def parse_job(header: str, command: str) -> Result[Job, str]:
    words = header.split_whitespace()
    if len(words) == 0:
        return Result.Err("missing job name")
    mut job = Job(
        name=words[0].clone(),
        command=command.trim(),
        restart=Restart.Never,
        timeout_s=30,
    )
    for word in words[1:]:
        pair = word.split_once("=")
        if pair is None:
            return Result.Err(f"expected key=value, found `{word}`")
        key, value = pair
        if key == "retries":
            job.restart = Restart.OnFailure(try parse_int32(value))
        elif key == "timeout":
            job.timeout_s = try parse_int64(value)
        else:
            return Result.Err(f"unknown option `{key}`")
    return Result.Ok(job)


def parse_jobs(text: str) -> Result[list[Job], str]:
    mut jobs: list[Job] = []
    mut line_number = 0
    for raw in text.lines():
        line_number += 1
        line = raw.trim()
        if line == "" or line.starts_with("#"):
            continue
        pair = line.split_once(":")
        if pair is None:
            return Result.Err(f"line {line_number}: expected `name: command`")
        header, command = pair
        match own parse_job(header, command):
            case Result.Ok(job):
                jobs.append(job)
            case Result.Err(message):
                return Result.Err(f"line {line_number}: {message}")
    return Result.Ok(jobs)


def run_once(job: Job) -> Result[str, str]:
    command = ["/bin/sh", "-c", job.command.clone()]
    timeout = Duration.seconds(job.timeout_s)
    match process.run(command, timeout=timeout, group=true):
        case Result.Ok(completed):
            if completed.success():
                return Result.Ok(completed.stdout().trim())
            errors = completed.stderr().trim()
            if errors == "":
                return Result.Err(f"{completed.status()}")
            return Result.Err(f"{completed.status()}: {errors}")
        case Result.Err(error):
            return Result.Err(f"{error}")


def allowed_restarts(policy: Restart) -> int32:
    match policy:
        case Restart.Never:
            return 0
        case Restart.OnFailure(limit):
            return limit


def run_job(job: Job) -> Report:
    started = sys.monotonic_time_ms()
    limit = allowed_restarts(job.restart)
    mut attempts: int32 = 0
    mut backoff = 100ms
    mut last_error = ""
    while true:
        attempts += 1
        match own run_once(job):
            case Result.Ok(output):
                elapsed = sys.monotonic_time_ms() - started
                return Report(
                    name=job.name.clone(),
                    ok=true,
                    detail=output,
                    attempts=attempts,
                    elapsed_ms=elapsed,
                )
            case Result.Err(message):
                last_error = message
        if attempts > limit or cancelled():
            break
        print(
            f"  {job.name}: attempt {attempts} failed ({last_error}), retrying in {backoff}"
        )
        sleep(backoff)
        backoff = backoff * 2
    elapsed = sys.monotonic_time_ms() - started
    return Report(
        name=job.name.clone(),
        ok=false,
        detail=last_error,
        attempts=attempts,
        elapsed_ms=elapsed,
    )


def print_report(report: Report):
    mark = "ok  " if report.ok else "FAIL"
    tries = "" if report.attempts == 1 else f", {report.attempts} attempts"
    print(f"[{mark}] {report.name} ({report.elapsed_ms} ms{tries}) {report.detail}")


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
    deadline_s: int64


def parse_options(args: list[str]) -> Result[Options, str]:
    mut options = Options(path="jobs.txt", workers=3, deadline_s=60)
    mut index = 0
    while index < len(args):
        arg = args[index]
        if arg == "--workers" or arg == "--deadline":
            if index + 1 >= len(args):
                return Result.Err(f"{arg} needs a number")
            value = args[index + 1]
            if arg == "--workers":
                options.workers = try parse_int32(value)
            else:
                options.deadline_s = try parse_int64(value)
            index += 2
        else:
            options.path = arg.clone()
            index += 1
    if options.workers < 1:
        return Result.Err("--workers must be at least 1")
    return Result.Ok(options)


def run_all(jobs: own list[Job], options: Options) -> int32:
    total = len(jobs)
    queue = Queue[Job](capacity=options.workers)
    reports = Queue[Report]()
    deadline = sys.monotonic_time_ms() + options.deadline_s * 1000
    mut finished = 0
    mut failed = 0
    with group = TaskGroup():
        group.start_soon(produce, jobs, queue)
        for _ in range(options.workers):
            group.start_soon(work, queue, reports)
        while finished < total:
            remaining = max(deadline - sys.monotonic_time_ms(), 0)
            match reports.get(timeout=Duration.ms(remaining)):
                case QueueReceive.Item(report):
                    print_report(report)
                    finished += 1
                    if not report.ok:
                        failed += 1
                case QueueReceive.TimedOut:
                    print(
                        f"deadline reached, cancelling {total - finished} unfinished jobs"
                    )
                    group.cancel()
                    break
                case _:
                    break
    unfinished = total - finished
    print(f"{total} jobs, {failed} failed, {unfinished} cancelled")
    return 1 if failed + unfinished > 0 else 0


def main() -> int32:
    match parse_options(sys.args()):
        case Result.Ok(options):
            match fs.read_to_string(options.path):
                case Result.Ok(text):
                    match own parse_jobs(text):
                        case Result.Ok(jobs):
                            return run_all(jobs, options)
                        case Result.Err(message):
                            print(f"{options.path}: {message}")
                            return 2
                case Result.Err(error):
                    print(f"cannot read {options.path}: {error}")
                    return 2
        case Result.Err(message):
            print(f"jobrunner: {message}")
            return 2
```

<span class="caption">Listing 19-9: The supervised job runner</span>

The runner is also a perfectly ordinary candidate for `aura build`, which
turns it into a standalone native binary that takes its arguments directly:

```console
$ aura build -o target/jobrunner src/main.au
aura: built `target/jobrunner` with direct backend
$ rm -f .flaky-count
$ ./target/jobrunner --workers 2
[ok  ] count (7 ms) 155
[ok  ] greet (7 ms) hello from the job runner
  flaky: attempt 1 failed (ExitStatus.Exited(1): not yet (run 1)), retrying in 100ms
  flaky: attempt 2 failed (ExitStatus.Exited(1): not yet (run 2)), retrying in 200ms
[ok  ] flaky (323 ms, 3 attempts) worked on run 3
  broken: attempt 1 failed (ExitStatus.Exited(3): about to fail), retrying in 100ms
[FAIL] broken (115 ms, 2 attempts) ExitStatus.Exited(3): about to fail
[FAIL] slow (2210 ms) process.Error.TimedOut
5 jobs, 2 failed, 0 cancelled
```

## Services that should stay up

Everything so far treats a job as a batch task: it runs, it finishes, and
success means exit code 0. Some programs are not like that. A web server, a
queue consumer, or a file watcher is supposed to run until you stop it, and
if it exits, that is the failure. For those, writing our own retry loop
around `process.run` would be the wrong shape, since `process.run` waits for
the child to finish. The `process` module has a separate tool for this case:
`process.supervisor()`.

A supervisor owns a set of named, long-running children. You give each child
a restart policy when you start it, and the supervisor restarts it on its own
schedule, reporting each exit and restart as an event. Here is a small
program that supervises two services for four seconds: a `ticker` that runs
forever, and a `crasher` that exits with a failure every half second:

*Filename: src/services.au*

```aura
import process
import sys


def supervise(seconds: int64) -> Result[None, process.Error]:
    with supervisor = process.supervisor():
        try supervisor.start(
            name="ticker",
            command=["/bin/sh", "-c", "while true; do sleep 1; done"],
        )
        try supervisor.start(
            name="crasher",
            command=["/bin/sh", "-c", "sleep 0.5; exit 1"],
            restart=process.RestartPolicy.OnFailure,
            backoff=200ms,
            max_restarts=3,
        )
        deadline = sys.monotonic_time_ms() + seconds * 1000
        while sys.monotonic_time_ms() < deadline:
            match own supervisor.wait_or_none(timeout=500ms):
                case Result.Ok(process.SupervisorEvent as event):
                    print(event)
                case Result.Ok(None):
                    pass
                case Result.Err(error):
                    return Result.Err(error)
        print("time is up, stopping services")
    print("all services stopped")
    return Result.Ok(None)


def main() -> int32:
    match supervise(4):
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
```

<span class="caption">Listing 19-10: Supervising two long-running
services</span>

```console
$ aura run src/services.au
SupervisorEvent.Restarted(crasher, ExitStatus.Exited(1), 1)
SupervisorEvent.Restarted(crasher, ExitStatus.Exited(1), 2)
SupervisorEvent.Restarted(crasher, ExitStatus.Exited(1), 3)
SupervisorEvent.Exited(crasher, ExitStatus.Exited(1), 3)
time is up, stopping services
all services stopped
$ ps -eo args | grep -c '^/bin/sh -c while true'
0
```

The supervisor restarted `crasher` three times, waiting 200 milliseconds
before each restart, and when the fourth run failed it reported `Exited`
instead of `Restarted`: its `max_restarts` was used up, so it stayed down.
`ticker` produced no events at all, which is what a healthy service looks
like. Each event carries the child's name, its exit status, and the restart
count so far, which is everything a log line or an alert needs.

The supervisor is a resource, and we bound it with `with`, the same way
Chapter 14 handled files. When `supervise` leaves the block, the supervisor
closes and stops every child it still manages, so `ticker` is terminated even
though nothing in our code mentions it. The `ps` check confirms that it is
gone. The same is true if `supervise` returns early through one of the `try`
expressions, or if a trap unwinds through it. `supervisor.start` defaults to
`group=true`, so like our runner's `process.run` calls, stopping a service
reaches the processes it started too.

Supervised children default to `RestartPolicy.OnFailure` with a 100
millisecond backoff and no limit on restarts, which is why `ticker` needed no
options. `RestartPolicy.Never` and `RestartPolicy.Always` cover the other
cases: `Always` restarts even after a clean exit, which suits a service that
should never stop at all. Choose the policy per child, for the same reason we
put `retries` on each job: a service that crashes because its configuration
is wrong will crash the same way every time, and an unlimited restart policy
only turns one clear failure into an endless stream of them.

We used `wait_or_none` here, which returns `Result.Ok(None)` when the timeout
passes with no event, so that "nothing happened" and "something happened"
share one `match`. `match own` consumes the returned `Result`, and the nested
type pattern `process.SupervisorEvent as event` binds the event when there is
one. The [process manual page](/manual/process) has the full supervisor API,
including `stop()`, `is_empty()`, and the `wait` variant that returns a
`process.SupervisorWait` enum.

Batch jobs and services can live in the same program. A natural next step for
`jobrunner` would be a `service` option in the job file that hands those jobs
to a supervisor instead of the worker pool, with the parent waiting on report
queues and supervisor events side by side. Everything you need to build that
is already in this chapter.

## Summary

You have built a complete supervised job runner from `aura new`. It parses a
small configuration language into typed values and rejects bad lines with
line numbers. It runs shell commands safely, by naming the shell explicitly
rather than trusting a string. It runs jobs concurrently through a bounded
queue and a worker pool, retries each job according to its own policy with
exponential backoff, enforces timeouts with process groups so no grandchild
survives, and shuts the whole pool down at a deadline through task-group
cancellation, leaving nothing running.

Along the way, the compiler stopped us several times, and each time it was
pointing at a real decision: a `split_once` result that might be `None`, a
string we tried to take out of a borrowed job, a list we gave to a task and
then tried to read, a `Result` we matched by sharing and then tried to
consume. The runtime stopped us once, over a negative timeout. None of these
was a matter of style. Each one would have been a crash, a data race, or a
silently wrong answer in a language that let it through.

That is the end of the main text. The appendices that follow are reference
material to keep nearby as you write your own programs: the keywords and
operators, a guide for Python programmers, the tools around the compiler, and
the diagnostics you are most likely to meet.
