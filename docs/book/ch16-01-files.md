# Files and the filesystem

Let's start `checkup` the way most tools start: by reading a file somebody
else wrote. Create the package:

```console
$ aura new checkup
created `checkup`
$ cd checkup
```

Our checks will live in a plain text file, one check per line. Each line has a
name, a kind, and a target. For now the only kind that does anything is
`file`, which asks whether a path exists. Lines that start with `#` are
comments. Save this as *checks.txt* in the package root, next to *Aura.toml*:

*Filename: checks.txt*

```text
# name    kind  target
config    file  checks.txt
notes     file  notes.txt
```

## Reading a whole file

The quickest way to get a file into memory is `fs.read_to_string`. It opens
the file, reads all of it, decodes it as UTF-8, and closes it again. Replace
*src/main.au* with this:

*Filename: src/main.au*

```aura
import fs

def main() -> int32:
    match fs.read_to_string("checks.txt"):
        case Result.Ok(text):
            for line in text.lines():
                print(line)
            return 0
        case Result.Err(error):
            print(f"cannot read checks.txt: {error}")
            return 1
```

<span class="caption">Listing 16-1: Reading the checks file and echoing each line</span>

Run it from the package root, because the path `"checks.txt"` is relative to
the directory you start the program in:

```console
$ aura run src/main.au
# name    kind  target
config    file  checks.txt
notes     file  notes.txt
```

Now rename *checks.txt* to something else and run it again:

```console
$ aura run src/main.au
cannot read checks.txt: io.Error.NotFound
```

`fs.read_to_string` returns `Result[str, io.Error]`, and here we got the
`Err` side. `io.Error` is the error enum shared by `io`, `fs`, and `net`. Its
variants name the conditions the operating system reports, such as
`NotFound`, `PermissionDenied`, `IsDirectory`, and `InvalidData` (which is
what you get if the file is not valid UTF-8). Because the failure is a value
with a type, the compiler would not let us use `text` without first deciding
what happens when there is no text. In Python, `open("checks.txt")` raises
`FileNotFoundError` and the question of who handles it is left to whoever
happens to be up the call stack. In Aura the question is answered at the
call site, in plain sight.

When the program has a different policy for one variant, match it directly.
A tool that falls back to built-in defaults when its config is absent, but
still reports every other failure, would write:

```aura fragment
match fs.read_to_string("checks.txt"):
    case Result.Ok(text):
        print(text)
    case Result.Err(io.Error.NotFound):
        print("no checks.txt, using defaults")
    case Result.Err(error):
        print(error)
```

Keep the `io.Error` value as long as you can rather than turning it into a
string early. The variant is information your control flow can use; a string
is only information a person can read.

Whole-file reads are capped at 256 MiB. A read over the cap fails with
`io.Error.InvalidData` instead of quietly eating all your memory. Aura 0.11 has
no chunked read API, so a tool that must process larger files has to split
them first. For configuration files and reports, the cap is far away.

## Turning lines into checks

Echoing lines is not much of a tool. Let's give the checks a type. A check has
a name and a probe, and a probe is one of three kinds, each with its own data.
That is an enum with payloads, which we met in
[Chapter 6](/book/ch06-00-enums-and-match):

```aura
enum Probe:
    File(path: str)
    Command(argv: list[str])
    Http(url: str)

class Check:
    name: str
    probe: Probe
```

We are declaring all three kinds now, even though only `File` will do
anything in this section. The next two sections fill in `Command` and `Http`,
and having the shape settled early means the parser will not change.

Loading the file can fail in two different ways: the file cannot be read, or
one of its lines makes no sense. Those are different problems with different
information attached, so they get different variants of our own error type:

```aura fragment
enum ConfigError:
    Unreadable(path: str, error: io.Error)
    BadLine(number: int64, message: str)
```

`Unreadable` keeps the original `io.Error` instead of a description of it, for
the reason given above. `BadLine` carries the line number, because "unknown
kind" is much less useful than "line 7: unknown kind".

Here is a first attempt at the parser and the loader, with a `main` that
prints what it found. Read through it before running it; there is a problem
the compiler will catch.

*Filename: src/main.au*

```aura check-fail:AU3002
import fs
import io

enum Probe:
    File(path: str)
    Command(argv: list[str])
    Http(url: str)

class Check:
    name: str
    probe: Probe

enum ConfigError:
    Unreadable(path: str, error: io.Error)
    BadLine(number: int64, message: str)

def parse_line(line: str) -> Result[Check, str]:
    mut parts = line.split_whitespace()
    if parts.len() < 3:
        return Result.Err("expected `name kind target`")
    name = parts.pop(0)
    kind = parts.pop(0)
    if kind == "file":
        return Result.Ok(Check(name=name, probe=Probe.File(path=parts.pop(0))))
    if kind == "http":
        return Result.Ok(Check(name=name, probe=Probe.Http(url=parts.pop(0))))
    if kind == "cmd":
        return Result.Ok(Check(name=name, probe=Probe.Command(argv=parts)))
    return Result.Err(f"unknown kind `{kind}`")

def load_checks(path: str) -> Result[list[Check], ConfigError]:
    mut text = ""
    match fs.read_to_string(path):
        case Result.Ok(contents):
            text = contents
        case Result.Err(error):
            return Result.Err(ConfigError.Unreadable(path=path.clone(), error=error))
    mut checks: list[Check] = []
    mut number: int64 = 0
    for line in text.lines():
        number += 1
        trimmed = line.trim()
        if trimmed == "" or trimmed.starts_with("#"):
            continue
        match parse_line(trimmed):
            case Result.Ok(check):
                checks.append(check)
            case Result.Err(message):
                return Result.Err(ConfigError.BadLine(number=number, message=message))
    return Result.Ok(checks)

def main() -> int32:
    match load_checks("checks.txt"):
        case Result.Ok(checks):
            for check in checks:
                print(f"{check.name}: {check.probe}")
            return 0
        case Result.Err(error):
            print(error)
            return 2
```

<span class="caption">Listing 16-2: Parsing lines into `Check` values (does not compile yet)</span>

`parse_line` uses `split_whitespace`, which splits on runs of spaces and tabs,
so the columns in *checks.txt* can be lined up however you like. It takes the
pieces out of the list with `pop(0)` rather than reading `parts[0]`, because
indexing a list of strings gives a shared view of an element and we want to
own the name. For a `cmd` line, whatever is left after the name and kind is
the command and its arguments, so the rest of `parts` becomes the `argv`.

Now the compiler's opinion:

```console
$ aura run src/main.au
error[AU3002]: cannot move `contents` out of a shared match on `fs.read_to_string(...)`
 --> src/main.au:35:20
   |
35 |             text = contents
   |                    ^
  = related src/main.au:34:24: value is borrowed here
  = help: write `match own fs.read_to_string(...)` to consume the scrutinee, or call `.clone()` to consume an independent copy
  = fix: replace src/main.au:35:28-35:28 with `.clone()`
```

Line 35 tries to move the file's contents into `text`, and the compiler says
the match only borrowed them. A plain `match` gives its arms shared access to
the value being matched, the same default a bare function parameter gets.
That is usually what you want: most matches only look at a value. Here,
though, we want to take the string out of the `Result` and keep it after the
match ends, which is a move, and you cannot move out of something you are
only borrowing.

The help line offers two fixes. Calling `.clone()` would copy the whole file
contents to get an owned string, which works but wastes the copy. The better
fix is `match own`, which tells the compiler that the match consumes the
`Result`. Nothing else needs the `Result` after the match, so consuming it
costs nothing and hands each arm an owned payload. The same thing happens a
few lines later with `match parse_line(trimmed)`, where `checks.append(check)`
wants to move the new `Check` into the list. Change both matches:

```aura fragment
    match own fs.read_to_string(path):
```

```aura fragment
        match own parse_line(trimmed):
```

You will write `match own` often in this chapter. The functions in `fs`,
`process`, and `net` hand back fresh owned values (file contents, child
processes, sockets), and the usual thing to do with a fresh value is keep it.

With both changes the program runs:

```console
$ aura run src/main.au
config: Probe.File(checks.txt)
notes: Probe.File(notes.txt)
```

And a line with a kind we do not know gets reported with its position. With
a file *bad.txt* whose first line is `web ftp ftp://x`, and `main` pointed at
it, the program prints:

```console
$ aura run src/main.au
ConfigError.BadLine(1, unknown kind `ftp`)
```

## Writing the report safely

Each check now needs a result. For a `File` probe that is one call,
`fs.exists`, which returns a plain `bool`. It is the only function in `fs`
that does not return a `Result`: any error while looking at the path, such as
a permission problem, counts as "does not exist". That is the right answer for
our check and the wrong one for code that needs to know why, so use it only
when "no" is all you need to hear.

```aura fragment
def run_check(check: Check) -> str:
    match check.probe:
        case Probe.File(path):
            if fs.exists(path):
                return "ok"
            return "missing"
        case Probe.Command(_):
            return "skipped"
        case Probe.Http(_):
            return "skipped"
```

This match is a plain one, because `run_check` only reads the probe.

Writing the report is where we need to be more careful. The obvious call is
`fs.write_string("report.txt", report)`, and for a scratch file it is fine. But
think about another program that reads *report.txt*, perhaps a dashboard
polling it every few seconds. `write_string` truncates the file and then
writes the new contents. A reader that opens the file between those two steps
sees an empty or half-written report. If the machine loses power in the
middle, the old report is gone and the new one is incomplete.

The standard fix, in any language, is to write the new contents to a
temporary file, force them to storage, and then rename the temporary file over
the real one. A rename within one filesystem is atomic: a reader sees either
the old file or the new one, never a mixture. Here is that recipe in Aura:

```aura
import fs
import io

def save(path: str, text: str) -> Result[None, io.Error]:
    temporary = f"{path}.tmp"
    with file = try fs.create(temporary):
        try file.write_all(text)
        try file.sync()
    try fs.rename(temporary, path)
    return Result.Ok(None)
```

<span class="caption">Listing 16-3: Replacing a file atomically</span>

This is the first time we have held a file open ourselves. `fs.create`
returns `Result[fs.File, io.Error]`, and `try` unwraps it or returns the error
to our caller. An `fs.File` is a resource, so we bind it with `with`, exactly
as [Chapter 14](/book/ch14-00-resources-and-cleanup) did for our own resource
types. When the block ends, by falling off the bottom or by any of the `try`s
returning early, the file is closed exactly once.

Three calls do the work. `write_all` hands the text to the file. A handle from
`fs.create` or `fs.append` buffers its writes in a 64 KiB buffer, the way
Python's file objects do, so many small writes cost about the same as one big
one. `sync` flushes that buffer and then waits until the operating system
reports that the data has reached the disk; it is the `fsync` system call.
Finally `fs.rename` moves the temporary file over the target, replacing any
file already there.

Why `sync` and not let the `with` block close the file? Closing does
flush the buffer, but `close()` returns nothing, so a write failure discovered
during that final flush has nowhere to go. A program that needs to know its
data was written calls `flush()` or `sync()` itself, where the `Result` can
be checked. The rest of the time, closing on scope exit is enough, and within
your own program you never see stale data: before any other filesystem
operation or child process starts, Aura flushes every open handle's buffer.

Notice that `temporary` is used twice, first by `fs.create` and then by
`fs.rename`, without any copying. The filesystem functions take their path,
text, and byte arguments with shared access for the duration of the call and
keep nothing, so the caller still owns them afterwards. The same is true of
`path`, which is a bare parameter of `save`.

The rename is atomic only within one filesystem. If *report.txt* were on a
different disk from its temporary file, `fs.rename` would fail with an
`io.Error`, which is one reason the temporary file sits right next to the
target. Aura 0.11 also has no call to sync the directory itself, which some
filesystems need before the rename survives a crash. For a health report that
is regenerated on every run, this recipe is more than enough.

## Appending to a log

Reports get replaced; logs grow. Let's keep a history of every run in
*history.log*. `fs.append` opens a file for appending and creates it if it is
not there yet:

```aura fragment
def append_history(report: str) -> Result[None, io.Error]:
    with file = try fs.append("history.log"):
        try file.write_all(f"run at {sys.unix_time_ms()}\n")
        try file.write_all(report)
    return Result.Ok(None)
```

`sys.unix_time_ms()` gives milliseconds since the Unix epoch, which is enough
to tell runs apart. The two `write_all` calls land in the handle's buffer and
reach the file when the `with` block closes it. If you only need to add one
string, `fs.append_string(path, text)` does the open, write, and close in one
call, as `fs.write_string` does for replacing.

## Taking the config path from the command line

Hard-coding *checks.txt* is limiting. `sys.args()` returns the program's
arguments as a `list[str]`, without the program name. With `aura run`,
everything after `--` is passed to the program:

```aura fragment
def config_path() -> str:
    mut args = sys.args()
    if args.len() > 0:
        return args.pop(0)
    return "checks.txt"
```

## The whole program so far

Here is everything together. The `main` function loads the checks, reports
config errors with an exit status of `2`, runs each check, saves the report,
and appends to the history.

*Filename: src/main.au*

```aura
import fs
import io
import sys

enum Probe:
    File(path: str)
    Command(argv: list[str])
    Http(url: str)

class Check:
    name: str
    probe: Probe

enum ConfigError:
    Unreadable(path: str, error: io.Error)
    BadLine(number: int64, message: str)

def parse_line(line: str) -> Result[Check, str]:
    mut parts = line.split_whitespace()
    if parts.len() < 3:
        return Result.Err("expected `name kind target`")
    name = parts.pop(0)
    kind = parts.pop(0)
    if kind == "file":
        return Result.Ok(Check(name=name, probe=Probe.File(path=parts.pop(0))))
    if kind == "http":
        return Result.Ok(Check(name=name, probe=Probe.Http(url=parts.pop(0))))
    if kind == "cmd":
        return Result.Ok(Check(name=name, probe=Probe.Command(argv=parts)))
    return Result.Err(f"unknown kind `{kind}`")

def load_checks(path: str) -> Result[list[Check], ConfigError]:
    mut text = ""
    match own fs.read_to_string(path):
        case Result.Ok(contents):
            text = contents
        case Result.Err(error):
            return Result.Err(ConfigError.Unreadable(path=path.clone(), error=error))
    mut checks: list[Check] = []
    mut number: int64 = 0
    for line in text.lines():
        number += 1
        trimmed = line.trim()
        if trimmed == "" or trimmed.starts_with("#"):
            continue
        match own parse_line(trimmed):
            case Result.Ok(check):
                checks.append(check)
            case Result.Err(message):
                return Result.Err(ConfigError.BadLine(number=number, message=message))
    return Result.Ok(checks)

def run_check(check: Check) -> str:
    match check.probe:
        case Probe.File(path):
            if fs.exists(path):
                return "ok"
            return "missing"
        case Probe.Command(_):
            return "skipped"
        case Probe.Http(_):
            return "skipped"

def save(path: str, text: str) -> Result[None, io.Error]:
    temporary = f"{path}.tmp"
    with file = try fs.create(temporary):
        try file.write_all(text)
        try file.sync()
    try fs.rename(temporary, path)
    return Result.Ok(None)

def append_history(report: str) -> Result[None, io.Error]:
    with file = try fs.append("history.log"):
        try file.write_all(f"run at {sys.unix_time_ms()}\n")
        try file.write_all(report)
    return Result.Ok(None)

def config_path() -> str:
    mut args = sys.args()
    if args.len() > 0:
        return args.pop(0)
    return "checks.txt"

def main() -> int32:
    path = config_path()
    mut checks: list[Check] = []
    match own load_checks(path):
        case Result.Ok(loaded):
            checks = loaded
        case Result.Err(ConfigError.Unreadable(file, error)):
            print(f"cannot read {file}: {error}")
            return 2
        case Result.Err(ConfigError.BadLine(number, message)):
            print(f"{path}:{number}: {message}")
            return 2
    mut report = ""
    for check in checks:
        report += f"{check.name}: {run_check(check)}\n"
    match save("report.txt", report):
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(f"cannot write report: {error}")
            return 1
    match append_history(report):
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(f"cannot update history: {error}")
    print(report.trim())
    return 0
```

<span class="caption">Listing 16-4: `checkup` with file checks, an atomic report, and a history log</span>

One small thing in `main`: the `Unreadable` arm binds the path as `file`, not
`path`. A pattern binding is not allowed to shadow a name already in scope,
and `main` already has a `path`. Run it:

```console
$ aura run src/main.au
config: ok
notes: missing
$ cat report.txt
config: ok
notes: missing
```

Try the error paths too. A missing config and a bad line both exit with
status `2`, so a script calling `checkup` can tell "the checks failed" apart
from "I could not even read the checks":

```console
$ aura run src/main.au -- missing.txt
cannot read missing.txt: io.Error.NotFound
$ aura run src/main.au -- bad.txt
bad.txt:2: unknown kind `ftp`
```

(Here *bad.txt* holds a valid `config` line followed by `web ftp
ftp://example.com`.)

## A few more filesystem tools

`checkup` does not need them, but you will reach for a few other `fs`
functions sooner or later. `fs.read_dir(path)` returns the names in a
directory, sorted:

```aura
import fs
import io

def main() -> int32:
    match fs.read_dir("."):
        case Result.Ok(names):
            for name in names:
                print(name)
            return 0
        case Result.Err(error):
            print(error)
            return 1
```

Save it as *list.au* one directory above `checkup`, so it does not list
itself, and run it from inside `checkup` after a few runs:

```console
$ aura run ../list.au
.gitignore
Aura.lock
Aura.toml
bad.txt
checks.txt
history.log
report.txt
src
tests
```

`fs.create_dir` makes one directory (its parent must exist),
`fs.remove_file` deletes a file, and `fs.read_bytes`, `fs.write_bytes`, and
`fs.append_bytes` are the raw-byte counterparts of the text functions, working
with `list[uint8]`. Text and bytes are always separate functions in Aura; no
flag turns a byte read into a text read. When you need to convert, use
`text.to_bytes()` and `str.from_bytes(bytes)`, which returns a `Result`
because the bytes may not be valid UTF-8.

The Manual's [Filesystem Module](/manual/filesystem) page lists every
function and its exact contract, including what Aura 0.11 does not have yet:
recursive directory operations, file metadata, permissions, and watchers.

## Standard input and output

The terminal is a file too, in a sense. `print` covers most output, but it
always adds a newline. The `io` module handles the rest: `io.write` writes
text with no newline, `io.flush` pushes buffered output to the terminal, and
`io.read_line` reads one line from standard input. Here is a prompt:

```aura
import io

def ask(prompt: str) -> Result[str | None, io.Error]:
    try io.write(prompt)
    try io.flush()
    return io.read_line()

def main() -> int32:
    match own ask("your name> "):
        case Result.Ok(str as name):
            print(f"hello, {name.trim()}")
            return 0
        case Result.Ok(None):
            print("")
            print("no name given")
            return 0
        case Result.Err(error):
            print(f"cannot read input: {error}")
            return 1
```

<span class="caption">Listing 16-5: Prompting for a line of input</span>

The return type of `io.read_line` is worth a second look:
`Result[str | None, io.Error]`. It separates three outcomes that a lot of
languages blur together. `Ok(str)` is a line, with its trailing newline
removed; an empty line is a present `""`. `Ok(None)` is end of input, such as
the user pressing Ctrl-D or a pipe running dry. `Err` is a real failure. The
pattern `Result.Ok(str as name)` nests a type pattern inside the `Ok`, so the
first arm matches only when there is a line. Save the program as *greet.au*
and feed it three kinds of input:

```console
$ echo "Ada" | aura run greet.au
your name> hello, Ada
$ aura run greet.au < /dev/null
your name> 
no name given
$ printf '\xff\n' | aura run greet.au
your name> cannot read input: io.Error.InvalidData
```

The last one is a byte that is not valid UTF-8. Every text read in Aura
decodes strictly, so bad bytes become `io.Error.InvalidData` instead of being
replaced with question marks behind your back. We will see the same three-way
shape again for pipes and sockets, where "the other side closed cleanly" is
as normal an outcome as "here is a line".

`checkup` can now look at the filesystem. Next we will let it run commands,
which is where timeouts start to matter.
