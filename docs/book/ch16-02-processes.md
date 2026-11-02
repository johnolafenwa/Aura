# Running processes and supervisors

A health checker that can only look for files is not very healthy. The most
flexible check is "run this command and tell me whether it worked", because
then any existing tool becomes a check: `pg_isready`, `systemctl is-active`, a
shell script your team already has. We will fill in the `Command` probe,
then look at talking to a running child through pipes, and finish with
supervisors, which keep a set of children alive under a restart policy.

## Running a command to completion

The `process` module has three entry points, and which one you want depends on
how long the child lives relative to your code. `process.run` starts a
command, waits for it to finish, and hands back everything it printed. That is
exactly what a check needs. Add `import process` at the top of
*src/main.au* and write the function:

```aura fragment
def run_command(argv: list[str]) -> str:
    match own process.run(argv, timeout=2s, group=true):
        case Result.Ok(completed):
            if completed.success():
                return "ok"
            return f"failed ({completed.status()})"
        case Result.Err(error):
            return f"error ({error})"
```

<span class="caption">Listing 16-6: Running a command check with a deadline</span>

Then point the `Command` arm of `run_check` at it:

```aura fragment
        case Probe.Command(argv):
            return run_command(argv)
```

Before running it, let's read the call. The first argument is the command as a
`list[str]`: the program, then each argument as its own element. This is
called an argument vector, or argv, and it is the only way to describe a
command in Aura. There is no `shell=True`, and no function that takes a
command line as one string. The `argv` from our config line
`greeting cmd /bin/echo hello` is `["/bin/echo", "hello"]`, and that is
exactly what the operating system receives.

This matters more than it might seem. In Python,
`subprocess.run(f"grep {pattern} log.txt", shell=True)` hands the string to
`/bin/sh`, which splits it on spaces, expands `$VARIABLES`, and obeys `;` and
`|`. If `pattern` came from a user, the user can run anything they like. With
an argv list there is nothing to inject into, because nothing is parsed:

```aura
import process

def main():
    match own process.run(["/bin/echo", "$HOME; rm -rf /"], timeout=1s):
        case Result.Ok(completed):
            print(completed.stdout().trim())
        case Result.Err(error):
            print(error)
```

```console
$ aura run argv.au
$HOME; rm -rf /
```

`echo` received one argument and printed it. If you really want a shell, say
so explicitly with `["/bin/sh", "-c", script]`, and then the quoting problem
is yours, in plain sight.

The program name does not have to be an absolute path. A bare `"echo"` is
looked up on `PATH` the way a shell would. Absolute paths are better in a
config file that will run on other machines, because they do not depend on
the environment of whoever starts `checkup`.

The named arguments do the rest. `timeout=2s` is a `Duration` literal, the
same kind of value [Chapter 15](/book/ch15-00-concurrency) used for queue
waits. If the child has not exited after two seconds, `process.run`
terminates it, waits for it to go away, and returns an error. If you leave
`timeout` out, there is no deadline at all. (A negative duration does not
mean "no deadline"; it is invalid input and returns an error.) `group=true`
starts the child in its own process group on Unix hosts. When Aura terminates
a grouped child it signals the whole group, so a shell script that started
three helpers of its own does not leave those helpers running after the
script is killed. For checks, which might be arbitrary scripts, that is
exactly what we want.

By default `process.run` gives the child an empty standard input and captures
its standard output and standard error, each up to 64 MiB. The `Completed`
value it returns answers questions about the finished process: `success()` is
`true` for exit code `0`, `status()` gives the `process.ExitStatus`, and
`stdout()` and `stderr()` return the captured text.

Let's add some commands to *checks.txt*, including a few that should go
wrong:

*Filename: checks.txt*

```text
# name    kind  target
config    file  checks.txt
notes     file  notes.txt
greeting  cmd   /bin/echo hello
listing   cmd   /bin/ls /no/such/dir
slow      cmd   /bin/sleep 5
typo      cmd   /bin/ehco hello
```

```console
$ aura run src/main.au
config: ok
notes: missing
greeting: ok
listing: failed (ExitStatus.Exited(2))
slow: error (process.Error.TimedOut)
typo: error (process.Error.Spawn(No such file or directory (os error 2)))
```

There are three different kinds of failure here, and the types keep them
apart. `listing` ran and exited with code `2`, which is `ls` saying the
directory does not exist. That is a successful `process.run` (it got a
`Completed` back) of an unsuccessful command, and our code reported it from
the `Ok` arm. `slow` was still sleeping after two seconds, so `process.run`
killed it and returned `process.Error.TimedOut`. And `typo` never started at
all: the operating system could not find `/bin/ehco`, so we got
`process.Error.Spawn` with the system's message. The whole run took a little
over two seconds, almost all of it spent waiting for `slow`.

`ExitStatus` has two variants. `Exited(code)` is a normal exit with a numeric
code. `Signaled(signal)` means a signal killed the process, which is how you
will see a child that crashed or was killed from outside:

```aura
import process

def main():
    match own process.run(["/bin/sh", "-c", "echo oops >&2; kill -9 $$"], timeout=1s):
        case Result.Ok(completed):
            print(completed.status())
            print(completed.stderr().trim())
        case Result.Err(error):
            print(error)
```

```console
$ aura run signal.au
ExitStatus.Signaled(9)
oops
```

When a failed command should stop the current function, `Completed`
has a shortcut: `try completed.check()` returns `Ok(None)` for a successful
exit and an error otherwise. We do not use it in `checkup`, because a failing
check is a result to report, not a reason to stop.

### Working directory, environment, and bytes

Two more named arguments shape the child's world. `cwd` sets the directory it
starts in, and `env` is a dictionary of variables added on top of the
environment the parent already has:

```aura
import process

def main():
    match own process.run(["/bin/sh", "-c", "echo $GREETING from $(pwd)"], cwd="/tmp", env={"GREETING": "hi"}, timeout=1s):
        case Result.Ok(completed):
            print(completed.stdout().trim())
        case Result.Err(error):
            print(error)
```

```console
$ aura run env.au
hi from /tmp
```

Captured output is bytes until you ask for text. `stdout()` decodes it as
strict UTF-8, and if the child printed something that is not UTF-8, that call
traps:

```aura
import process

def main():
    match own process.run(["/bin/sh", "-c", "printf '\\377ok'"], timeout=1s):
        case Result.Ok(completed):
            print(completed.stdout_bytes())
            print(completed.stdout())
        case Result.Err(error):
            print(error)
```

```console
$ aura run bytes.au
[255, 111, 107]
error[AU4005]: received non-UTF-8 data: invalid utf-8 sequence of 1 bytes from index 0
 --> bytes.au
  = note: Aura call chain (innermost first): main at 3:1
```

`stdout_bytes()` gave us the three raw bytes, and then `stdout()` stopped the
program. When you run a tool whose output you do not control, read the bytes
and convert them with `str.from_bytes`, which returns a `Result` you can
handle instead of a trap.

## Talking to a running child

`process.run` is for commands that start, work, and finish. Sometimes you
need to feed a child input while it runs, or read its output as it arrives.
For that, `process.start` returns a live `process.Child` immediately. Let's
use it to sort some text with the system's `sort`, writing to its standard
input and reading its standard output. A first try:

```aura check-fail:AU3003
import process

def sort_lines(text: str) -> Result[str, process.Error]:
    with child = try process.start(["/usr/bin/sort"], stdin=process.pipe(), stdout=process.pipe(), group=true):
        match child.stdin():
            case process.Pipe as input:
                try input.write_all(text, timeout=1s)
                input.close()
            case None:
                pass
        return Result.Ok("")

def main():
    print(sort_lines("b\na\n"))
```

```console
$ aura run sort.au
error[AU3003]: method `write_all` requires a mutable receiver, but `input` is bound by a shared match on `child.stdin()`
 --> sort.au:7:27
  |
7 |                 try input.write_all(text, timeout=1s)
  |                           ^
  = related sort.au:6:34: shared match binding is declared here
  = help: write `match own child.stdin()` to own the matched value
```

This is the same lesson as `match own` in the last section, from a different
angle. `child.stdin()` returns `process.Pipe | None`: the pipe if we asked for
one with `stdin=process.pipe()`, and `None` if we did not. A plain `match`
binds `input` as a shared view, and writing to a pipe or closing it changes
it, which needs more than shared access. The fix the compiler suggests,
`match own`, moves the pipe out of the result so the arm owns it. Here is the
complete function:

```aura
import process

def sort_lines(text: str) -> Result[str, process.Error]:
    with child = try process.start(["/usr/bin/sort"], stdin=process.pipe(), stdout=process.pipe(), group=true):
        match own child.stdin():
            case process.Pipe as input:
                try input.write_all(text, timeout=1s)
                input.close()
            case None:
                return Result.Err(process.Error.Other(message="stdin was not piped"))

        mut sorted = ""
        match own child.stdout():
            case process.Pipe as output:
                sorted = try output.read_all()
            case None:
                return Result.Err(process.Error.Other(message="stdout was not piped"))

        try child.wait_ok(timeout=1s)
        return Result.Ok(sorted)

def main():
    match sort_lines("pear\napple\nfig\n"):
        case Result.Ok(text):
            print(text.trim())
        case Result.Err(error):
            print(error)
```

<span class="caption">Listing 16-7: Writing to a child's stdin and reading its stdout</span>

```console
$ aura run sort.au
apple
fig
pear
```

The order of operations is the important part. We write all the input, then
close stdin, then read the output, then wait for the exit. `sort` cannot
print anything until it has seen the end of its input, and the only way it
learns the input has ended is that the pipe closes. Let's see what happens if
we leave out `input.close()` and read one line with a timeout:

```aura
import process

def forgetful() -> Result[None, process.Error]:
    with child = try process.start(["/usr/bin/sort"], stdin=process.pipe(), stdout=process.pipe(), group=true):
        match own child.stdin():
            case process.Pipe as input:
                try input.write_all("pear\napple\n", timeout=1s)
            case None:
                pass
        match own child.stdout():
            case process.Pipe as output:
                print(try output.read_line(timeout=500ms))
            case None:
                pass
        return Result.Ok(None)

def main():
    print(forgetful())
```

```console
$ aura run forgetful.au
Result.Err(process.Error.TimedOut)
```

`sort` is still waiting for more input, and we are waiting for its output.
Without the timeout this program would hang forever, which is the classic
pipe deadlock. Every pipe read and write accepts a `timeout`, and an expired
one is an error (`process.Error.TimedOut`), never a quiet `None`. `None` from
`read_line` means only one thing: the child closed its end. Using timeouts on
pipe operations turns "my tool hangs sometimes" into an error message that
says where it was stuck.

Notice also that the child is bound with `with`. When the `with` block ends,
the `Child` is closed, and closing a child that is still running terminates
it: it sends a polite termination signal, waits briefly, and kills it if it
does not leave. In `forgetful`, `sort` was still running when we returned, and
the `with` block cleaned it up. Without that, every early `return` would need
its own `child.kill()`.

`Child` has three ways to wait. `wait(timeout=...)` returns a `process.Wait`
enum with `Exited`, `TimedOut`, `Cancelled`, and `Failed` variants.
`wait_or_none` folds a timeout into `Ok(None)`. `wait_ok`, which we used,
returns the status only for a successful exit and turns anything else into an
error, which suits a `try`. None of them kill the child on timeout; they only
stop waiting. That is different from `process.run`, which owns the child's
whole life and kills it when its deadline passes.

## Supervisors

The last level of process management is for programs that keep children
running: a dev server that restarts a crashed backend, an agent that keeps a
pool of workers alive. `process.supervisor()` returns a `Supervisor`, which
owns a set of named children, restarts them according to a policy, and
reports what happened as a stream of events.

Let's give it a deliberately flaky child: a shell script that fails on its
first two runs and succeeds on the third, counting attempts in a file.

```aura
import process

def watch() -> Result[None, process.Error]:
    script = "n=$(cat attempts 2>/dev/null || echo 0); n=$((n + 1)); echo $n > attempts; test $n -ge 3"
    with supervisor = process.supervisor():
        try supervisor.start(name="flaky", command=["/bin/sh", "-c", script], restart=process.RestartPolicy.OnFailure, backoff=50ms, max_restarts=5)
        while not supervisor.is_empty():
            match own try supervisor.wait_or_none(timeout=2s):
                case process.SupervisorEvent as event:
                    print(event)
                case None:
                    print("no event yet")
    return Result.Ok(None)

def main() -> int32:
    match watch():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
```

<span class="caption">Listing 16-8: Restarting a failing child under a supervisor</span>

```console
$ aura run flaky.au
SupervisorEvent.Restarted(flaky, ExitStatus.Exited(1), 1)
SupervisorEvent.Restarted(flaky, ExitStatus.Exited(1), 2)
SupervisorEvent.Exited(flaky, ExitStatus.Exited(0), 2)
$ cat attempts
3
```

`supervisor.start` takes a unique name, the argv, and the lifecycle settings.
`restart=process.RestartPolicy.OnFailure` restarts the child after an
unsuccessful exit; the other policies are `Never` and `Always`. `backoff` is
the pause before each restart, and it must be at least `10ms` whenever
restarts are enabled, so a broken command cannot spin in a tight loop.
`max_restarts` caps the number of restarts; leave it out for no limit.
Supervised children default to `group=true`, so stopping one stops its
descendants.

Each event says what happened, to which child, and how many restarts it has
had. The first two runs exited with `1` and were restarted; the third exited
with `0`, which `OnFailure` does not restart, so we got `Exited` and the
supervisor became empty, ending our loop. `wait_or_none` returns
`Result[process.SupervisorEvent | None, process.Error]`, where `None` means the
timeout passed with no event, the same three-way shape as `io.read_line`.

Names are how you refer to children, so the supervisor refuses duplicates:

```aura
import process

def watch() -> Result[None, process.Error]:
    with supervisor = process.supervisor():
        try supervisor.start(name="broken", command=["/bin/false"], restart=process.RestartPolicy.OnFailure, backoff=50ms, max_restarts=2)
        try supervisor.start(name="sleeper", command=["/bin/sleep", "30"], restart=process.RestartPolicy.Never)
        match supervisor.start(name="sleeper", command=["/bin/sleep", "1"]):
            case Result.Ok(_):
                print("second sleeper started")
            case Result.Err(error):
                print(error)
        for _ in range(0, 4):
            match own try supervisor.wait_or_none(timeout=1s):
                case process.SupervisorEvent as event:
                    print(event)
                case None:
                    print("no event yet")
        print(supervisor.is_empty())
    return Result.Ok(None)

def main():
    print(watch())
```

```console
$ aura run names.au
process.Error.Other(supervisor already manages a child named `sleeper`)
SupervisorEvent.Restarted(broken, ExitStatus.Exited(1), 1)
SupervisorEvent.Restarted(broken, ExitStatus.Exited(1), 2)
SupervisorEvent.Exited(broken, ExitStatus.Exited(1), 2)
no event yet
false
Result.Ok(None)
```

The second `sleeper` was rejected and the first one kept running. `broken`
used up its two restarts and was left exited. And the 30-second `sleeper`?
It was still running when the loop finished, so `is_empty()` was `false`.
Then the `with` block ended, which closed the supervisor, and closing a
supervisor stops every child it still manages. The program finished in about
a second, and no `sleep 30` was left behind. That guarantee holds for every
way out of the block, including an error returned by one of the `try`s, which
is why a supervisor belongs in a `with` and why you rarely need its explicit
`stop()`.

Picking a policy is a judgment about each child. `Always` will happily restart
a program that fails because its config file is wrong, a hundred times.
`Never` turns a one-second network blip into an outage. Most services want
`OnFailure` with a backoff and a restart cap, and a supervisor lets you choose
per child, not per program. [Chapter 19](/book/ch19-00-final-project) builds a
complete supervised job runner on these pieces.

The [Process Module](/manual/process) page in the Manual has every signature,
default, and error variant. There are things the module deliberately does not
have: no shell, no pipeline builder, no pseudo-terminal. Each of those either
reintroduces string parsing or hides process lifetimes, and the module's job
is to keep both explicit.

`checkup` can now run commands. Next we will teach it HTTP, and on the way we
will write a small server of our own to check against.
