# Reading the arguments and the file

As always, we start with a new package:

```console
$ aura new logscan
created `logscan`
$ cd logscan
$ aura run src/main.au
Hello from Aura
```

`aura new` gave us `Aura.toml`, a `src/main.au` that prints a greeting, and a
`tests/` directory we'll come back to in the last section. Copy `app.log` from
the chapter introduction into the `logscan` directory, beside `Aura.toml`.

The first job of any command-line tool is to find out what the user asked for.
`logscan` will take the path of the log file as its first argument, so the
user can type `logscan app.log`. Later we'll add an option or two.

## Reading the command-line arguments

Aura's `sys` module holds the things a program asks of its host: arguments,
environment variables, the current directory, and clocks. `sys.args()` returns
the arguments as a `list[str]`. Let's print that list and see what arrives.
Replace the contents of `src/main.au` with Listing 12-1.

*Filename: src/main.au*

```aura
import sys

def main() -> int32:
    args = sys.args()
    print(args)
    return 0
```

<span class="caption">Listing 12-1: Printing the arguments the program was given</span>

Run it once with no arguments and once with a few:

```console
$ aura run src/main.au
[]
$ aura run src/main.au -- app.log --min-level warn
[app.log, --min-level, warn]
```

Two details are worth noticing. First, the `--` matters. Everything before it
is an argument to `aura` itself, and everything after it goes to our program.
If you leave it out and type `aura run src/main.au app.log`, `aura` doesn't
know what to do with `app.log`, prints its own usage text, and exits with
status 2. Second, the list starts with `app.log`. Python's `sys.argv[0]` is the
script name and C's `argv[0]` is the executable, but `sys.args()` leaves the
program name out, so index 0 is the first thing the user typed. That holds for
a native binary too: once we build `logscan` with `aura build`, running
`./logscan app.log` gives the same list.

We declared `main` as returning `int32` rather than nothing. The value `main`
returns becomes the process's exit status, and a command-line tool should
report failure through its exit status so that scripts and shells can tell a
successful run from a broken one. For now we always return 0.

## Taking the path out of the list

We want the first argument in a binding of its own. Coming from Python, the
obvious line is `path = args[0]`:

*Filename: src/main.au*

```aura check-fail:AU3005
import sys

def main() -> int32:
    args = sys.args()
    path = args[0]
    print(f"reading {path}")
    return 0
```

<span class="caption">Listing 12-2: Trying to copy the first argument out of the list</span>

The compiler refuses:

```console
$ aura run src/main.au -- app.log
error[AU3005]: cannot implicitly copy `str` out of a list index; use `view value = items[index]` for shared access, `items[index].clone()` for an explicit cloned owner, or `pop(index)` or `set(index, value)` to transfer ownership
 --> src/main.au:5:12
  |
5 |     path = args[0]
  |            ^
```

This is the ownership rule from [Chapter 4](/book/ch04-00-understanding-ownership)
showing up in a new place. A `str` is an owned value: exactly one binding owns
its bytes and frees them when it goes out of scope. The list `args` owns every
string in it. If `path = args[0]` were allowed to quietly make `path` a second
owner of the same string, the list and `path` would both try to free it.
Python dodges the question because its garbage collector frees the string only
when nothing refers to it any more. Aura has no garbage collector, so it asks
us to say which of the three things we meant, and the message lists them:

- `view path = args[0]` reads the element in place. No copy, no transfer; the
  list still owns the string, and `path` is a window onto it.
- `path = args[0].clone()` makes an independent copy that `path` owns.
- `path = args.pop(0)` moves the string out of the list, which then no longer
  holds it. That needs `mut args`, since it changes the list.

We only want to read the path, and the list will outlive every use of it, so a
view is the right fit. It costs nothing and keeps the ownership where it was.
Change the line to:

```aura fragment
    view path = args[0]
```

Now the program runs:

```console
$ aura run src/main.au -- app.log
reading app.log
```

## When the argument is missing

Now let's see what happens when the user forgets the file name:

```console
$ aura run src/main.au
error[AU4003]: list index `0` is out of bounds for length `0`
 --> src/main.au:5:22
  |
5 |     view path = args[0]
  |                      ^
  = note: Aura call chain (innermost first): main at 3:1
```

This time the error comes from the running program, not the compiler. An index
past the end of a list is a *trap*: the task stops at once, the runtime prints
where it happened, and the process exits with status 1. Chapter 9 explained
when a trap is the right response, and a user leaving off an argument is not
one of those times. A trap tells a programmer that the code is wrong. Here the
code is fine and the input is incomplete, so we should check for it and print
something the user can act on.

The check is a length test before the index:

```aura fragment
    if args.len() < 1:
        print("usage: logscan <path>")
        return 2
```

By convention, status 2 means the command was used incorrectly, as opposed to
status 1 for a run that started and then failed. Many Unix tools follow that
split, and it lets a calling script distinguish "you typed it wrong" from "the
file was bad".

## Reading the file

With a path in hand, we can read the file. The `fs` module has a one-shot
helper for this, `fs.read_to_string(path)`, which reads a whole UTF-8 file into
a `str`. Reading a file can fail in several ways (the file might not exist, we
might not have permission, the path might be a directory), so the function
doesn't return a `str`. It returns `Result[str, io.Error]`. In Chapter 9 we
used `try` to unwrap a `Result` and pass any error up to the caller. Let's try
that in `main`:

*Filename: src/main.au*

```aura check-fail:AU2999
import fs
import sys

def main() -> int32:
    args = sys.args()
    if args.len() < 1:
        print("usage: logscan <path>")
        return 2
    view path = args[0]
    text = try fs.read_to_string(path)
    print(f"read {text.len()} characters from {path}")
    return 0
```

<span class="caption">Listing 12-3: Using `try` in a function that does not return `Result`</span>

```console
$ aura run src/main.au -- app.log
error[AU2999]: `try` requires the enclosing function to return `Result`, found `int32`
 --> src/main.au:10:12
   |
10 |     text = try fs.read_to_string(path)
   |            ^
```

`try` means "if this is an error, return it from the current function". That
only makes sense when the current function returns a `Result` with room for the
error. `main` returns an `int32` exit status, and Aura only accepts `int32` or
nothing as `main`'s result, so there is nowhere for an `io.Error` to go.

The usual shape for a command-line program, in Aura as in many languages, is to
put the real work in a function that returns `Result`, and keep `main` as a
thin layer that turns the outcome into output and an exit status. Let's call
that function `run`. Inside it, `try` works the way we want, and `main` matches
on whatever `run` returns.

*Filename: src/main.au*

```aura
import fs
import io
import sys

def run(path: str) -> Result[None, io.Error]:
    text = try fs.read_to_string(path)
    lines = text.split("\n")
    print(f"{path}: {lines.len()} lines")
    return Ok(None)

def main() -> int32:
    args = sys.args()
    if args.len() < 1:
        print("usage: logscan <path>")
        return 2
    view path = args[0]
    match run(path):
        case Ok(_):
            return 0
        case Err(error):
            print(f"logscan: cannot read {path}: {error}")
            return 1
```

<span class="caption">Listing 12-4: Moving the work into `run`, which returns a `Result`</span>

`run` takes `path` as a bare parameter, which is shared access: it reads the
string and gives nothing back, so the view in `main` is all it needs. Its
result is `Result[None, io.Error]`, because on success it has nothing to hand
back beyond the printed output, and on failure the error is whatever
`fs.read_to_string` reported. We import `io` only to name that error type.

You'll also notice that Listing 12-4 writes `Ok(None)` and `case Err(error)`
without the `Result.` prefix we've used so far. Aura accepts the bare variant
names wherever the compiler already knows a `Result` is expected: in a
`return` from a function declared to return one, or in a `case` of a `match`
on one. The qualified form is still the reference style, and the
[Manual](/manual/enums-and-match) spells out where the short form is allowed.
In this chapter's longer listings the short form keeps the error handling from
crowding out the logic, so we'll use it from here on.

Let's run it, and compare with what `wc` says about the same file:

```console
$ aura run src/main.au -- app.log
app.log: 13 lines
$ wc -l app.log
12 app.log
```

We're off by one. The file has twelve lines, each ending in a newline
character, including the last. `split("\n")` cuts the text at every newline, so
it also returns the empty string that comes after the final one. This is the
same thing Python's `text.split("\n")` does, and it is a classic source of an
extra blank "record" at the end of a parsed file.

Strings have a method made for this job. `text.lines()` splits at each `\n`,
drops a `\r` before it so Windows line endings come out clean, and does not
produce an empty line after a final terminator. Replace the split:

```aura fragment
    lines = text.lines()
```

```console
$ aura run src/main.au -- app.log
app.log: 12 lines
```

## Trying the failure paths

A program that reads files should be tried against files that aren't there.
Here are the three ways we can misuse `logscan` so far:

```console
$ aura run src/main.au -- missing.log
logscan: cannot read missing.log: io.Error.NotFound
$ echo $?
1
$ aura run src/main.au -- src
logscan: cannot read src: io.Error.IsDirectory
$ aura run src/main.au
usage: logscan <path>
$ echo $?
2
```

Each failure gets its own exit status and a line that names the file. The
error text itself is still the raw enum value, `io.Error.NotFound`. That is
accurate, and a programmer reads it without trouble, but someone who only wants
their report deserves "no such file". We'll fix that in the last section of
the chapter, along with the fact that these messages go to standard output.
(Aura 0.11 has no call for writing plain text to standard error; the `log`
module writes there, but only as JSON records. The exit status is what tells a
script that the run failed.)

Notice what we did not have to do. We never checked whether `fs.read_to_string`
succeeded before using `text`, because `try` made that impossible to forget:
inside `run`, `text` is a plain `str` only on the path where the read worked.
And `main` could not ignore the error either, since a `match` on `Result` has
to handle `Err` to be exhaustive.

`run` now has the whole log in hand as a list of lines. Next we'll give those
lines some structure.
