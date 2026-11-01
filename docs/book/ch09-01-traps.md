# Traps: failures that stop a task

Our settings loader starts out with a hopeful view of its input. Every line of
a settings file should look like `port = 9000`: a key, an equals sign, a value.
So the first version splits each line on `=` and takes the two halves. Create a
file called `src/main.au` (in a new package from `aura new settings`, or
anywhere you like) and type this in:

*Filename: src/main.au*

```aura
def parse_line(line: str) -> (str, str):
    parts = line.split("=")
    return (parts[0].trim(), parts[1].trim())

def main():
    lines = ["port = 9000", "workers = 4", "verbose"]
    for line in lines:
        (key, value) = parse_line(line)
        print(f"{key} -> {value}")
    print("loaded")
```

<span class="caption">Listing 9-1: A settings parser that assumes every line contains `=`</span>

The third line, `verbose`, is the kind of thing people write in settings files
all the time: a flag with no value. The program compiles, because nothing about
it is wrong as far as types and ownership go. Let's run it:

```console
$ aura run src/main.au
port -> 9000
workers -> 4
error[AU4003]: list index `1` is out of bounds for length `1`
 --> src/main.au:3:36
  |
3 |     return (parts[0].trim(), parts[1].trim())
  |                                    ^
  = note: Aura call chain (innermost first): parse_line at 1:1 -> main at 5:1
```

The first two lines worked. On the third, `"verbose".split("=")` produced a
list with one element, and `parts[1]` asked for a second one. Aura stopped the
program right there. This is a *trap*: a runtime failure that ends the current
task immediately. `loaded` never printed.

## Reading a trap

The diagnostic has the same shape as the compiler errors you've already met,
and it's worth reading line by line.

The header, `error[AU4003]: list index `1` is out of bounds for length `1``,
gives the code and a sentence with the actual values involved. Every runtime
diagnostic has a code in the `AU4xxx` range. `AU4003` is the code for bounds
and lookup failures; the [Manual](/manual/diagnostics) lists every code.
Because the message includes both the index and the length, you often know what
went wrong before you look at the source.

The `-->` line and the snippet under it point at the exact spot: line 3, column
36, the `1` inside `parts[1]`. The caret marks the operation that failed, not
the start of the statement.

The last line is the call chain. It reads innermost first: the trap happened in
`parse_line`, which is defined at line 1, and `parse_line` was called from
`main`, defined at line 5. In a small program that's obvious. In a program with
forty functions, the call chain is how you find the route the bad value took.
These are Aura frames only. You won't see interpreter internals or generated
code in them, and a native binary built with `aura build` reports the same
frames as `aura run`.

Notice also what happened to the output. The two lines printed before the trap
are still there. Aura writes program output to standard output and the
diagnostic to standard error, and it doesn't throw away output that was already
produced. If we hide standard error, we can see the exit status the shell gets:

```console
$ aura run src/main.au 2>/dev/null; echo $?
port -> 9000
workers -> 4
1
```

The final `1` comes from `echo $?`. A trapped program always exits with status
1, so a shell script or a CI job knows that something went wrong.

## What traps

Index errors aren't the only failures that trap. Every one of the following
stops the task with a diagnostic, and none of them can be ignored by accident.

Integer arithmetic is checked. If a result doesn't fit its type, Aura traps
with `AU4002` instead of wrapping around:

```aura
def main():
    mut retries: int8 = 125
    for attempt in range(5):
        retries += 1
    print(retries)
```

```console
$ aura run src/main.au
error[AU4002]: integer value `128` does not fit in `int8`
 --> src/main.au:4:9
  |
4 |         retries += 1
  |         ^
  = note: Aura call chain (innermost first): main at 1:1
```

C would quietly turn `128` into `-128` here, and Python would quietly grow the
integer. Aura does neither, because a counter that silently changes sign is a
bug that surfaces far from where it happened.

Dividing by zero traps with `AU4004`:

```aura
def share(budget: int64, workers: int64) -> int64:
    return budget // workers

def main():
    print(share(1000, 4))
    print(share(1000, 0))
```

```console
$ aura run src/main.au
250
error[AU4004]: division by zero
 --> src/main.au:2:12
  |
2 |     return budget // workers
  |            ^
  = note: Aura call chain (innermost first): share at 1:1 -> main at 4:1
```

A few other operations trap for the same reason: they were asked to do
something impossible with the values they were given. Reading a dictionary key
that isn't there with `settings["timeout"]` traps with `AU4003`. A numeric
conversion with `as` that doesn't fit, such as `5000000000 as int32`, traps
with `AU4002` (we'll look at conversions closely in [Chapter
10](/book/ch10-00-generics-traits-conversions)). And an assertion that turns
out to be false traps with `AU4001`, which brings us to the one trap you write
on purpose.

## Assertions

An `assert` statement states something that must be true for the program to
continue. If the condition is `false`, the task traps at the `assert`. Let's
say `share` should never be called with zero workers, and we want the program
to say so clearly rather than failing inside the division:

```aura
def share(budget: int64, workers: int64) -> int64:
    assert workers > 0, f"share() needs at least one worker, got {workers}"
    return budget // workers

def main():
    print(share(1000, 0))
```

```console
$ aura run src/main.au
error[AU4001]: share() needs at least one worker, got 0
 --> src/main.au:2:5
  |
2 |     assert workers > 0, f"share() needs at least one worker, got {workers}"
  |     ^
  = note: left = 0
  = note: right = 0
  = note: Aura call chain (innermost first): share at 1:1 -> main at 5:1
```

The message after the comma became the diagnostic's message. Without one, the
message is `assertion failed`. The two `note` lines are something the compiler
added for us: when the condition is a single comparison such as `workers > 0`,
a failed assertion reports the value on each side. Here `left` is `workers` and
`right` is the literal `0`. You'll come to rely on this in tests, where seeing
the actual value saves you from adding a `print` and running again. [Chapter
11](/book/ch11-00-testing) has more on how those values are reported.

The message is evaluated only when the assertion fails. So you can build an
expensive message, or call a function to describe the state, and pay for it
only on the failure path:

```aura
def explain(workers: int64) -> str:
    print("building the message")
    return f"bad worker count {workers}"

def main():
    workers = 4
    assert workers > 0, explain(workers)
    print("ok")
```

This prints only `ok`. The condition was true, so `explain` never ran.

The condition has to be a `bool`. If you're coming from Python you might write
`assert workers` to mean "workers is not zero", and Aura won't accept it:

```aura check-fail:AU2002
def share(budget: int64, workers: int64) -> int64:
    assert workers, "share() needs at least one worker"
    return budget // workers
```

```console
$ aura check src/main.au
error[AU2002]: `assert` condition must have type `bool`, found `int64`
 --> src/main.au:2:5
  |
2 |     assert workers, "share() needs at least one worker"
  |     ^
  = help: Aura has no implicit truthiness; compare the value explicitly, for example `value != 0`
```

Aura has no truthiness anywhere in the language, so `assert` is no exception.
Write the comparison you mean.

One more property of assertions matters a great deal: Aura never removes them.
There's no release mode, optimization flag, or environment variable that strips
assertions out, the way `python -O` does. An `assert` you write runs in every
build. That makes assertions trustworthy for checking your own program's logic,
and it also means they are the wrong tool for checking input from the outside
world, as we'll see in [To trap or not to trap](/book/ch09-03-to-trap-or-not).

## What happens when a task traps

A trap stops the task it happens in. It doesn't stop instantly, though: before
the task ends, Aura runs any cleanup that the task had registered with `with`.
Here's a lock file that has to be removed however the block ends:

```aura
class Lockfile:
    path: str

    def __exit__(mut self) -> None:
        print(f"removing {self.path}")

def main():
    lines = ["port = 9000", "verbose"]
    with lock = Lockfile(path="settings.lock"):
        print(f"holding {lock.path}")
        parts = lines[1].split("=")
        print(parts[1])
    print("finished")
```

```console
$ aura run src/main.au
holding settings.lock
removing settings.lock
error[AU4003]: list index `1` is out of bounds for length `1`
 --> src/main.au:12:21
   |
12 |         print(parts[1])
   |                     ^
  = note: Aura call chain (innermost first): main at 7:1
```

`removing settings.lock` printed before the diagnostic, and `finished` never
printed. The `with` block's cleanup ran on the way out, and then the trap ended
`main`. If a cleanup step itself fails during a trap, the original trap stays
the one you see, because that's the failure you need to fix. [Chapter
14](/book/ch14-00-resources-and-cleanup) covers `with` and cleanup order in
full.

When `main` is the task that traps, the whole program ends. A task started
inside a task group is different. A trap ends that task only, and the task that
started it finds out by looking at the result:

```aura
def value_of(line: str) -> str:
    parts = line.split("=")
    return parts[1].trim()

def main():
    with group = TaskGroup():
        good = group.start(value_of, "port = 9000")
        bad = group.start(value_of, "verbose")
        match good.result():
            case TaskResult.Ready(value):
                print(f"good task: {value}")
            case TaskResult.Error(message):
                print(f"good task trapped: {message}")
            case TaskResult.Cancelled:
                print("good task was cancelled")
            case TaskResult.TimedOut:
                print("good task timed out")
        match bad.result():
            case TaskResult.Ready(value):
                print(f"bad task: {value}")
            case TaskResult.Error(message):
                print(f"bad task trapped: {message}")
            case TaskResult.Cancelled:
                print("bad task was cancelled")
            case TaskResult.TimedOut:
                print("bad task timed out")
    print("main is still running")
```

```console
$ aura run src/main.au
good task: 9000
bad task trapped: list index `1` is out of bounds for length `1`
main is still running
```

The second task trapped on the same `parts[1]` as before. Its trap became a
`TaskResult.Error` value in `main`, and `main` carried on. This is how a server
keeps running when one request handler hits a bug: the handler's task is the
unit that fails. Don't worry about the task group syntax yet; [Chapter
15](/book/ch15-00-concurrency) builds it up properly. The point here is the
boundary. A trap always ends a whole task, and the only place you can observe
it is from outside that task.

## No catching

If you've written Python, you may be reaching for `try` and `except` to wrap
`parse_line` and recover from the bad line. Aura doesn't have them, and the
compiler tells you so:

```aura check-fail:AU2005
def parse_port(text: str) -> int64:
    try:
        return int(text)
    except ValueError:
        return 8080
```

```console
$ aura check src/main.au
error[AU2005]: `try`/`except` is not supported; use `Result` with `match` today
 --> src/main.au:2:5
  |
2 |     try:
  |     ^
```

There is no way, inside a task, to catch a trap and continue. Aura does this
deliberately. A trap means the program reached a state its author didn't plan
for. Code that catches every failure and carries on tends to carry on with
half-updated data, and the real bug shows up later somewhere unrelated. By
making traps uncatchable, Aura guarantees that when one happens you see it, at
the line where it happened, with the values that caused it.

That leaves our settings loader in an awkward spot. A line without `=` isn't a
bug in our program. It's a mistake in a file that some person wrote, and a good
loader should report it politely and keep its own state intact. For failures
like that, Aura has a second mechanism, and it's the subject of the next
section.
