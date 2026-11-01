# Parsing lines into records

At the end of the last section `run` held a list of lines, each one a plain
`str`. We could count errors straight from those strings with
`line.contains("ERROR")`, and for a one-off question that would be fine. But
"contains `ERROR`" also matches a message like `retrying after ERROR from
upstream`, it can't tell us which service a line came from, and every new
question would mean another round of string poking. It is better to read each
line once, at the edge of the program, and turn it into a value whose type says
exactly what a log entry is. After that the rest of the program works with
types instead of text, and the compiler can check it.

## Describing a record

A well-formed line has four parts: a timestamp, a level, the service name, and
the message. Most of those are text, but the level is not really free text. It
is one of a small, fixed set of values, which is what an `enum` is for. Add
these declarations to `src/main.au`, above `run`:

```aura
enum Level:
    Debug
    Info
    Warn
    Error

class Record:
    timestamp: str
    level: Level
    service: str
    message: str
```

<span class="caption">Listing 12-5: The types a parsed line turns into</span>

Using `Level` instead of `str` for the level field pays off right away. A
`Record` can't hold a level spelled `"EROR"` or `"warning"`, because no such
variant exists. When we later write code that does something for each level,
`match` will tell us if we forget one. We keep the timestamp as text, because
`logscan` only prints it and never does arithmetic on it. If we wanted to ask
"how many errors in the last hour", parsing the timestamp into a number would
be the next step, but we don't need it yet.

## Parsing the level

Turning a word into a `Level` is a `match` on the string. Each literal case
returns a variant, and the `_` case catches everything else:

```aura
enum Level:
    Debug
    Info
    Warn
    Error

def parse_level(text: str) -> Level | None:
    match text:
        case "DEBUG":
            return Level.Debug
        case "INFO":
            return Level.Info
        case "WARN":
            return Level.Warn
        case "ERROR":
            return Level.Error
        case _:
            return None
```

<span class="caption">Listing 12-6: Mapping a word to a `Level`, or to `None`</span>

The return type is `Level | None`, the optional type from Chapter 6. A word we
don't recognize is not a bug in our program, and it is not a reason to stop
reading the file. It is an ordinary outcome that the caller has to deal with,
and putting `None` in the type makes the caller deal with it.

## Parsing a whole line

Now `parse_line`. The plan is to split the line on whitespace, check that there
are at least four pieces, parse the second piece as a level, and put the rest
of the pieces back together as the message. Here is a first version:

```aura check-fail:AU2002
enum Level:
    Debug
    Info
    Warn
    Error

class Record:
    timestamp: str
    level: Level
    service: str
    message: str

def parse_level(text: str) -> Level | None:
    match text:
        case "DEBUG":
            return Level.Debug
        case "INFO":
            return Level.Info
        case "WARN":
            return Level.Warn
        case "ERROR":
            return Level.Error
        case _:
            return None

def parse_line(line: str) -> Record | None:
    fields = line.split_whitespace()
    if fields.len() < 4:
        return None
    level = parse_level(fields[1])
    return Record(
        timestamp=fields[0].clone(),
        level=level,
        service=fields[2].clone(),
        message=" ".join(fields[3:]),
    )
```

<span class="caption">Listing 12-7: A first `parse_line` that forgets the unknown-level case</span>

`split_whitespace()` splits on runs of spaces and tabs and drops the empty
pieces, so two spaces between fields don't produce an empty field.
`fields[3:]` is a slice, a new list holding the fourth piece onward, and
`" ".join(...)` glues those pieces back into one string with single spaces.
The timestamp and service use `.clone()` for the reason we met in the last
section: `fields` owns its strings, and the `Record` needs strings of its own.

The compiler isn't happy with this version:

```console
$ aura check src/main.au
error[AU2002]: field `level` expects `Level`, found `Level | None` (expected `Level`, found `Level | None`)
 --> src/main.au:37:9
   |
37 |         level=level,
   |         ^
```

We declared `Record.level` as a `Level`, but `parse_level` hands back a
`Level | None`, and we passed that straight into the constructor. In Python the
equivalent code would run, build a record whose level is `None`, and fail
somewhere much later, perhaps in the report code, perhaps only on the one log
file where an unknown level shows up. Aura won't let a value that might be
`None` flow into a place that promises it isn't. We have to decide here, at the
point where we know the most about the line, what an unknown level means.

For now an unknown level means the line is skipped, the same as a line with too
few fields. Add a test for `None` right after the call:

```aura fragment
    level = parse_level(fields[1])
    if level is None:
        return None
```

The function returns early when the level is missing, so after the `if` the
compiler knows `level` can only be a `Level`. This is *narrowing*: an
`is None` or `is not None` test on a binding refines its type on each path
the test selects. The constructor call below it now type-checks without any
further conversion.

Here is the whole function as it stands:

```aura
enum Level:
    Debug
    Info
    Warn
    Error

class Record:
    timestamp: str
    level: Level
    service: str
    message: str

def parse_level(text: str) -> Level | None:
    match text:
        case "DEBUG":
            return Level.Debug
        case "INFO":
            return Level.Info
        case "WARN":
            return Level.Warn
        case "ERROR":
            return Level.Error
        case _:
            return None

def parse_line(line: str) -> Record | None:
    fields = line.split_whitespace()
    if fields.len() < 4:
        return None
    level = parse_level(fields[1])
    if level is None:
        return None
    return Record(
        timestamp=fields[0].clone(),
        level=level,
        service=fields[2].clone(),
        message=" ".join(fields[3:]),
    )
```

<span class="caption">Listing 12-8: `parse_line` with both failure cases handled</span>

Joining the message with single spaces does lose information: a message that
had two spaces in a row comes back with one. For a report that counts records,
that doesn't matter. If `logscan` ever needs the exact original text, the
`Record` could keep the whole line as well, and nothing else in the design
would have to change.

## Numbering the lines

When `logscan` skips a line, the user will want to know which one. Users count
lines from 1, as editors and `grep -n` do. A Python programmer reaches for
`enumerate(lines, start=1)`:

```aura fragment
    for number, line in enumerate(text.lines(), start=1):
```

```console
$ aura check src/main.au
error[AU2004]: `enumerate` takes 1 iterable, found 2
 --> src/main.au:46:25
   |
46 |     for number, line in enumerate(text.lines(), start=1):
   |                         ^
```

Aura's `enumerate` takes exactly one argument and always counts from zero. The
fix is one line of arithmetic inside the loop, `number = index + 1`, which you
can see in Listing 12-9. You'll meet a handful of small gaps like this when you
bring Python habits along. Each one is caught at the call, before the program
runs, with a message that says what the function expected.

## Using the parser

Let's wire `parse_line` into `run` and print one line of output per input line,
so we can see what the parser decided. Listing 12-9 is the full file.

*Filename: src/main.au*

```aura
import fs
import io
import sys

enum Level:
    Debug
    Info
    Warn
    Error

class Record:
    timestamp: str
    level: Level
    service: str
    message: str

def parse_level(text: str) -> Level | None:
    match text:
        case "DEBUG":
            return Level.Debug
        case "INFO":
            return Level.Info
        case "WARN":
            return Level.Warn
        case "ERROR":
            return Level.Error
        case _:
            return None

def parse_line(line: str) -> Record | None:
    fields = line.split_whitespace()
    if fields.len() < 4:
        return None
    level = parse_level(fields[1])
    if level is None:
        return None
    return Record(
        timestamp=fields[0].clone(),
        level=level,
        service=fields[2].clone(),
        message=" ".join(fields[3:]),
    )

def run(path: str) -> Result[None, io.Error]:
    text = try fs.read_to_string(path)
    for index, line in enumerate(text.lines()):
        number = index + 1
        match parse_line(line):
            case Record as record:
                print(f"{number}: {record.level} {record.service}")
            case None:
                print(f"{number}: skipped")
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

<span class="caption">Listing 12-9: Parsing every line and printing what the parser saw</span>

The `match` in `run` has one arm for each member of `Record | None`. The
pattern `Record as record` matches when the value is a `Record` and binds it to
`record`, so the arm body can read its fields. The `None` arm handles a skipped
line. If we deleted either arm, the match would no longer be exhaustive and the
compiler would reject it, which is the same guarantee we got for `Result` in
`main`.

```console
$ aura run src/main.au -- app.log
1: Level.Info api
2: Level.Info api
3: Level.Warn db
4: Level.Error api
5: Level.Error db
6: Level.Info worker
7: skipped
8: Level.Debug worker
9: skipped
10: Level.Info api
11: skipped
12: Level.Error worker
```

Check the skipped lines against the file. Line 7, `connection pool
exhausted`, has three fields. Line 9 is blank, so it has none. Line 11 has
four fields, but `NOTICE` is not one of our levels. All three came back as
`None`, which is correct, but the output can't tell them apart, and a blank
line probably shouldn't count as "skipped" at all. A reader of the report would
want to know *why* a line was dropped. We'll come back to that in the last
section, when we give `parse_line` a richer error type. First, let's turn this
stream of records into the report we actually want.
