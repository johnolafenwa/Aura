# Writing your own resource manager

The standard library's resources close themselves, but plenty of cleanup is
specific to your program. A test fixture deletes the directory it set up. A
progress bar erases itself when the work is done. A transaction rolls back if it
was never committed. In Aura, any class can take part in `with` by following a
small protocol, and once it does, it gets the same guarantees as `fs.File`.

We'll build one example through the whole section: a scratch file that a
function writes intermediate results into and that is deleted when the function
is done with it, however it finishes.

## The protocol

A class becomes a context manager by defining one or two methods:

- `__exit__(mut self)`, which runs once when control leaves the block. Instead
  of `__exit__`, a class may define `close(mut self) -> None`, and Aura uses
  that as the exit.
- `__enter__(mut self)`, which is optional and runs after the item expression
  has produced the value and before the body starts.

Both methods take `mut self` and nothing else. Here is the scratch file:

*Filename: src/main.au*

```aura
import fs
import io

class Scratch:
    path: str

    def create(path: own str) -> Result[Scratch, io.Error]:
        try fs.write_string(path, "")
        print("created " + path)
        return Result.Ok(Scratch(path=path))

    def append(self, line: str) -> Result[None, io.Error]:
        return fs.append_string(self.path, line + "\n")

    def read(self) -> Result[str, io.Error]:
        return fs.read_to_string(self.path)

    def __exit__(mut self) -> Result[None, io.Error]:
        try fs.remove_file(self.path)
        print("removed " + self.path)
        return Result.Ok(None)

def summarize(values: list[int64]) -> Result[int64, io.Error]:
    with scratch = try Scratch.create("work.tmp"):
        for value in values:
            try scratch.append(str(value))
        text = try scratch.read()
        print(text)
    return Result.Ok(len(values))

def main():
    match summarize([3, 1, 4]):
        case Result.Ok(count):
            print(f"summarized {count} values")
        case Result.Err(error):
            print(f"error: {error}")
```

<span class="caption">Listing 14-4: A scratch file that deletes itself</span>

```console
$ aura run src/main.au
created work.tmp
3
1
4

removed work.tmp
summarized 3 values
$ ls
Aura.lock
Aura.toml
src
tests
```

The output reads in the order the program runs: the file is created, filled,
read back, and removed before `summarize` returns. Afterwards `ls` shows no
`work.tmp`.

There are a few design choices in `Scratch` worth looking at, because each one
comes from a rule of the protocol.

Creating the file happens in `Scratch.create`, an associated function that
returns `Result[Scratch, io.Error]`, and not in `__enter__`. That is because
`__enter__` can't fail in a recoverable way. It must return `None` (or a view,
which we'll get to at the end of this section), so it has no way to hand an
`io.Error` back to the caller. The pattern that works is to put the fallible
setup in the item expression and use `try` there, exactly as we did with
`try fs.open(path)`. If `create` fails, the `with` never registers an exit,
because there is nothing yet to clean up.

The exit, on the other hand, is allowed to fail, and ours can: `fs.remove_file`
returns a `Result`. So `__exit__` is declared to return `Result[None, io.Error]`
and uses `try` like any other function. We'll see what Aura does with that error
shortly.

`create` takes `path` as `own str`. The new `Scratch` stores the string in its
field, so it has to own it. If you write `path: str`, the compiler stops you at
`Scratch(path=path)` with an `AU3002` and suggests either `own str` or
`.clone()`. Taking ownership is the better fit: the caller passes a literal, and
nobody else needs the string afterwards.

If the methods have the wrong shape, the class isn't a context manager at all.
Here `__exit__` forgot the `mut` on `self`:

```aura check-fail:AU2017
class Scratch:
    path: str

    def __exit__(self) -> None:
        print("removing " + self.path)

def main():
    with scratch = Scratch(path="work.tmp"):
        print("working")
```

```console
$ aura run src/main.au
error[AU2017]: class `Scratch` is not a context manager: `__exit__(mut self)` is missing or has the wrong shape
 --> src/main.au:8:5
  |
8 |     with scratch = Scratch(path="work.tmp"):
  |     ^
```

The error lands on the `with`, not on the method, because the method is
perfectly fine as an ordinary method. It only becomes a problem when we ask the
class to manage a scope. The reason for `mut self` is that cleanup almost always
changes the resource: it closes a handle, marks a transaction finished, empties a
buffer. Requiring `mut` means the exit has the access it needs, and it tells
readers that calling it changes the object.

## When cleanup itself fails

What should happen if the scratch file is already gone when the block ends?
Let's make that happen on purpose, by deleting the file in the body:

```aura
import fs
import io

class Scratch:
    path: str

    def create(path: own str) -> Result[Scratch, io.Error]:
        try fs.write_string(path, "")
        print("created " + path)
        return Result.Ok(Scratch(path=path))

    def __exit__(mut self) -> Result[None, io.Error]:
        try fs.remove_file(self.path)
        print("removed " + self.path)
        return Result.Ok(None)

def tidy() -> Result[None, io.Error]:
    with scratch = try Scratch.create("work.tmp"):
        try fs.remove_file("work.tmp")
        print("someone removed work.tmp early")
    print("after the block")
    return Result.Ok(None)

def main():
    match tidy():
        case Result.Ok(_):
            print("tidy finished")
        case Result.Err(error):
            print(f"tidy failed: {error}")
```

```console
$ aura run src/main.au
created work.tmp
someone removed work.tmp early
tidy failed: io.Error.NotFound
```

The body finished normally, then the exit ran and its `fs.remove_file` failed
with `NotFound`. Aura treated that `Err` the way `try` treats one: it returned
from `tidy` with the error, so `after the block` never printed. An exit that
returns `Result` behaves as if there were an invisible `try` at the end of the
block.

That has a consequence for every function that uses such a manager. The
enclosing function must be able to return the error. If we use `Scratch` in a
`main` that returns nothing, the compiler tells us so:

```aura check-fail:AU2017
import fs
import io

class Scratch:
    path: str

    def __exit__(mut self) -> Result[None, io.Error]:
        try fs.remove_file(self.path)
        return Result.Ok(None)

def main():
    with scratch = Scratch(path="work.tmp"):
        print("working")
```

```console
$ aura run src/main.au
error[AU2017]: exit of `Scratch` may fail with `io.Error`; the enclosing function must return `Result[_, io.Error]`
 --> src/main.au:12:5
   |
12 |     with scratch = Scratch(path="work.tmp"):
   |     ^
```

This is the same reasoning Aura applies to `try` everywhere: an error has to go
somewhere you can see in the signature. Python's `__exit__` can raise, and the
exception then flies out of the `with` statement with nothing in the function's
signature to warn the caller. In Aura, a fallible cleanup shows up as a
`Result` return type on every function that uses it. The error type can also be
one that has a `From` conversion from the exit's error, as described in
[Chapter 9](/book/ch09-00-error-handling).

If you don't want callers to deal with cleanup errors, decide inside the exit:
log the problem and return `None` from a `__exit__(mut self) -> None`. That is a
legitimate choice for best-effort cleanup like deleting a temporary file. The
point is that the choice is written down in one place.

### When the body is already failing

What if the body fails and then the cleanup fails too? Only one error can come
out of the block, and Aura picks the body's. Here both fail:

```aura
class Flush:
    name: str

    def __exit__(mut self) -> Result[None, str]:
        print("exit " + self.name)
        return Result.Err("exit of " + self.name + " failed")

def step() -> Result[int64, str]:
    return Result.Err("body failed")

def run() -> Result[int64, str]:
    with flush = Flush(name="flush"):
        value = try step()
        print(value)
    return Result.Ok(0)

def main():
    print(run())
```

```console
$ aura run src/main.au
exit flush
Result.Err(body failed)
```

The exit still ran (it printed `exit flush`), but its error was dropped and
`body failed` came out. That is the right priority. The body's error describes
what actually went wrong. The cleanup failure is often a consequence of it, a
half-written file that couldn't be flushed because the write that filled it
failed. Reporting the cleanup error instead would send you looking in the wrong
place.

## Traps unwind through every `with`

A `Result` is an error the program expects and handles. A trap is different: an
index out of range, a failed assertion, an overflow. As
[Chapter 9](/book/ch09-00-error-handling) explains, a trap stops the task. But
"stops" does not mean "abandons its resources". Let's add a line to
`summarize` that traps when the list is empty, and call it with an empty list:

*Filename: src/main.au*

```aura
import fs
import io

class Scratch:
    path: str

    def create(path: own str) -> Result[Scratch, io.Error]:
        try fs.write_string(path, "")
        print("created " + path)
        return Result.Ok(Scratch(path=path))

    def append(self, line: str) -> Result[None, io.Error]:
        return fs.append_string(self.path, line + "\n")

    def read(self) -> Result[str, io.Error]:
        return fs.read_to_string(self.path)

    def __exit__(mut self) -> Result[None, io.Error]:
        try fs.remove_file(self.path)
        print("removed " + self.path)
        return Result.Ok(None)

def summarize(values: list[int64]) -> Result[int64, io.Error]:
    with scratch = try Scratch.create("work.tmp"):
        print(f"first value: {values[0]}")
        for value in values:
            try scratch.append(str(value))
        text = try scratch.read()
        print(text)
    return Result.Ok(len(values))

def main():
    match summarize([3, 1, 4]):
        case Result.Ok(count):
            print(f"summarized {count} values")
        case Result.Err(error):
            print(f"error: {error}")
    match summarize([]):
        case Result.Ok(count):
            print(f"summarized {count} values")
        case Result.Err(error):
            print(f"error: {error}")
```

<span class="caption">Listing 14-5: A trap inside the body of a `with`</span>

```console
$ aura run src/main.au
created work.tmp
first value: 3
3
1
4

removed work.tmp
summarized 3 values
created work.tmp
removed work.tmp
error[AU4003]: list index `0` is out of bounds for length `0`
 --> src/main.au:25:38
   |
25 |         print(f"first value: {values[0]}")
   |                                      ^
  = note: Aura call chain (innermost first): summarize at 23:1 -> main at 32:1
$ ls
Aura.lock
Aura.toml
src
tests
```

The first call behaves as before. The second creates the scratch file and then
traps on `values[0]`. Look at the order of the last lines: `removed work.tmp`
comes *before* the diagnostic. When the trap happened, Aura did not stop on the
spot. It walked back out of the block, ran the scratch file's exit, and only
then reported the trap and ended the program with a failure status. The `ls`
confirms that no `work.tmp` was left behind.

The diagnostic itself is the ordinary trap report. `AU4003` is the
out-of-bounds code, the arrow points at the `0` in `values[0]`, and the note
lists the call chain from the innermost frame outwards.

### Cleanup order during a trap

A trap doesn't only unwind the block it happens in. It unwinds every block that
is active, in this function and in all the functions that called it, innermost
first. Here is a program with `with` blocks at two levels, and a manager whose
exit fails:

*Filename: src/main.au*

```aura
class Section:
    name: str

    def __enter__(mut self) -> None:
        print("enter " + self.name)

    def __exit__(mut self) -> None:
        print("exit " + self.name)

class Journal:
    entries: list[str]

    def __exit__(mut self) -> Result[None, str]:
        if len(self.entries) == 0:
            return Result.Err("journal is empty")
        print(f"flushed {len(self.entries)} entries")
        return Result.Ok(None)

def lookup(values: list[int64], index: int64) -> Result[int64, str]:
    with journal = Journal(entries=[]), section = Section(name="lookup"):
        return Result.Ok(values[index])

def main():
    with section = Section(name="main"):
        print(lookup([1, 2, 3], 1))
        print(lookup([1, 2, 3], 9))
```

<span class="caption">Listing 14-6: A trap unwinding through two functions</span>

```console
$ aura run src/main.au
enter main
enter lookup
exit lookup
Result.Err(journal is empty)
enter lookup
exit lookup
exit main
error[AU4003]: list index `9` is out of bounds for length `3`
 --> src/main.au:21:33
   |
21 |         return Result.Ok(values[index])
   |                                 ^
  = note: `Journal.__exit__` also failed during cleanup: journal is empty
  = note: Aura call chain (innermost first): lookup at 19:1 -> main at 23:1
```

Let's read this from the top. The first call to `lookup` succeeds in its body,
but the `Journal` is empty, so its exit returns an `Err`. The `Section` exit runs
first (it was entered second), and then the journal's `Err` replaces the
function's result, which is why `main` prints `Result.Err(journal is empty)`
instead of `Result.Ok(2)`. That's the rule from the previous part: on a normal
exit, an exit's error is returned like `try`.

The second call traps on `values[9]`. Now three exits are active: the
`Section` and the `Journal` in `lookup`, and the `Section` in `main`. All three
run, in reverse order of entry: `exit lookup`, then the journal's exit (which
prints nothing because it fails), then `exit main`. Only after the last one does
the diagnostic appear.

Look at the first note. The journal's exit failed during the unwind, but its
error didn't replace the trap, and it wasn't thrown away silently either. Aura
attached it to the trap report as a note. The trap stays the primary
diagnostic because it is the reason the program stopped. The cleanup failure is
extra information you may need while debugging it.

### An exit that traps doesn't stop the others

An exit is ordinary code, so it can trap too. What happens to the exits that
haven't run yet? Here three managers are entered and the middle one's exit
fails an assertion:

```aura
class Step:
    name: str
    fail: bool

    def __enter__(mut self) -> None:
        print("enter " + self.name)

    def __exit__(mut self) -> None:
        print("exit " + self.name)
        assert not self.fail, "cleanup of " + self.name + " failed"

def main():
    with a = Step(name="a", fail=false), b = Step(name="b", fail=true), c = Step(name="c", fail=false):
        print("body")
```

<span class="caption">Listing 14-7: A trap inside an exit</span>

```console
$ aura run src/main.au
enter a
enter b
enter c
body
exit c
exit b
exit a
error[AU4001]: cleanup of b failed
 --> src/main.au:10:9
   |
10 |         assert not self.fail, "cleanup of " + self.name + " failed"
   |         ^
  = note: Aura call chain (innermost first): Step.__exit__ at 8:5 -> main at 12:1
```

`exit b` traps, and `exit a` still runs afterwards. This is the rule that makes
the whole system dependable: *every remaining `with` exit runs after one traps.*
Each exit that was registered before the failing one still runs, in this frame
and in the callers' frames. If a failing cleanup could cancel the cleanups after
it, one buggy manager deep in a library could leave your files open and your
temporary directories on disk. Aura isolates each exit from the others.

When more than one thing traps, Aura has to choose which diagnostic to report.
If the body was already failing, the body's diagnostic stays primary, as in
Listing 14-5. If the body finished cleanly, as here, the first exit to trap is
the one reported. You can see that in the call chain: the innermost frame is
`Step.__exit__`.

### Failing while entering

There is one more place a failure can happen: partway through a header with
several items. Suppose the second item's `__enter__` traps:

```aura
class Step:
    name: str
    fail: bool

    def __enter__(mut self) -> None:
        assert not self.fail, "could not start " + self.name
        print("enter " + self.name)

    def __exit__(mut self) -> None:
        print("exit " + self.name)

def main():
    with a = Step(name="a", fail=false), b = Step(name="b", fail=true), c = Step(name="c", fail=false):
        print("body")
```

```console
$ aura run src/main.au
enter a
exit a
error[AU4001]: could not start b
 --> src/main.au:6:9
  |
6 |         assert not self.fail, "could not start " + self.name
  |         ^
  = note: Aura call chain (innermost first): Step.__enter__ at 5:5 -> main at 12:1
```

`a` was fully entered, so its exit runs. `b` failed while entering, so its exit
does *not* run, and `c` was never constructed at all. An exit is registered only
after the item's expression and its `__enter__` have both succeeded. That's
the right line to draw: cleanup code is allowed to assume that setup finished.
If `b`'s exit ran after a failed enter, it would have to defend against a
half-initialized object, which is exactly the kind of code that has bugs.

## Things `__exit__` can't do

If you're coming from Python, you'll notice that Aura's `__exit__` takes no
arguments besides `self`. Python passes `exc_type`, `exc_value`, and
`traceback`, and an `__exit__` that returns `True` swallows the exception. In
Aura, the exit can't see why the block ended and can't stop a failure from
continuing.

That's a real limitation. You can't write a manager that rolls back on failure
and commits on success purely by looking at how the block ended. The usual
workaround is to make the success explicit: give the manager a `commit(mut
self)` method that the body calls as its last step, and have `__exit__` roll
back if `commit` was never called. That's arguably clearer anyway, because the
commit point is visible in the body.

The other half of the limitation is deliberate. Because no exit can suppress a
trap, a trap always reaches the top of the task. A library's context manager
can't hide your bug from you, and reading a `with` block never requires you to
check whether the manager might silently swallow failures.

## Entering with a view

So far the `with` name has bound the manager itself. A manager can instead hand
the body a view into part of itself, by returning `view` or `view mut` from
`__enter__`. This is how you write a guard: the body gets access to the
protected data, and the manager stays out of reach until the exit.

```aura
class Lock:
    name: str
    values: list[int64]

    def __enter__(mut self) -> view mut list[int64] from self:
        print("lock " + self.name)
        return view mut self.values

    def __exit__(mut self) -> None:
        print(f"unlock {self.name} with {len(self.values)} values")

def main():
    with Lock(name="counter", values=[]) as values:
        values.append(1)
        values.append(2)
        print(f"inside: {len(values)}")
```

<span class="caption">Listing 14-8: An entry view</span>

```console
$ aura run src/main.au
lock counter
inside: 2
unlock counter with 2 values
```

Inside the block, `values` is a mutable view of the lock's list. The `Lock`
itself is held in a hidden binding that the body can't name, so the body can't
call methods on it or move it while the view is live. The view ends before the
exit runs, which is why `__exit__` can read `self.values` freely.

Two rules keep this sound, and you'll run into both. The first is that a `with`
item consumes its expression. If we bind the lock first and then manage it,
the lock is gone afterwards:

```aura check-fail:AU3001
class Lock:
    name: str
    values: list[int64]

    def __enter__(mut self) -> view mut list[int64] from self:
        print("lock " + self.name)
        return view mut self.values

    def __exit__(mut self) -> None:
        print(f"unlock {self.name} with {len(self.values)} values")

def main():
    lock = Lock(name="counter", values=[])
    with lock as values:
        values.append(1)
    print(len(lock.values))
```

```console
$ aura run src/main.au
error[AU3001]: use of moved value `lock`
 --> src/main.au:16:15
   |
16 |     print(len(lock.values))
   |               ^
  = related src/main.au:14:10: value moved here
  = help: pass shared access when ownership is not needed, or transfer this non-cloneable value only once
```

This is the ordinary moved-value error from
[Chapter 4](/book/ch04-00-understanding-ownership). The `with` took ownership
of `lock` on line 14, ran its exit at the end of the block, and dropped it. A
managed resource belongs to its block; that is how the block can promise to
clean it up.

The second rule is that the view can't escape the block. Storing it somewhere
that outlives the block is rejected:

```aura check-fail:AU3010
class Lock:
    name: str
    values: list[int64]

    def __enter__(mut self) -> view mut list[int64] from self:
        print("lock " + self.name)
        return view mut self.values

    def __exit__(mut self) -> None:
        print(f"unlock {self.name} with {len(self.values)} values")

class Cache:
    items: list[int64]

def main():
    mut cache = Cache(items=[])
    with Lock(name="counter", values=[]) as values:
        values.append(1)
        cache.items = values
```

```console
$ aura run src/main.au
error[AU3010]: a view cannot be stored as a class field
 --> src/main.au:19:23
   |
19 |         cache.items = values
   |                       ^
  = help: keep it in a matching `view` binding, or produce an explicit owned value
```

If this were allowed, `cache.items` would point into the lock's list after the
lock had been unlocked and dropped. `AU3010` is the code for a view that would
outlive what it borrows from. If you need the data afterwards, make an owned
copy inside the block with `values.copy()`.

## The one case where exits don't run

Everything in this section is about Aura control flow leaving a block: a
return, a `try`, a trap, or a cancellation. There is one situation outside
that, and you should know about it before the next chapter. If the program's
`main` finishes while some task is still suspended in the middle of its work,
the runtime abandons that task. An abandoned task's `with` exits do not run.

That sounds alarming, but it can only happen to a task that escaped structured
concurrency, and the next chapter shows how `TaskGroup` keeps every task inside
a `with` block that waits for it. Treat abandonment as a last-resort cleanup of
the runtime's own state, never as a way your cleanup code gets called. The
Manual's [Execution Model](/manual/execution-model) has the precise rules.

## Summary

A `with` block ties a resource to a region of code. Items enter left to right,
exits run right to left, and each registered exit runs exactly once on every way
out: falling through, `return`, `break` and `continue`, `try`, traps, and
cancellation. An exit that returns `Result` acts like a `try` at the end of the
block on normal paths. When the body is already failing, the body's error or
trap stays primary, and a cleanup error becomes a note on the trap rather than
replacing it. A trap inside one exit doesn't stop the exits registered before
it, so one bad cleanup can't leak every other resource.

You can now write your own managers with `__enter__` and `__exit__` (or
`close`), keep fallible setup in the item expression where `try` can reach it,
and hand the body a view when the manager should guard its data. You also know
the edges of the design: an exit can't inspect or suppress a failure, and a
managed value belongs to its block.

The next chapter puts all of this to work. A `TaskGroup` is a resource whose
exit waits for every task it started, and the cleanup rules from this chapter
are what let Aura promise that no task outlives the block that created it.
