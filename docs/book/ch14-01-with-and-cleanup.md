# `with` and deterministic cleanup

Let's start with a small, ordinary job: count the lines in a text file. Make a
new package with `aura new line_count`, put a `notes.txt` with three lines next
to `Aura.toml`, and replace `src/main.au` with this:

*Filename: src/main.au*

```aura
import fs
import io

def line_count(path: str) -> Result[int64, io.Error]:
    with file = try fs.open(path):
        text = try file.read_all()
        return Result.Ok(len(text.split("\n")) - 1)

def report(path: str):
    match line_count(path):
        case Result.Ok(count):
            print(f"{path}: {count} lines")
        case Result.Err(error):
            print(f"{path}: {error}")

def main():
    report("notes.txt")
    report("missing.txt")
```

<span class="caption">Listing 14-1: Reading a file inside a `with` block</span>

```console
$ aura run src/main.au
notes.txt: 3 lines
missing.txt: io.Error.NotFound
```

If you have written Python, the `with` line reads the way you expect. `fs.open`
opens the file, the block uses it, and when the block ends the file is closed.
There are two small differences in spelling. Aura writes the binding first,
`with file = ...`, although `with ... as file` also works and means the same
thing. And `fs.open` returns a `Result`, because opening a file can fail, so we
put `try` in front of it. When the file is missing, `try` returns the error from
`line_count` before the block is ever entered, which is the second line of
output.

The interesting part is the line that is not there. Nowhere do we call
`file.close()`. The `return` on the last line of the block leaves `line_count`
while the file is open, and Aura closes it on the way out. The same happens if
`file.read_all()` fails and `try` propagates the error: the file is closed
before the error reaches `report`.

## Forgetting `try`

It is easy to forget that `fs.open` gives back a `Result` and not a file. Let's
see what happens if we leave out the `try`:

```aura check-fail:AU2017
import fs
import io

def line_count(path: str) -> Result[int64, io.Error]:
    with file = fs.open(path):
        text = try file.read_all()
        return Result.Ok(len(text.split("\n")) - 1)

def main():
    print(line_count("notes.txt"))
```

```console
$ aura run src/main.au
error[AU2017]: `with` requires a context manager, found `Result[fs.File, io.Error]`
 --> src/main.au:5:5
  |
5 |     with file = fs.open(path):
  |     ^
```

The compiler points at the `with` and tells us what it found:
`Result[fs.File, io.Error]`. A `Result` has no cleanup of its own, so it can't
manage a scope. `AU2017` is the code Aura uses for everything that goes wrong
with the context manager protocol, and we'll meet it again in the next section
when we write our own managers. The fix is the one the type suggests: unwrap the
`Result` with `try` (or `match` on it) so that the `with` gets the `fs.File`
inside.

## Every way out runs the exit

Files don't print anything when they close, so to watch cleanup happen we need a
resource that talks. Here is a tiny class, `Section`, that prints a line when a
block starts and another when it ends. We'll look at how `__enter__` and
`__exit__` work in detail in the next section. For now, read them as "run this
at the start" and "run this at the end".

*Filename: src/main.au*

```aura
class Section:
    name: str

    def __enter__(mut self) -> None:
        print("  enter " + self.name)

    def __exit__(mut self) -> None:
        print("  exit " + self.name)

def main():
    for n in range(4):
        with section = Section(name=f"round {n}"):
            if n == 1:
                print("  skipping")
                continue
            if n == 3:
                print("  stopping")
                break
            print("  working")
    print("done")
```

<span class="caption">Listing 14-2: Leaving a `with` block by falling through, `continue`, and `break`</span>

```console
$ aura run src/main.au
  enter round 0
  working
  exit round 0
  enter round 1
  skipping
  exit round 1
  enter round 2
  working
  exit round 2
  enter round 3
  stopping
  exit round 3
done
```

Each round leaves the block in a different way. Rounds 0 and 2 fall off the end.
Round 1 leaves with `continue`, which jumps to the next iteration of the loop
around the block. Round 3 leaves with `break`, which jumps out of the loop
entirely. Every round prints its `exit` line before control goes anywhere else.

That is the core promise of `with`. Once the block has been entered, its exit
runs exactly once, whichever way control leaves the block:

- falling through the end of the block
- `return`
- `break` or `continue` that leaves the block
- `try` propagating an error
- a trap, such as a failed assertion or an index out of range
- cancellation of the task the block is running in

We've seen the first four. The next section covers traps in detail, and
[Chapter 15](/book/ch15-00-concurrency) covers cancellation. They follow the
same rule.

"Exactly once" matters as much as "always". An exit can't run twice because two
paths both tried to clean up, and it can't be skipped because one path forgot.
The compiler plans the cleanup for every exit edge of the block when it compiles
the function, so it is not something you can get wrong by adding a new `return`
later.

## Bindings in the body stay in the body

Python programmers sometimes rely on a detail of Python's `with`: names bound
inside the block are still visible after it. That isn't true in Aura. A `with`
body is a block like the body of an `if` or a `for`, and bindings made inside it
end with it. Suppose we try to move the `return` out of the block in Listing
14-1:

```aura check-fail:AU2001
import fs
import io

def line_count(path: str) -> Result[int64, io.Error]:
    with file = try fs.open(path):
        text = try file.read_all()
    return Result.Ok(len(text.split("\n")) - 1)

def main():
    print(line_count("notes.txt"))
```

```console
$ aura run src/main.au
error[AU2001]: unknown name `text`
 --> src/main.au:7:26
  |
7 |     return Result.Ok(len(text.split("\n")) - 1)
  |                          ^
```

As far as the compiler is concerned, there is no `text` on line 7. It is the same scoping
rule we met in [Chapter 3](/book/ch03-00-common-concepts), and it has a pleasant
consequence here: nothing that was computed while the file was open can be
mistaken for the file itself after it closes. If you need a value after the
block, declare it before the block with `mut` and assign it inside, or return
from inside the block as Listing 14-1 does.

## Several resources in one header

Copying a file needs two open files at once: one to read and one to write. A
`with` header can list several items separated by commas:

*Filename: src/main.au*

```aura
import fs
import io

def copy_file(source: str, target: str) -> Result[None, io.Error]:
    with input = try fs.open(source), output = try fs.create(target):
        text = try input.read_all()
        try output.write_all(text)
    return Result.Ok(None)

def report(source: str, target: str):
    match copy_file(source, target):
        case Result.Ok(_):
            print(f"copied {source} to {target}")
        case Result.Err(error):
            print(f"could not copy {source}: {error}")

def main():
    report("notes.txt", "notes.bak")
    report("missing.txt", "missing.bak")
```

<span class="caption">Listing 14-3: Two resources in one `with` header</span>

```console
$ aura run src/main.au
copied notes.txt to notes.bak
could not copy missing.txt: io.Error.NotFound
$ ls
Aura.lock
Aura.toml
notes.bak
notes.txt
src
tests
```

Items are set up from left to right, and each one is fully entered before the
next is evaluated. That means a later item may use an earlier one's binding,
which is handy when the second resource is derived from the first. It also
explains what `ls` shows: there is no `missing.bak`. When `fs.open("missing.txt")`
failed, `try` left the function before `fs.create` was ever evaluated, so no
empty target file was created.

When the block ends, the items are cleaned up in the opposite order: `output`
first, then `input`. We can see the order with two `Section`s:

```aura
class Section:
    name: str

    def __enter__(mut self) -> None:
        print("enter " + self.name)

    def __exit__(mut self) -> None:
        print("exit " + self.name)

def main():
    with Section(name="outer") as outer, inner = Section(name="inner"):
        print("body")
```

```console
$ aura run src/main.au
enter outer
enter inner
body
exit inner
exit outer
```

This also shows the two spellings side by side: `Section(name="outer") as outer`
and `inner = Section(name="inner")` mean the same thing, and you can mix them in
one header.

Reverse order is the only order that is safe in general. A resource entered
later may depend on one entered earlier: a buffered writer wrapping a file, a
transaction on a connection, a lock that protects a data structure. If the
earlier one were torn down first, the later one's cleanup would run against
something already gone. Closing in reverse means each cleanup runs while
everything it might need is still alive. Writing one header with two items is
the same as nesting one `with` inside another, and nested blocks naturally unwind
from the inside out.

## What Aura closes for you without `with`

You might wonder what happens if we open a file without `with` at all. Aura
doesn't leak it. An `fs.File` is an owned value, and like every owned value
(see [Chapter 4](/book/ch04-00-understanding-ownership)) it is released when its
owner goes out of scope. A file bound with `mut file = try fs.create(path)` in a
function is flushed and closed when that function returns.

So why use `with`? There are three reasons, and the third is the important one.

The first is that `with` puts the end of the resource's life on the page. The
indented block is the region where the file is open, and anyone reading the code
can see it without tracing ownership.

The second is that it keeps the resource from going anywhere else. The `with`
binding is managed by the block, and the compiler won't let you move it out:
writing `return Result.Ok(file)` inside the block of Listing 14-1 is rejected
with ``cannot move managed `with` resource `file` ``. We'll come back to that
error in [Chapter 15](/book/ch15-00-concurrency), where it is the rule that keeps
tasks from outliving their group.

The third is that `with` is how your own code gets to run at cleanup. Aura
classes have no destructor. A class can define `__exit__`, but only a `with`
block calls it. Watch what happens to the same `Section` with and without
`with`:

```aura
class Section:
    name: str

    def __enter__(mut self) -> None:
        print("enter " + self.name)

    def __exit__(mut self) -> None:
        print("exit " + self.name)

def loose():
    section = Section(name="loose")
    print("using " + section.name)

def main():
    loose()
    with section = Section(name="scoped"):
        print("using " + section.name)
    print("done")
```

```console
$ aura run src/main.au
using loose
enter scoped
using scoped
exit scoped
done
```

The loose `Section` is dropped at the end of `loose`: its memory is freed and
its `name` string with it. But no `enter` or `exit` line appears for it, because
dropping a value only releases what it owns. It doesn't run arbitrary code.
The `with` version runs both.

This is a deliberate choice, and Python is a useful contrast. Python has
`__del__`, which runs when an object is collected, but *when* that is depends on
the implementation: right away in CPython when the last reference goes, at some
later collection in PyPy, possibly never for an object caught in a reference
cycle at shutdown. That is why Python style says to use `with` for files even
though CPython would usually close them anyway. Aura removes the ambiguity from
the other side. Memory and the standard library's handles are released when
their owner's scope ends, which is always a known point. Code you write runs at
cleanup only through `with`, which is always visible in the source.

## Resources the standard library gives you

You can use these standard library types in a `with` header, and each knows how
to clean itself up:

- `fs.File`, from `fs.open`, `fs.create`, and `fs.append`
- the `net` socket, listener, and HTTP types
- `process.Child`, `process.Pipe`, and `process.Supervisor`
- `TaskGroup`

For files, sockets, and pipes the cleanup is a close. For a `TaskGroup` it is
something richer: waiting for every task the group started. That is the subject
of [Chapter 15](/book/ch15-00-concurrency). The details for each resource are in
its Manual page, and the full contract of the statement is in the Manual's
[Statements](/manual/statements) chapter.

So far every exit we have watched has succeeded. Real cleanup can fail: deleting
a file that someone else already deleted, flushing to a disk that is full. And
real bodies can trap. In the next section we'll build a resource of our own and
find out exactly what Aura does in each of those cases.
