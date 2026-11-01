# A single-task runner

Every concurrent program starts life as a sequential one. Before we think
about workers and queues, we want a runner that does one job at a time and
does it correctly: it reads the job file, runs each command, captures what the
command printed, and tells us plainly which jobs failed. Once that part is
solid, making it parallel is a change to the shape of the program, not to
what each job does.

## Creating the project

Let's start from an empty directory, the same way we did in Chapter 2:

```console
$ aura new jobrunner
created `jobrunner`
$ cd jobrunner
$ aura run src/main.au
Hello from Aura
```

`aura new` gives us an `Aura.toml` manifest, a `src/main.au` with a `main`
function, a `tests/smoke.au` placeholder, and a `.gitignore` that ignores the
`target/` directory where native builds land. The generated `main` returns
`int32`, and that matters for a tool like ours: whatever `main` returns
becomes the process exit status, so scripts that call `jobrunner` can tell
success from failure without parsing its output.

## The job file

A job is a name and a command. We will keep the file format as plain as
possible: one job per line, the name, a colon, and the command. Blank lines
and lines starting with `#` are skipped, so the file can carry comments.
Create `jobs.txt` in the project root:

*Filename: jobs.txt*

```text
# Each line is `name: command`. Blank lines and # comments are skipped.
greet: echo hello from the job runner
count: ls /etc | wc -l
nap: sleep 1; echo rested
broken: echo about to fail >&2; exit 3
```

<span class="caption">Listing 19-1: A first job file with four jobs, one of
which fails on purpose</span>

The four jobs are chosen to exercise different paths. `greet` prints a line.
`count` uses a pipe, which only works if a shell runs the command. `nap` takes
a full second, which will be useful when we start measuring. `broken` writes
to standard error and exits with status 3, so we can see what a failure looks
like.

## Parsing the jobs

Our first goal is a program that reads the file and prints the jobs it found.
We need a `Job` class with two fields, and a function that turns the file's
text into a `list[Job]`. For each line, we want to split at the first colon.
Strings in Aura have a `split_once` method that does exactly that, and it
returns a pair. A tuple pattern seems like the natural way to take the pair
apart:

*Filename: src/main.au*

```aura check-fail:AU2999
import fs


class Job:
    name: str
    command: str


def parse_jobs(text: str) -> list[Job]:
    mut jobs: list[Job] = []
    for raw in text.lines():
        line = raw.trim()
        if line == "" or line.starts_with("#"):
            continue
        match line.split_once(":"):
            case (name, command):
                jobs.append(Job(name=name.trim(), command=command.trim()))
    return jobs


def main() -> int32:
    match fs.read_to_string("jobs.txt"):
        case Result.Ok(text):
            for job in parse_jobs(text):
                print(f"{job.name} -> {job.command}")
            return 0
        case Result.Err(error):
            print(f"cannot read jobs.txt: {error}")
            return 2
```

<span class="caption">Listing 19-2: A first attempt at parsing, which does
not compile</span>

The compiler stops us at the pattern:

```console
$ aura check src/main.au
error[AU2999]: tuple pattern requires a tuple scrutinee, found `(str, str) | None`
 --> src/main.au:16:18
   |
16 |             case (name, command):
   |                  ^
```

The message tells us the exact type we are matching on, and that type is the
lesson. `split_once` does not return a pair. It returns `(str, str) | None`,
because a line with no colon in it has nothing to split. A Python programmer
would write `name, command = line.split(":", 1)` and find out about the
missing colon when the program raised `ValueError` on someone else's input.
Aura puts the missing case into the return type, so we cannot forget it, and
a tuple pattern alone does not cover the `None` half.

We could add a second `case` for `None`, but there is a more direct way to say
what we mean. An `is None` test narrows a union: after we have handled the
`None` case and moved on with `continue`, the compiler knows that `pair` holds
a tuple for the rest of the loop body, and ordinary tuple unpacking works.
Replace `parse_jobs` with this version:

*Filename: src/main.au*

```aura
import fs


class Job:
    name: str
    command: str


def parse_jobs(text: str) -> list[Job]:
    mut jobs: list[Job] = []
    for raw in text.lines():
        line = raw.trim()
        if line == "" or line.starts_with("#"):
            continue
        pair = line.split_once(":")
        if pair is None:
            print(f"skipping a line with no name: {line}")
            continue
        name, command = pair
        jobs.append(Job(name=name.trim(), command=command.trim()))
    return jobs


def main() -> int32:
    match fs.read_to_string("jobs.txt"):
        case Result.Ok(text):
            for job in parse_jobs(text):
                print(f"{job.name} -> {job.command}")
            return 0
        case Result.Err(error):
            print(f"cannot read jobs.txt: {error}")
            return 2
```

<span class="caption">Listing 19-3: Parsing the job file with `split_once`
and an `is None` test</span>

Now it compiles and runs:

```console
$ aura run src/main.au
greet -> echo hello from the job runner
count -> ls /etc | wc -l
nap -> sleep 1; echo rested
broken -> echo about to fail >&2; exit 3
```

Notice that the comment line in `jobs.txt` contains a colon too, inside the
backticks. It is skipped because the `#` check comes first. Order matters in a
parser like this, and putting the cheap "ignore this line" tests at the top
keeps the rest of the loop simple.

`fs.read_to_string` returns a `Result[str, io.Error]`, and `main` matches on
it. If we run the program from a directory that has no `jobs.txt`, the `Err`
arm runs, and `main` returns 2:

```console
$ cd /tmp
$ aura run ~/jobrunner/src/main.au
cannot read jobs.txt: io.Error.NotFound
```

The path is relative to the directory we run from, not to the source file.
We will make the path a command-line argument at the end of this section.

## Running a command

Now for the part that makes this a job runner. The `process` module's `run`
function starts a child process, waits for it to finish, and captures its
standard output and standard error. It returns
`Result[process.Completed, process.Error]`: `Err` if the child could not be
started at all, `Ok` with a `Completed` record if it ran, whether or not it
succeeded.

There is one thing about `process.run` that surprises people coming from
Python's `subprocess` or from shell scripting: it takes a list of arguments,
and it never starts a shell. Let's see what that means for our jobs by
handing it the whole command string as the program name:

```aura fragment
def run_job(job: Job):
    match process.run([job.command.clone()]):
        case Result.Ok(completed):
            print(f"{job.name}: {completed.status()}")
            print(completed.stdout())
        case Result.Err(error):
            print(f"{job.name}: could not start: {error}")
```

```console
$ aura run src/main.au
greet: could not start: process.Error.Spawn(No such file or directory (os error 2))
count: could not start: process.Error.Spawn(No such file or directory (os error 2))
nap: could not start: process.Error.Spawn(No such file or directory (os error 2))
broken: could not start: process.Error.Spawn(No such file or directory (os error 2))
```

The operating system went looking for a program literally named
`echo hello from the job runner`, spaces and all, and did not find one. Aura
does no splitting, no quoting, and no expansion of `|`, `;`, or `>&2`. That
is a deliberate choice. A runtime that quietly passes strings through a shell
makes it far too easy to build a command out of user input and end up with
shell injection. In Aura, a command is always an explicit argument vector,
and if you want a shell you have to ask for one by name.

We do want a shell here, because our job file is written in shell syntax:
`count` has a pipe and `broken` has a redirection. So we ask for one: the
program is `/bin/sh`, its first argument is `-c`, and its second argument is
the command text. Let's write that, and change `main` to call `run_job` for
each job:

```aura fragment
def run_job(job: Job):
    match process.run(["/bin/sh", "-c", job.command]):
        case Result.Ok(completed):
            print(f"{job.name}: {completed.status()}")
            print(completed.stdout())
        case Result.Err(error):
            print(f"{job.name}: could not start: {error}")
```

This time the checker objects to `job.command`:

```console
$ aura check src/main.au
error[AU3002]: cannot move non-copy field `command` out of borrowed value `job`
 --> src/main.au:26:45
   |
26 |     match process.run(["/bin/sh", "-c", job.command]):
   |                                             ^
  = related src/main.au:25:13: `job` is borrowed here
  = help: take `job` as `own Job` when the field should be moved, or call `.clone()` on the field to return an independent value
  = fix: replace src/main.au:26:52-26:52 with `.clone()`
```

This is the ownership rule from Chapter 4 showing up in a new place. A list
literal owns its elements, so writing `job.command` inside `[...]` tries to
move the string out of `job` and into the new list. But `run_job` declared
`job: Job`, a bare parameter, which means it only has shared access: the
caller still owns the job, and we are not allowed to take a piece of it. The
`related` line points at the parameter to make that connection for us.

The help line offers two ways out, and it is worth pausing on which one is
right. Taking `own Job` would let `run_job` consume the job, but then `main`
could not loop over its list without giving the jobs away, and later we will
want the name for the report. The argument list, on the other hand, is a
short-lived value that `process.run` reads and then drops. A copy of one
string is exactly what it needs. So we follow the `fix` line and clone:

```aura fragment
def run_job(job: Job):
    match process.run(["/bin/sh", "-c", job.command.clone()]):
        case Result.Ok(completed):
            print(f"{job.name}: {completed.status()}")
            print(completed.stdout())
        case Result.Err(error):
            print(f"{job.name}: could not start: {error}")
```

```console
$ aura run src/main.au
greet: ExitStatus.Exited(0)
hello from the job runner

count: ExitStatus.Exited(0)
155

nap: ExitStatus.Exited(0)
rested

broken: ExitStatus.Exited(3)

```

All four jobs ran. (Your `count` will differ; it is the number of entries in
your `/etc`.) Each `stdout()` ends with the newline the command printed,
which is why there is a blank line after each one. `broken` printed its
message to standard error, so it does not appear here, but its exit status
does: `ExitStatus.Exited(3)`. Notice that a failing command is still
`Result.Ok`. As far as `process.run` is concerned, the child started and
finished, and that is a success. Whether exit status 3 counts as a failure is
a decision for our program, and that is the next thing to build.

## Reporting results

Printing as we go was fine for exploring, but a real runner needs to hand its
results to someone: to a summary at the end, and in the next section, from a
worker back to the parent. That means a value. Let's add a `Report` class
that records the job's name, whether it succeeded, a one-line detail, and how
long it took. `sys.monotonic_time_ms()` gives us a clock that only moves
forward, which is the right kind for measuring durations: the wall clock can
jump when the system time is corrected.

Here is the whole program with reporting, an exit status, and the path to the
job file taken from the command line:

*Filename: src/main.au*

```aura
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


def parse_jobs(text: str) -> list[Job]:
    mut jobs: list[Job] = []
    for raw in text.lines():
        line = raw.trim()
        if line == "" or line.starts_with("#"):
            continue
        pair = line.split_once(":")
        if pair is None:
            print(f"skipping a line with no name: {line}")
            continue
        name, command = pair
        jobs.append(Job(name=name.trim(), command=command.trim()))
    return jobs


def make_report(job: Job, ok: bool, detail: own str, elapsed_ms: int64) -> Report:
    return Report(name=job.name.clone(), ok=ok, detail=detail, elapsed_ms=elapsed_ms)


def run_job(job: Job) -> Report:
    started = sys.monotonic_time_ms()
    outcome = process.run(["/bin/sh", "-c", job.command.clone()])
    elapsed = sys.monotonic_time_ms() - started
    match outcome:
        case Result.Ok(completed):
            if completed.success():
                return make_report(job, true, completed.stdout().trim(), elapsed)
            detail = f"{completed.status()} {completed.stderr().trim()}"
            return make_report(job, false, detail, elapsed)
        case Result.Err(error):
            return make_report(job, false, f"{error}", elapsed)


def print_report(report: Report):
    mark = "ok  " if report.ok else "FAIL"
    print(f"[{mark}] {report.name} ({report.elapsed_ms} ms) {report.detail}")


def jobs_path() -> str:
    args = sys.args()
    if len(args) > 0:
        return args[0].clone()
    return "jobs.txt"


def main() -> int32:
    path = jobs_path()
    match fs.read_to_string(path):
        case Result.Ok(text):
            jobs = parse_jobs(text)
            mut failed = 0
            for job in jobs:
                report = run_job(job)
                print_report(report)
                if not report.ok:
                    failed += 1
            print(f"{len(jobs)} jobs, {failed} failed")
            return 1 if failed > 0 else 0
        case Result.Err(error):
            print(f"cannot read {path}: {error}")
            return 2
```

<span class="caption">Listing 19-4: The single-task runner, with reports and
an exit status</span>

A few details are worth reading closely.

`make_report` takes `detail: own str` but `job: Job`. The report needs to
keep the detail string, and every caller builds that string fresh (from
`stdout().trim()`, an f-string, or an error), so handing it over costs
nothing. The name is different: it belongs to the job, which the caller still
owns, so `make_report` clones it. Writing `detail: str` instead would get you
an `AU3002` telling you that the parameter is borrowed; the fix is the
`own` you see here. Signatures like this one are where Aura's ownership model
earns its keep. You can read `make_report`'s declaration and know, without
looking at the body, that it keeps the detail and leaves the job alone.

`run_job` now decides what failure means. `completed.success()` is true only
for exit code 0. For anything else, the detail combines the status with
whatever the command wrote to standard error, because that is usually where
the useful message is. A child that could not start at all becomes a failed
report too, with the `process.Error` as its detail. Everything that can go
wrong with a job ends up as a `Report`, so the caller has one kind of value to
deal with.

`jobs_path` reads the program's arguments. `sys.args()` returns the arguments
after `--` on the `aura run` command line (or after the program name, for a
built binary), without the program name itself.

Let's run it:

```console
$ aura run src/main.au
[ok  ] greet (6 ms) hello from the job runner
[ok  ] count (6 ms) 155
[ok  ] nap (1006 ms) rested
[FAIL] broken (7 ms) ExitStatus.Exited(3) about to fail
4 jobs, 1 failed
$ echo $?
1
```

Now the failure carries its reason, the timings show where the time went, and
the exit status tells a calling script that something failed. Passing a file
that does not exist takes the other path:

```console
$ aura run src/main.au -- missing.txt
cannot read missing.txt: io.Error.NotFound
$ echo $?
2
```

We are using three exit codes: 0 when every job passed, 1 when at least one
job failed, and 2 when the runner itself could not do its work. That split is
a common convention for tools like this, because "your jobs failed" and "you
gave me a bad file" usually call for different reactions.

Look again at the timings, though. The runner spent about a second on `nap`
and almost nothing on the rest, and it ran them strictly one after another.
With ten jobs that each sleep for a second, it would take ten seconds, even
though the jobs have nothing to do with each other and the machine is idle the
whole time. In the next section we will keep `run_job` exactly as it is and
change who calls it: a pool of workers, fed by a queue, so independent jobs
run at the same time.
