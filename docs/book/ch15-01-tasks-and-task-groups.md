# Tasks and task groups

A *task* is a function call that runs concurrently with the code that started
it. Tasks are lightweight: each has its own small stack, and the Aura runtime
schedules thousands of them over a handful of operating system threads. You
never create a task on its own. You ask a `TaskGroup` to start one, and the
group is responsible for it from then on.

Let's count the words in a sentence on a separate task:

*Filename: src/main.au*

```aura
def count_words(text: str) -> int64:
    return len(text.split(" "))

def main():
    with group = TaskGroup():
        task = group.start(count_words, "the quick brown fox")
        match task.result(timeout=1s):
            case TaskResult.Ready(count):
                print(f"words: {count}")
            case TaskResult.Error(message):
                print("failed: " + message)
            case TaskResult.TimedOut:
                print("timed out")
            case TaskResult.Cancelled:
                print("cancelled")
    print("group closed")
```

<span class="caption">Listing 15-1: Starting one task and waiting for its result</span>

```console
$ aura run src/main.au
words: 4
group closed
```

There are three pieces here. `TaskGroup()` creates the group, and `with` makes
the group's lifetime the indented block. `group.start(count_words, "the quick
brown fox")` starts a task that will call `count_words` with that argument. Notice
that we pass the function and its arguments separately, rather than calling
`count_words(...)` ourselves. Calling it would run it here and now, on the
current task. Passing it lets the group run it somewhere else.

`start` returns a `Task[int64]`, a handle to the result that will exist once the
task finishes. `task.result(timeout=1s)` waits for that result, for at most one
second. The wait can end in four ways, and `TaskResult[T]` has a case for each:
the task returned a value, the task trapped, our wait ran out of time, or our
wait was cancelled. Aura makes us handle all four because each is a real
possibility in concurrent code, and each usually needs different handling. A
timeout, in particular, says nothing about the task itself; it only means *we*
stopped waiting. We'll come back to timeouts in
[Section 15.3](/book/ch15-03-cancellation-and-shutdown).

The timeout is a `Duration`. Literals like `1s`, `100ms`, and `2m` cover the
common cases, and you'll see how to compute durations at runtime later in the
chapter.

## Why the group is a `with` block

The line that matters most in Listing 15-1 is the one with no code on it: the
end of the `with` block. Leaving the block runs the group's exit, and a task
group's exit *waits for every task the group started*. So by the time `group
closed` prints, `count_words` has finished, whatever happened to it. No task
started inside the block is still running after it.

That is what *structured concurrency* means. A task's lifetime is nested inside
a block of code, the same way a binding's lifetime is nested inside its scope.
You can read the structure of the concurrent program off the indentation.

To see why that's worth a rule, think about the alternative. In Python, this is
legal and common:

```python
import threading

def save_report(): ...

def handle_request():
    threading.Thread(target=save_report).start()
    return "ok"
```

`handle_request` returns while `save_report` is still running. Nothing in the
code says who is waiting for it, so if it fails, the exception is printed to
standard error by the thread machinery and the caller never hears about it. If
the program exits while it runs, a daemon thread is killed mid-write, and a
non-daemon thread holds the exit up until it finishes. `asyncio.create_task`
has the same shape, plus a twist: the event loop holds only a weak reference to
the task, so a task nobody keeps a reference to can be garbage collected before
it finishes. Every one of these is a bug that comes from a task outliving the
code that started it.

Python's answer, since version 3.11, is `asyncio.TaskGroup`, which follows the
"nurseries" of the Trio library. Inside `async with asyncio.TaskGroup() as tg:`,
tasks started with `tg.create_task` are awaited when the block ends. Aura's
`TaskGroup` is the same idea. The difference is that Python still has
`create_task` and threads alongside it, so structure is a convention you have to
keep. Aura has no way to start a detached task.

### The group can't leave its block

What stops us from smuggling the group out of its block, so that its tasks keep
running after the function returns? Let's try:

```aura check-fail:AU2999
def count_words(text: str) -> int64:
    return len(text.split(" "))

def launch() -> TaskGroup:
    with group = TaskGroup():
        group.start_soon(count_words, "the quick brown fox")
        return group

def main():
    group = launch()
```

```console
$ aura run src/main.au
error[AU2999]: cannot move managed `with` resource `group`
 --> src/main.au:7:16
  |
7 |         return group
  |                ^
```

This is the rule from [Chapter 14](/book/ch14-01-with-and-cleanup): a value
managed by `with` belongs to its block, and the compiler won't let it move out.
For a file, that guarantees the file is closed where the block ends. For a task
group, it guarantees the tasks are finished where the block ends.

You can still pass the group to a helper function, as long as the helper only
borrows it. The helper starts tasks that belong to the caller's group, and the
caller's block waits for them:

```aura
def report(name: str, text: str):
    print(f"{name}: {len(text.split(" "))} words")

def start_reports(group: TaskGroup, documents: own list[str]):
    mut n = 0
    for text in own documents:
        group.start_soon(report, f"doc {n}", text)
        n += 1

def main():
    with group = TaskGroup():
        start_reports(group, ["the quick brown fox", "jumps over"])
    print("all reports done")
```

```console
$ aura run src/main.au
doc 0: 4 words
doc 1: 2 words
all reports done
```

### Always open a group with `with`

The current compiler accepts a `TaskGroup()` that isn't bound with `with`, and
that is a hole you should know about. Such a group has no block, so nothing
waits for its tasks:

```aura
def work():
    sleep(100ms)
    print("child finished")

def launch():
    group = TaskGroup()
    group.start_soon(work)
    print("launch returning")

def main():
    launch()
    print("back in main")
```

```console
$ aura run src/main.au
launch returning
back in main
```

`launch` returned with its task still running, and when `main` finished, the
runtime abandoned the task before it could print. As we saw at the end of
[Chapter 14](/book/ch14-02-resource-managers), an abandoned task's `with` exits
don't run either. Every guarantee in this chapter assumes the group is opened
with `with`, so always write it that way.

## Starting several tasks

One task isn't much use. Let's count three documents at once, keep the handles
in a list, and wait for all of them. Here's the obvious first attempt:

```aura check-fail:AU3002
def count_words(text: str) -> int64:
    return len(text.split(" "))

def main():
    documents = ["the quick brown fox", "jumps over", "the lazy dog again"]
    mut tasks: list[Task[int64]] = []
    with group = TaskGroup():
        for doc in documents:
            tasks.append(group.start(count_words, doc))
        match wait_all(tasks, timeout=1s):
            case WaitAll.Ready(counts):
                print(counts)
            case WaitAll.Error(index, message):
                print(f"document {index} failed: {message}")
            case WaitAll.TimedOut:
                print("timed out")
            case WaitAll.Cancelled:
                print("cancelled")
```

```console
$ aura run src/main.au
error[AU3002]: cannot move borrowed value `doc`
 --> src/main.au:9:51
  |
9 |             tasks.append(group.start(count_words, doc))
  |                                                   ^
  = related src/main.au:8:13: value is borrowed here
  = help: take `doc` as `own str` when ownership is required, or call `.clone()` to consume an independent copy
  = fix: replace src/main.au:9:54-9:54 with `.clone()`
```

The error points at `doc` in the call to `start`, and the related line tells us
that `doc` is borrowed from the loop on line 8. A plain `for doc in documents`
gives the body shared access to each element (that's
[Chapter 4](/book/ch04-00-understanding-ownership)), and `start` wants to
*move* its arguments.

That's the key ownership rule for tasks: starting a task moves (or copies) each
argument into storage that belongs to the task. The child never borrows from the
parent's stack. It has to be this way, because the child runs concurrently with
the parent. If the child borrowed `doc`, the parent could change or drop the
list while the child was still reading from it, and there would be no point at
which the compiler could check that the borrow had ended. Moving the argument
makes the child the only owner of its input.

The help line offers two fixes. We could `.clone()` each string, which leaves
`documents` intact. Here we don't need `documents` afterwards, so the cheaper
fix is to hand each string over with `for doc in own documents`:

*Filename: src/main.au*

```aura
def count_words(text: str) -> int64:
    return len(text.split(" "))

def main():
    documents = ["the quick brown fox", "jumps over", "the lazy dog again"]
    mut tasks: list[Task[int64]] = []
    with group = TaskGroup():
        for doc in own documents:
            tasks.append(group.start(count_words, doc))
        match wait_all(tasks, timeout=1s):
            case WaitAll.Ready(counts):
                print(counts)
            case WaitAll.Error(index, message):
                print(f"document {index} failed: {message}")
            case WaitAll.TimedOut:
                print("timed out")
            case WaitAll.Cancelled:
                print("cancelled")
```

<span class="caption">Listing 15-2: Starting a task per document and waiting for all of them</span>

```console
$ aura run src/main.au
[4, 2, 4]
```

`wait_all` takes the list of handles and waits until every task has produced a
value, or one has failed, or the timeout passes. On success, `WaitAll.Ready`
carries the results in the same order as the handles, not the order in which
the tasks finished. There's a matching `wait_any` that returns as soon as the
first task finishes, with the index of the one that won.

Notice that the parameter of `count_words` is a plain `text: str`. The
function borrows its argument as usual; what it borrows from is the task's own
copy. The same holds for `own` parameters, which consume the task's copy. The one
kind of parameter a task target can't have is `mut`, because there's nobody to
write the change back to:

```aura check-fail:AU3002
def add_one(counts: mut list[int64]):
    counts.append(1)

def main():
    mut counts: list[int64] = []
    with group = TaskGroup():
        group.start_soon(add_one, counts)
    print(counts)
```

```console
$ aura run src/main.au
error[AU3002]: task starting does not support mutable parameter `counts` on function `add_one`; child tasks cannot write back through the starting call frame
 --> src/main.au:7:26
  |
7 |         group.start_soon(add_one, counts)
  |                          ^
```

If a task needs to produce something, it returns it, or sends it through a
queue, which is the subject of the next section.

## Fire and forget, inside a scope

When we don't need a task's result, `start_soon` starts it without returning a
handle:

```aura
def report(name: str, text: str):
    print(f"{name}: {len(text.split(" "))} words")

def main():
    with group = TaskGroup():
        group.start_soon(report, "a", "the quick brown fox")
        group.start_soon(report, "b", "jumps over")
        group.start_soon(report, "c", "the lazy dog again")
    print("all reports done")
```

```console
$ aura run src/main.au
a: 4 words
c: 4 words
b: 2 words
all reports done
```

Run this a few times and the first three lines may come out in different
orders. The order in which independent tasks run, and so the order of their
output, is deliberately unspecified. The last line, though, is always last,
because the group's exit waits for all three. "Fire and forget" in Aura means
you forget the handle, not the task: the group still owns it.

## When a task fails

A task can trap, like any code. What happens next depends on whether anybody is
looking at its result. Let's put an assertion in `count_words` and give it an
empty document:

```aura
def count_words(text: str) -> int64:
    assert len(text) > 0, "empty document"
    return len(text.split(" "))

def main():
    documents = ["the quick brown fox", "", "the lazy dog again"]
    mut tasks: list[Task[int64]] = []
    with group = TaskGroup():
        for doc in own documents:
            tasks.append(group.start(count_words, doc))
        match wait_all(tasks, timeout=1s):
            case WaitAll.Ready(counts):
                print(counts)
            case WaitAll.Error(index, message):
                print(f"document {index} failed: {message}")
            case WaitAll.TimedOut:
                print("timed out")
            case WaitAll.Cancelled:
                print("cancelled")
    print("after the group")
```

```console
$ aura run src/main.au
document 1 failed: empty document
after the group
```

The trap stopped the task, not the program. `wait_all` reported it as
`WaitAll.Error` with the index of the failing task and the assertion's message,
and because we observed the failure, it's handled: the group closes normally.

Now the same failure with `start_soon`, where there is no handle to observe it
through:

```aura
def report(text: str):
    assert len(text) > 0, "empty document"
    print(f"{len(text.split(" "))} words")

def main():
    with group = TaskGroup():
        group.start_soon(report, "the quick brown fox")
        group.start_soon(report, "")
        group.start_soon(report, "jumps over")
    print("after the group")
```

```console
$ aura run src/main.au
4 words
2 words
error[AU4001]: empty document
 --> src/main.au:2:5
  |
2 |     assert len(text) > 0, "empty document"
  |     ^
  = note: left = 0
  = note: right = 0
  = note: Aura call chain (innermost first): report at 1:1
  = note: Aura task entry: report at 1:1
  = note: Aura task ancestry (youngest first): report spawned from main at 8:15
```

This time `after the group` never prints. An unread failure doesn't disappear:
when the group closes, it raises the child's trap in the parent. The two notes
at the bottom are specific to tasks. `Aura task entry` names the function the
task was started with, and `Aura task ancestry` says which code started it: line
8 of `main`, the second `start_soon`. In a program with many tasks, that's
how you find which one went wrong.

Compare this with the Python thread from earlier, whose exception would be
printed and then forgotten. In Aura, a failure always reaches some code that can
see it: the code reading the result, or failing that, the block that owns the
task.

### A failure doesn't cancel the siblings

There's one more detail worth testing, because it differs from Python's
`asyncio.TaskGroup`. When one child fails and nobody reads its result, what
happens to its siblings?

```aura
def slow():
    sleep(2s)
    print("slow child finished")

def bad():
    assert false, "bad child"

def main():
    with group = TaskGroup():
        group.start_soon(slow)
        group.start_soon(bad)
    print("after the group")
```

```console
$ time aura run src/main.au
slow child finished
error[AU4001]: bad child
 --> src/main.au:6:5
  |
6 |     assert false, "bad child"
  |     ^
  = note: Aura call chain (innermost first): bad at 5:1
  = note: Aura task entry: bad at 5:1
  = note: Aura task ancestry (youngest first): bad spawned from main at 11:15

real	0m2.046s
user	0m0.294s
sys	0m0.022s
```

`bad` traps almost immediately, but the program takes two seconds: the group
let `slow` run to completion and then reported the failure. In Python,
`asyncio.TaskGroup` cancels the remaining tasks as soon as one raises. Aura's
group does not cancel on its own. If you want "one failure stops everyone", you
observe the failure and call `group.cancel()` yourself, which is what
[Section 15.3](/book/ch15-03-cancellation-and-shutdown) is about. The upside is
that cancellation in Aura always comes from a line of code you can point to.

## What can cross into a task

Tasks in Aura really do run in parallel. The runtime starts one scheduler worker
per core the host reports (you can set the count with the `AURA_WORKERS`
environment variable), and each task is pinned to one worker for its whole life.
There is no global interpreter lock, so two CPU-bound tasks can use two cores at
once, unlike two Python threads.

Parallel execution is exactly when shared mutable data becomes dangerous, so
Aura doesn't allow it. Everything passed to a task, and everything a task
returns, must be `Transfer`: a value that can be moved to another worker and
used there without anyone else holding on to it. The compiler works out which
types are `Transfer` from their structure. Numbers, strings, lists and
dictionaries of transferable values, and your own classes and enums built from
them all qualify. A live host resource doesn't, because an open file handle
belongs to the worker that opened it:

```aura check-fail:AU3008
import fs

def read_it(file: own fs.File):
    print("reading")

def main():
    match own fs.open("notes.txt"):
        case Result.Ok(file):
            with group = TaskGroup():
                group.start_soon(read_it, file)
        case Result.Err(error):
            print(f"{error}")
```

```console
$ aura run src/main.au
error[AU3008]: task argument `file` cannot cross a task boundary because `fs.File` is a host resource and is not Transfer
 --> src/main.au:10:43
   |
10 |                 group.start_soon(read_it, file)
   |                                           ^
  = related src/main.au:3:13: task parameter `file` is declared here
  = help: send owned data made only from Transfer components; keep capabilities and host resources on their owning worker
```

The fix is to send the task what it needs to open the resource itself, such as
the path, or to read the data first and send the text. Other things that can't
cross include `random.Rng`, a `TaskGroup`, and views. You can't declare a type
`Transfer` yourself; it is always derived, so there's no way to claim a type is
safe when it isn't.

Two kinds of handle are the exception that makes communication possible. A
`Queue` handle and a `Task` handle can be passed to any task, because their
internal state is synchronized by the runtime. Everything else moves as owned,
unshared data. That's the whole model: tasks share nothing except the channels
between them.

### A result can be observed once

One consequence of moving values is that a task's result can be taken out only
once when it's an owned value. `count_words` returns an `int64`, a copy value,
so we could ask for it repeatedly. A task that returns a `str` is different:

```aura check-fail:AU3001
def shout(text: str) -> str:
    return text.upper()

def main():
    with group = TaskGroup():
        task = group.start(shout, "hello")
        print(task.poll(timeout=1s))
        print(task.poll(timeout=1s))
```

```console
$ aura run src/main.au
error[AU3001]: use of moved value `task`
 --> src/main.au:8:15
   |
8 |         print(task.poll(timeout=1s))
   |               ^
  = related src/main.au:7:15: value moved here
  = help: pass shared access when ownership is not needed, or transfer this non-cloneable value only once
```

`poll` is the short form of `result`: it gives back `Poll.Ready(value)` or
`Poll.Unavailable`, folding failure, timeout, and cancellation into one case.
The first `poll` consumed the handle. That holds even if the first attempt had
timed out: the right to observe the result is used up by the attempt, not by
success. If you need to retry a wait, use a copy value as the result, or have the
task send its result through a queue instead.

So far, each task gets all its input when it starts and hands back one value
when it ends. That's enough for splitting up a fixed batch of work. For work that
arrives over time, or for tasks that talk to each other while they run, we need
queues.
