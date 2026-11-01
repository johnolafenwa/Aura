# Aggregating and reporting

`run` can now turn every line into a `Record` or a `None`. What we want from
`logscan` is not a list of records, though, but a summary: how many records
there were at each level, how many came from each service, which services
produced the errors, and which lines we had to skip. In this section we'll
collect those numbers in one value as the records stream past, and then print
them as a small aligned report.

## A place to keep the counts

The counts belong together, so let's give them a class. Each kind of count is a
dictionary from the thing being counted to how many times we saw it:

```aura fragment
class Summary:
    total: int64
    skipped: list[int64]
    by_level: dict[Level, int64]
    by_service: dict[str, int64]
    errors_by_service: dict[str, int64]
```

`total` counts the records we kept, and `skipped` holds the line numbers of the
lines we didn't. `by_level` is keyed by our `Level` enum directly. An enum
without payloads can be a dictionary key, so we don't need to turn levels back
into strings only to count them. The two service dictionaries use the service
name as the key.

Every count follows the same pattern: look the key up, and either store 1 or
add 1 to what is there. That deserves a helper. Here is a first version
written for string keys:

```aura
def bump(counts: mut dict[str, int64], key: own str):
    match counts.get(key):
        case Lookup.Found(count):
            counts[key] = count + 1
        case Lookup.Missing:
            counts[key] = 1
```

The signature carries the ownership story. `counts` is `mut` because `bump`
changes the caller's dictionary in place; nothing is copied or returned. `key`
is `own` because a new entry stores the key, and the dictionary has to own what
it stores. `counts.get(key)` only reads the key, so the same `key` is still
available for the assignment in either arm. `get` returns a `Lookup[int64]`,
which is `Found(value)` when the key is present and `Missing` when it is not.
Chapter 8 explained why dictionaries use `Lookup` rather than `T | None` here:
a dictionary may legitimately store `None` as a value, and `Lookup` keeps "the
value is `None`" and "there is no value" apart.

Now a method on `Summary` that folds one record into the counts:

```aura fragment
    def add(mut self, record: Record):
        self.total += 1
        bump(self.by_level, record.level)
        bump(self.by_service, record.service.clone())
        if record.level == Level.Error:
            bump(self.errors_by_service, record.service.clone())
```

The compiler stops at the second line of the body:

```console
$ aura check src/main.au
error[AU2002]: argument type mismatch for function `bump`: expected `str`, found `Level`
 --> src/main.au:53:14
   |
53 |         bump(self.by_level, record.level)
   |              ^
```

We wrote `bump` for `dict[str, int64]` and string keys, and `by_level` is a
`dict[Level, int64]` keyed by levels. The caret points at the dictionary
argument while the message talks about the key types; both are telling us the
same thing, which is that this `bump` only knows about strings. We could write
a second copy for levels, but the body would be identical. This is exactly the
situation generics exist for, from [Chapter 10](/book/ch10-00-generics-traits-conversions):
the function doesn't care what the key type is, only that the dictionary and
the key agree. Give `bump` a type parameter:

```aura
def bump[K](counts: mut dict[K, int64], key: own K):
    match counts.get(key):
        case Lookup.Found(count):
            counts[key] = count + 1
        case Lookup.Missing:
            counts[key] = 1
```

<span class="caption">Listing 12-10: A counting helper for any key type</span>

Each call fixes `K` from its arguments: `Level` for the first call in `add`,
`str` for the other two.

## Who owns the service name

Let's look more closely at the two service lines in `add`, because they each
end in `.clone()`, and it's fair to ask why. Try deleting the first one:

```aura fragment
        bump(self.by_service, record.service)
```

```console
$ aura check src/main.au
error[AU3002]: cannot move non-copy field `service` out of borrowed value `record`
 --> src/main.au:54:38
   |
54 |         bump(self.by_service, record.service)
   |                                      ^
  = related src/main.au:51:23: `record` is borrowed here
  = help: take `record` as `own Record` when the field should be moved, or call `.clone()` on the field to return an independent value
  = fix: replace src/main.au:54:45-54:45 with `.clone()`
```

`bump` takes its key with `own`, so passing `record.service` would move the
string out of the record and into the dictionary. But `add` received `record`
as a bare parameter, which is shared access: it may read the record, and it
must give it back whole. Moving a field out would hand back a record with a
hole in it. The `related` line points to the parameter that made the access
shared, and the help line offers both ways out. We could take the record with
`own` and move its fields, or we could clone the field.

Here, cloning is the honest choice. The service name has to end up as a key in
two different dictionaries for error records, and a string can only have one
owner, so at least one copy is needed anyway. Service names are short, and we
make one or two small copies per record. If profiling ever showed this
mattered, taking `own Record` and moving the name into the last dictionary that
needs it would save one copy per record. The compiler would then check that we
didn't touch `record.service` after the move.

The level needs no `.clone()`. `Level` is an enum without payloads, which is a
copy value, so passing it copies it and the record keeps its own.

## Building the summary

`summarize` takes the file's text and returns a filled-in `Summary`. It starts
from an empty one and calls `add` for every record:

```aura fragment
def summarize(text: str) -> Summary:
    mut summary = Summary(
        total=0,
        skipped=[],
        by_level={},
        by_service={},
        errors_by_service={},
    )
    for index, line in enumerate(text.lines()):
        if line.trim() == "":
            continue
        match parse_line(line):
            case Record as record:
                summary.add(record)
            case None:
                summary.skipped.append(index + 1)
    return summary
```

<span class="caption">Listing 12-11: Folding every line into one `Summary`</span>

The empty literals `[]` and `{}` would be ambiguous on their own, but here each
one is the value of a field whose type the class declares, so the compiler
knows `{}` for `by_level` means an empty `dict[Level, int64]`.

The `if line.trim() == ""` test answers the question we left open in the last
section. A blank line isn't a malformed record; it's nothing. Counting it as
skipped would make the report noisier without telling anyone anything, so we
pass over it with `continue`.

The `mut` on `summary` is not decoration. Leave it off and the compiler
reports the first place we try to change the summary:

```console
$ aura check src/main.au
error[AU3003]: method `add` requires a mutable receiver
 --> src/main.au:78:25
   |
78 |                 summary.add(record)
   |                         ^
  = help: declare the receiver place with `mut` before calling a mutating method
```

`add` is declared with `mut self`, so it can only be called on a binding that
allows mutation. That is the same rule that applies to `mut` parameters,
applied to the receiver. It means you can read a function and know which of
its bindings can change by looking for `mut`, without reading every method it
calls.

## Printing the report

With the numbers collected, the report is mostly formatting. We want each
level printed in upper case, the way it appears in the log. `print(level)`
would give us `Level.Error`, so we need a function from `Level` to its log
spelling. Here is one that forgets a case:

```aura check-fail:AU2999
enum Level:
    Debug
    Info
    Warn
    Error

def level_name(level: Level) -> str:
    match level:
        case Level.Debug:
            return "DEBUG"
        case Level.Info:
            return "INFO"
        case Level.Warn:
            return "WARN"
```

```console
$ aura check src/main.au
error[AU2999]: non-exhaustive match over `Level`: missing `Error`
 --> src/main.au:84:5
   |
84 |     match level:
   |     ^
```

The message names the missing variant. This is the payoff for using an enum
instead of a string: if we add a `Trace` level next month, every `match` over
`Level` that doesn't handle it will point itself out, and we won't ship a
report that silently drops a row. Add the missing arm, `case Level.Error:`
returning `"ERROR"`, and the function compiles.

The report itself prints a header line, then three blocks of counts:

```aura fragment
def print_report(path: str, summary: Summary):
    print(f"{path}: {summary.total} records, {summary.skipped.len()} skipped")
    print("")
    print("by level")
    for level in [Level.Debug, Level.Info, Level.Warn, Level.Error]:
        match summary.by_level.get(level):
            case Lookup.Found(count):
                print(f"  {level_name(level):<8s}{count:>5d}")
            case Lookup.Missing:
                pass
    print("")
    print("by service")
    for service in sorted(summary.by_service.keys()):
        print(f"  {service:<8s}{summary.by_service[service]:>5d}")
    print("")
    print("errors by service")
    for service in sorted(summary.errors_by_service.keys()):
        print(f"  {service:<8s}{summary.errors_by_service[service]:>5d}")
    if summary.skipped.len() > 0:
        print("")
        print(f"skipped lines: {summary.skipped}")
```

<span class="caption">Listing 12-12: Formatting the summary as a report</span>

Two things in this function deserve a closer look.

The format specifications are the ones from
[Chapter 8](/book/ch08-02-strings), and they use the same mini-language as
Python's. `{name:<8s}` pads a
string on the right to eight characters, and `{count:>5d}` right-aligns an
integer in five. The compiler checks these specifications against the value's
type, so `{count:<8s}` on an integer is a compile error, not a surprise at run
time.

Dictionaries in Aura iterate in insertion order. For `by_level`, insertion
order would be whatever order levels happened to appear in the log, which
changes from file to file. So instead of iterating the dictionary, we iterate
a list of levels in severity order and look each one up, printing only the
levels that occurred. For services there is no natural order, so we sort the
names with `sorted(summary.by_service.keys())`. `keys()` returns a fresh list
of the keys, and `sorted` returns a new sorted list. Either way, two runs over
the same log always print the same report, which matters as soon as anyone
compares reports or writes a test against one.

Inside the service loops, `summary.by_service[service]` reads one count
directly. Indexing a dictionary with a missing key would trap, but here every
key came from the dictionary itself a line earlier, so the lookup can't miss.

`run` shrinks to three steps: read, summarize, print.

```aura fragment
def run(path: str) -> Result[None, io.Error]:
    text = try fs.read_to_string(path)
    summary = summarize(text)
    print_report(path, summary)
    return Ok(None)
```

## The whole program

Listing 12-13 is `src/main.au` with every piece in place.

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

class Summary:
    total: int64
    skipped: list[int64]
    by_level: dict[Level, int64]
    by_service: dict[str, int64]
    errors_by_service: dict[str, int64]

    def add(mut self, record: Record):
        self.total += 1
        bump(self.by_level, record.level)
        bump(self.by_service, record.service.clone())
        if record.level == Level.Error:
            bump(self.errors_by_service, record.service.clone())

def bump[K](counts: mut dict[K, int64], key: own K):
    match counts.get(key):
        case Lookup.Found(count):
            counts[key] = count + 1
        case Lookup.Missing:
            counts[key] = 1

def summarize(text: str) -> Summary:
    mut summary = Summary(
        total=0,
        skipped=[],
        by_level={},
        by_service={},
        errors_by_service={},
    )
    for index, line in enumerate(text.lines()):
        if line.trim() == "":
            continue
        match parse_line(line):
            case Record as record:
                summary.add(record)
            case None:
                summary.skipped.append(index + 1)
    return summary

def level_name(level: Level) -> str:
    match level:
        case Level.Debug:
            return "DEBUG"
        case Level.Info:
            return "INFO"
        case Level.Warn:
            return "WARN"
        case Level.Error:
            return "ERROR"

def print_report(path: str, summary: Summary):
    print(f"{path}: {summary.total} records, {summary.skipped.len()} skipped")
    print("")
    print("by level")
    for level in [Level.Debug, Level.Info, Level.Warn, Level.Error]:
        match summary.by_level.get(level):
            case Lookup.Found(count):
                print(f"  {level_name(level):<8s}{count:>5d}")
            case Lookup.Missing:
                pass
    print("")
    print("by service")
    for service in sorted(summary.by_service.keys()):
        print(f"  {service:<8s}{summary.by_service[service]:>5d}")
    print("")
    print("errors by service")
    for service in sorted(summary.errors_by_service.keys()):
        print(f"  {service:<8s}{summary.errors_by_service[service]:>5d}")
    if summary.skipped.len() > 0:
        print("")
        print(f"skipped lines: {summary.skipped}")

def run(path: str) -> Result[None, io.Error]:
    text = try fs.read_to_string(path)
    summary = summarize(text)
    print_report(path, summary)
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

<span class="caption">Listing 12-13: `logscan` as one file, reading, parsing, counting, and reporting</span>

Run it against the sample log:

```console
$ aura run src/main.au -- app.log
app.log: 9 records, 2 skipped

by level
  DEBUG       1
  INFO        4
  WARN        1
  ERROR       3

by service
  api         4
  db          2
  worker      3

errors by service
  api         1
  db          1
  worker      1

skipped lines: [7, 11]
```

Check it against the file by hand once. Twelve lines, one blank, two
malformed, leaves nine records. Four are `INFO` (lines 1, 2, 6, and 10), and
`api` has four records because line 11, the `NOTICE` line, was skipped before
it could be counted. The program works, and it is a little over 130 lines in one file.

That's a good moment to stop and look at it critically. `main.au` now does
four unrelated jobs: it handles arguments, parses text, keeps counts, and
formats output. None of them can be tested without running the whole program
against a file. The skipped-line report says *which* lines were dropped but
not *why*. And the error messages from the first section still print raw enum
values. In the next section we'll fix all of that without changing what a
successful run prints, except for the reasons we'll add to it.
