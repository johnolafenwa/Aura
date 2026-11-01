# Refactoring with tests and better errors

`logscan` works, and it is the kind of program that tends to stay in one file
long after it should have been split up. The trouble with one file is not its
length. It is that every part can reach every other part, so nothing can be
checked on its own. To find out whether `parse_line` handles a lowercase level,
we would have to write a log file, run the whole program, and read the report.

In this section we'll make four changes, in an order that keeps the program
working after each one:

1. Split `main.au` into modules, one per job.
2. Give `parse_line` an error type that says why a line was rejected.
3. Move argument handling into a `Config` value, and add a `--min-level`
   option.
4. Write tests for each module, and use one of them to drive a fix.

Along the way we'll replace the raw `io.Error.NotFound` messages with ones a
person can act on.

## Deciding where the lines go

A useful test for where a module boundary belongs is to ask what each part of
the program needs to know. The parser needs to know the log format and nothing
about counting. The counting code needs records, but not where they came from.
The report needs a summary, but not the format of the input. Argument handling
needs to know the command-line syntax and nothing else. And `main` is the only
part that should know about all of them, because its job is to connect them.

That gives us five files:

```text
logscan/
├── Aura.toml
├── app.log
├── src/
│   ├── config.au    command-line arguments into a Config
│   ├── main.au      run, error messages, exit statuses
│   ├── records.au   Level, Record, and parse_line
│   ├── report.au    a Summary into report text
│   └── stats.au     records into a Summary
└── tests/
```

A file under `src/` is a module named after its path, so `src/records.au` is
the module `records`, imported with `import records` or
`from records import Record`. [Chapter 7](/book/ch07-00-modules-and-packages)
covered the rules. The only one we need right now is that everything in a
module is private unless it is marked `public`.

## The records module

Let's move the parsing code first, since everything else depends on it. Create
`src/records.au` and move `Level`, `Record`, `parse_level`, `parse_line`, and
`level_name` into it, with `public` on each declaration other modules will
use. While we're here, we'll make the change the end of the last section asked
for: instead of returning `None` for a bad line, `parse_line` will say what was
wrong with it.

*Filename: src/records.au*

```aura
public enum Level:
    Debug
    Info
    Warn
    Error

public class Record:
    public timestamp: str
    public level: Level
    public service: str
    public message: str

public enum ParseError:
    TooFewFields(found: int64)
    UnknownLevel(text: str)

public def parse_level(text: str) -> Level | None:
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

public def level_name(level: Level) -> str:
    match level:
        case Level.Debug:
            return "DEBUG"
        case Level.Info:
            return "INFO"
        case Level.Warn:
            return "WARN"
        case Level.Error:
            return "ERROR"

public def parse_line(line: str) -> Result[Record, ParseError]:
    fields = line.split_whitespace()
    if fields.len() < 4:
        return Err(ParseError.TooFewFields(found=fields.len()))
    level = parse_level(fields[1])
    if level is None:
        return Err(ParseError.UnknownLevel(text=fields[1].clone()))
    return Ok(Record(
        timestamp=fields[0].clone(),
        level=level,
        service=fields[2].clone(),
        message=" ".join(fields[3:]),
    ))

public def describe(error: ParseError) -> str:
    match error:
        case ParseError.TooFewFields(found):
            return f"expected at least 4 fields, found {found}"
        case ParseError.UnknownLevel(text):
            return f"unknown level `{text}`"
```

<span class="caption">Listing 12-14: The parser in its own module, with a reason for every rejected line</span>

`parse_line` now returns `Result[Record, ParseError]` instead of
`Record | None`. The question to ask when choosing between them is whether the
caller could do anything useful with a reason. In the last section the answer
was no; now we want to print one, so the absence carries data. Each variant of
`ParseError` holds exactly what the message needs: the number of fields found,
or the word that wasn't a level. Keeping the reason as an enum, rather than
building the message string inside `parse_line`, means a test can check *which*
failure happened without comparing English sentences, and the wording lives in
one place, `describe`.

Notice the `public` on each field of `Record` as well as on the class. Marking
a class public lets other modules name it, but its fields stay private unless
they are marked too. If you leave the fields private, the first module that
reads one will tell you:

```console
$ aura check src/main.au
error[AU2999]: field `level` is private on `Record`
 --> src/stats.au:53:32
   |
53 |                 if rank(record.level) >= rank(min_level):
   |                                ^
```

That default is deliberate. A field you haven't made public is one you can
rename or remove without breaking another module, so Aura makes you opt in to
each one. For a plain data type like `Record`, whose whole purpose is to carry
these four values, public fields are the right choice.

## The stats module

`Summary`, `bump`, and `summarize` move to `src/stats.au`. Two things change.
A skipped line now records its reason alongside its number, so we give it a
small class, `Skipped`. And `summarize` takes a minimum level, so that
`--min-level warn` can drop `DEBUG` and `INFO` records. Comparing levels needs
an order, which `rank` provides.

*Filename: src/stats.au*

```aura fragment
import records
from records import Level

public class Skipped:
    public line: int64
    public reason: str

public class Summary:
    public total: int64
    public skipped: list[Skipped]
    public by_level: dict[Level, int64]
    public by_service: dict[str, int64]
    public errors_by_service: dict[str, int64]

    public def add(mut self, record: records.Record):
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

public def rank(level: Level) -> int32:
    match level:
        case Level.Debug:
            return 0
        case Level.Info:
            return 1
        case Level.Warn:
            return 2
        case Level.Error:
            return 3

public def summarize(text: str, min_level: Level) -> Summary:
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
        match records.parse_line(line):
            case Ok(record):
                if rank(record.level) >= rank(min_level):
                    summary.add(record)
            case Err(error):
                summary.skipped.append(Skipped(line=index + 1, reason=records.describe(error)))
    return summary
```

<span class="caption">Listing 12-15: Counting, in its own module, with a level filter</span>

`bump` has no `public`, so it stays an implementation detail of `stats`. No
other module can call it, which means we can change how counting works later
without looking anywhere else.

This module uses both import forms: `import records` for the functions and for
`Record`, which it names as `records.Record`, and `from records import Level`
for the type it uses everywhere. Which form to use is mostly a matter of
taste: `records.Record` tells a reader at the point of use where the type comes
from, while `Level` reads better in the many places that compare levels.

## The report module

In the single-file version, `print_report` printed as it went. That is fine
for a program and awkward for a test, because a test has no easy way to look
at what was printed. So `report.render` builds the lines in a list and returns them
joined into one string, and `main` does the printing. The report now also
lists each skipped line with its reason, and leaves out the errors block when
there were no errors.

*Filename: src/report.au*

```aura fragment
from records import Level, level_name
from stats import Summary

public def render(path: str, summary: Summary) -> str:
    mut out: list[str] = []
    out.append(f"{path}: {summary.total} records, {summary.skipped.len()} skipped")
    out.append("")
    out.append("by level")
    for level in [Level.Debug, Level.Info, Level.Warn, Level.Error]:
        match summary.by_level.get(level):
            case Lookup.Found(count):
                out.append(row(level_name(level), count))
            case Lookup.Missing:
                pass
    out.append("")
    out.append("by service")
    for service in sorted(summary.by_service.keys()):
        out.append(row(service, summary.by_service[service]))
    if summary.errors_by_service.len() > 0:
        out.append("")
        out.append("errors by service")
        for service in sorted(summary.errors_by_service.keys()):
            out.append(row(service, summary.errors_by_service[service]))
    if summary.skipped.len() > 0:
        out.append("")
    for skipped in summary.skipped:
        out.append(f"skipped line {skipped.line}: {skipped.reason}")
    return "\n".join(out)

def row(name: str, count: int64) -> str:
    return f"  {name:<8s}{count:>5d}"
```

<span class="caption">Listing 12-16: The report as a function from a summary to text</span>

The repeated format string became the private helper `row`, so the column
widths are written once. Separating "compute the text" from "write the text"
is worth doing in almost any program that produces output. It is what lets the
test at the end of this section check the report without capturing standard
output.

## Arguments as a Config

So far `main` has looked at `args[0]` directly. With an option to support, the
argument handling grows enough to deserve its own module and its own error
type. The goal is that every mistake a user can make on the command line turns
into a specific message, and none of them turns into a trap.

*Filename: src/config.au*

```aura fragment
from records import Level, parse_level

public class Config:
    public path: str
    public min_level: Level

public enum ConfigError:
    MissingPath
    MissingValue(flag: str)
    UnknownFlag(flag: str)
    BadLevel(text: str)
    ExtraArgument(text: str)

public def parse_args(args: list[str]) -> Result[Config, ConfigError]:
    mut path: str | None = None
    mut min_level = Level.Debug
    mut index = 0
    while index < args.len():
        arg = args[index].clone()
        if arg == "--min-level":
            if index + 1 >= args.len():
                return Err(ConfigError.MissingValue(flag=arg))
            text = args[index + 1].clone()
            match parse_level(text):
                case Level as level:
                    min_level = level
                case None:
                    return Err(ConfigError.BadLevel(text=text))
            index += 2
        elif arg.starts_with("-"):
            return Err(ConfigError.UnknownFlag(flag=arg))
        elif path is not None:
            return Err(ConfigError.ExtraArgument(text=arg))
        else:
            path = arg
            index += 1
    match own path:
        case str as found:
            return Ok(Config(path=found, min_level=min_level))
        case None:
            return Err(ConfigError.MissingPath)

public def describe(error: ConfigError) -> str:
    match error:
        case ConfigError.MissingPath:
            return "missing the log file to read"
        case ConfigError.MissingValue(flag):
            return f"`{flag}` needs a value"
        case ConfigError.UnknownFlag(flag):
            return f"unknown option `{flag}`"
        case ConfigError.BadLevel(text):
            return f"`{text}` is not a level; use debug, info, warn, or error"
        case ConfigError.ExtraArgument(text):
            return f"unexpected argument `{text}`; logscan reads one file"
```

<span class="caption">Listing 12-17: Parsing the command line into a `Config`, or a specific error</span>

`parse_args` takes the argument list as a parameter instead of calling
`sys.args()` itself. That one decision is what makes it testable: a test can
pass `["app.log", "--min-level", "warn"]` without starting a process.

The loop walks the arguments with an explicit index, because `--min-level`
consumes two of them. Each argument is cloned into an owned `arg`, since most
paths through the loop store it somewhere: in `path`, or in an error variant
that outlives the list. Every check that could go wrong returns a
`ConfigError` naming the exact argument at fault. A path is `str | None` until
we find one, and the final `match own` turns it into a `Config` or a
`MissingPath` error. `own` moves the string out of `path` into `found`, so the
`Config` takes it over without a copy.

The flag names a minimum level, and it reuses `parse_level` from the records
module. That reuse is a small win of the module split: the set of valid levels
is defined in one place, and the log parser and the command line can't drift
apart.

## A thin main

What's left for `main.au` is the glue: parse the arguments, run, and turn
each failure into a message and an exit status.

*Filename: src/main.au*

```aura fragment
import config
import fs
import io
import report
import stats
import sys

def run(settings: config.Config) -> Result[None, io.Error]:
    text = try fs.read_to_string(settings.path)
    summary = stats.summarize(text, settings.min_level)
    print(report.render(settings.path, summary))
    return Ok(None)

def explain(error: io.Error) -> str:
    match error:
        case io.Error.NotFound:
            return "no such file"
        case io.Error.PermissionDenied:
            return "permission denied"
        case io.Error.IsDirectory:
            return "it is a directory, not a file"
        case io.Error.InvalidData:
            return "the file is not valid UTF-8 text"
        case _:
            return f"{error}"

def main() -> int32:
    match config.parse_args(sys.args()):
        case Ok(settings):
            match run(settings):
                case Ok(_):
                    return 0
                case Err(error):
                    print(f"logscan: cannot read {settings.path}: {explain(error)}")
                    return 1
        case Err(error):
            print(f"logscan: {config.describe(error)}")
            print("usage: logscan <path> [--min-level LEVEL]")
            return 2
```

<span class="caption">Listing 12-18: `main.au` reduced to wiring and error messages</span>

`explain` translates the `io.Error` variants a user is likely to cause into
plain words, and falls back to the variant's own rendering for the rest.
`InvalidData` is what `fs.read_to_string` reports when a file isn't valid
UTF-8, which is worth a clear message, since pointing a text tool at a binary
file by mistake is common. We kept the `io.Error` value all the way up to this
function instead of turning it into a string inside `run`. That way the
decision about wording is made once, at the top, where we know who is reading.

Usage errors exit with 2 and print the usage line. A file that can't be read
exits with 1 and doesn't, because the usage was fine. Here is every failure we
know how to cause:

```console
$ aura run src/main.au --
logscan: missing the log file to read
usage: logscan <path> [--min-level LEVEL]
$ aura run src/main.au -- --verbose app.log
logscan: unknown option `--verbose`
usage: logscan <path> [--min-level LEVEL]
$ aura run src/main.au -- app.log --min-level
logscan: `--min-level` needs a value
usage: logscan <path> [--min-level LEVEL]
$ aura run src/main.au -- app.log --min-level loud
logscan: `loud` is not a level; use debug, info, warn, or error
usage: logscan <path> [--min-level LEVEL]
$ aura run src/main.au -- app.log other.log
logscan: unexpected argument `other.log`; logscan reads one file
usage: logscan <path> [--min-level LEVEL]
$ aura run src/main.au -- nope.log
logscan: cannot read nope.log: no such file
$ aura run src/main.au -- src
logscan: cannot read src: it is a directory, not a file
```

Compare these with the first section's `io.Error.NotFound`. Each message now
names the thing that was wrong and, where it helps, what would be right.

## Testing each module

`aura new` created a `tests/` directory with a placeholder in it. Delete
`tests/smoke.au`; we'll replace it with one test file per module. As
[Chapter 11](/book/ch11-00-testing) explained, `aura test` runs every
parameterless function whose name starts with `test_` in the files under
`tests/`, and a test fails when it traps, for example on a failed `assert`.
Test files import the package's modules the same way `main.au` does.

Start with the parser. A test for a good line checks the fields, and a test
for a bad line checks which `ParseError` came back:

*Filename: tests/test_records.au*

```aura fragment
from records import Level, ParseError, parse_line

def test_parses_a_well_formed_line():
    match parse_line("2026-10-31T09:14:09 ERROR db connection reset by peer"):
        case Ok(record):
            assert record.level == Level.Error
            assert record.service == "db"
            assert record.message == "connection reset by peer"
        case Err(error):
            assert false, f"unexpected error {error}"

def test_short_line_reports_field_count():
    match parse_line("connection pool exhausted"):
        case Ok(_):
            assert false, "a three-field line should not parse"
        case Err(error):
            assert error == ParseError.TooFewFields(found=3)
```

<span class="caption">Listing 12-19: Tests for `parse_line`</span>

Because `ParseError` is an enum with data, the second test compares the whole
value, `TooFewFields(found=3)`, which checks both the kind of failure and the
count it carries.

For `stats`, a small log written inline as a string is enough. It has a good
`INFO` line, a blank line, a good `ERROR` line, and a bad one:

*Filename: tests/test_stats.au*

```aura fragment
from records import Level
from stats import summarize

LOG = "2026-10-31T09:00:00 INFO api up\n\n2026-10-31T09:00:01 ERROR api down\ngarbage\n"

def test_counts_records_and_skips_bad_lines():
    summary = summarize(LOG, Level.Debug)
    assert summary.total == 2
    assert summary.skipped.len() == 1
    assert summary.skipped[0].line == 4
    assert summary.by_service["api"] == 2
    assert summary.errors_by_service["api"] == 1

def test_min_level_filters_records():
    summary = summarize(LOG, Level.Error)
    assert summary.total == 1
    assert summary.by_level.len() == 1
```

<span class="caption">Listing 12-20: Tests for `summarize`</span>

The first test pins down two decisions we made on purpose: the blank line is
not counted as skipped, and the skipped line is reported as line 4, counting
from 1. Those are exactly the details a later edit could break without anyone
noticing, so they are the details worth testing.

It's good practice to see a test fail once, to know that it can. Change
`summary.total == 2` to `== 3` and run only this file:

```console
$ aura test tests/test_stats.au
FAILED tests/test_stats.au::test_counts_records_and_skips_bad_lines
error[AU4001]: assertion failed
 --> tests/test_stats.au:8:5
  |
8 |     assert summary.total == 3
  |     ^
  = note: left = 2
  = note: right = 3
  = note: Aura call chain (innermost first): test_counts_records_and_skips_bad_lines at 6:1
ok tests/test_stats.au::test_min_level_filters_records
1 passed; 1 failed
```

For a comparison, the failure shows both operands, so you can see what the
code produced without adding a `print`. Put the 2 back.

The report test uses `summarize` to build its input and checks two lines of
the output:

*Filename: tests/test_report.au*

```aura fragment
from records import Level
from report import render
from stats import summarize

def test_report_lists_skipped_lines_with_reasons():
    summary = summarize("2026-10-31T09:00:00 INFO api up\nshort line\n", Level.Debug)
    text = render("test.log", summary)
    assert text.starts_with("test.log: 1 records, 1 skipped")
    assert text.contains("skipped line 2: expected at least 4 fields, found 2")
```

And the configuration tests try a good command line, the default, and two
mistakes:

*Filename: tests/test_config.au*

```aura fragment
from config import ConfigError, parse_args
from records import Level

def test_path_and_level():
    match parse_args(["app.log", "--min-level", "warn"]):
        case Ok(settings):
            assert settings.path == "app.log"
            assert settings.min_level == Level.Warn
        case Err(error):
            assert false, f"unexpected error {error}"

def test_level_defaults_to_debug():
    match parse_args(["app.log"]):
        case Ok(settings):
            assert settings.min_level == Level.Debug
        case Err(error):
            assert false, f"unexpected error {error}"

def test_rejects_unknown_flag():
    match parse_args(["--verbose", "app.log"]):
        case Ok(_):
            assert false, "--verbose should be rejected"
        case Err(error):
            assert error == ConfigError.UnknownFlag(flag="--verbose")

def test_missing_path():
    match parse_args([]):
        case Ok(_):
            assert false, "no arguments should be rejected"
        case Err(error):
            assert error == ConfigError.MissingPath
```

<span class="caption">Listing 12-21: Tests for `parse_args`</span>

## Letting a test find a bug

We wrote `test_path_and_level` the way a user would type the command, with a
lowercase `warn`, and that is a problem: `parse_level` only knows the
upper-case spellings. Logs from some other services spell levels in lower case
too, so let's state that requirement as a test in `tests/test_records.au`
before we fix anything:

```aura fragment
def test_level_is_case_insensitive():
    match parse_line("2026-10-31T09:20:00 warn api retrying upstream"):
        case Ok(record):
            assert record.level == Level.Warn
        case Err(error):
            assert false, f"unexpected error {error}"
```

Now run the whole suite:

```console
$ aura test
FAILED tests/test_config.au::test_path_and_level
error[AU4001]: unexpected error config.ConfigError.BadLevel(warn)
 --> tests/test_config.au:10:13
   |
10 |             assert false, f"unexpected error {error}"
   |             ^
  = note: Aura call chain (innermost first): test_path_and_level at 4:1
ok tests/test_config.au::test_level_defaults_to_debug
ok tests/test_config.au::test_rejects_unknown_flag
ok tests/test_config.au::test_missing_path
ok tests/test_records.au::test_parses_a_well_formed_line
ok tests/test_records.au::test_short_line_reports_field_count
FAILED tests/test_records.au::test_level_is_case_insensitive
error[AU4001]: unexpected error records.ParseError.UnknownLevel(warn)
 --> tests/test_records.au:24:13
   |
24 |             assert false, f"unexpected error {error}"
   |             ^
  = note: Aura call chain (innermost first): test_level_is_case_insensitive at 19:1
ok tests/test_report.au::test_report_lists_skipped_lines_with_reasons
ok tests/test_stats.au::test_counts_records_and_skips_bad_lines
ok tests/test_stats.au::test_min_level_filters_records
8 passed; 2 failed
```

Two failures with one cause, and the messages from our `assert false, ...`
lines say what the cause is: `warn` was rejected as a level, once on the
command line and once in a log line. This is where the `ParseError` and
`ConfigError` enums earn their keep in tests, too. The failure shows the exact
variant and its payload, not a bare "parse failed".

The fix belongs in one place, `parse_level`, and it is one word. Match on the
upper-cased text:

```aura fragment
public def parse_level(text: str) -> Level | None:
    match text.to_upper():
        case "DEBUG":
            return Level.Debug
```

`to_upper()` returns a new string, and the `match` compares that against the
literal cases. Run the tests again:

```console
$ aura test
ok tests/test_config.au::test_path_and_level
ok tests/test_config.au::test_level_defaults_to_debug
ok tests/test_config.au::test_rejects_unknown_flag
ok tests/test_config.au::test_missing_path
ok tests/test_records.au::test_parses_a_well_formed_line
ok tests/test_records.au::test_short_line_reports_field_count
ok tests/test_records.au::test_level_is_case_insensitive
ok tests/test_report.au::test_report_lists_skipped_lines_with_reasons
ok tests/test_stats.au::test_counts_records_and_skips_bad_lines
ok tests/test_stats.au::test_min_level_filters_records
10 passed; 0 failed
```

Both tests pass, because the command line and the parser share
`parse_level`. When you are working on one module, `-k` narrows the run to
test names containing a substring. Use the file name, since a word like
`records` also appears in some test names in `test_stats.au`:

```console
$ aura test -k test_records.au
ok tests/test_records.au::test_parses_a_well_formed_line
ok tests/test_records.au::test_short_line_reports_field_count
ok tests/test_records.au::test_level_is_case_insensitive
3 passed; 0 failed
```

## The finished tool

Run the refactored program against the sample log. The counts are the same as
before the split, and the skipped lines now explain themselves:

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

skipped line 7: expected at least 4 fields, found 3
skipped line 11: unknown level `NOTICE`
```

And as a native binary, which behaves the same and reads its arguments from
the real command line:

```console
$ aura build -o target/logscan src/main.au
aura: built `target/logscan` with direct backend
$ ./target/logscan app.log --min-level ERROR
app.log: 3 records, 2 skipped

by level
  ERROR       3

by service
  api         1
  db          1
  worker      1

errors by service
  api         1
  db          1
  worker      1

skipped line 7: expected at least 4 fields, found 3
skipped line 11: unknown level `NOTICE`
```

There is plenty left you could add. A `--service` option would be a few lines
in `config` and one more check in `summarize`. Reading from standard input
when no path is given would use `io.read_line()` in a loop, and only `main`
would need to change. Parsing timestamps would let the report show a time
range. The structure we have now means each of these touches one or two
modules and comes with a place to put its tests.

## Summary

You've built a complete command-line tool in Aura. It reads its arguments with
`sys.args()` and a file with `fs.read_to_string`, keeps the `Result`-returning
work in `run` and leaves exit statuses to `main`, and parses untrusted text
into typed `Record` values at the boundary. Along the way you met ownership in
everyday code: views and `.clone()` for strings in a list, `own` keys for a
dictionary, and `mut self` for a method that updates counts. You saw the
choice between `T | None` for an absence with nothing to say and
`Result[T, E]` with an error enum when the caller needs a reason, and you
split the program into modules with deliberate `public` boundaries and a test
file for each.

Several helpers in this chapter took a value and did one fixed thing with it.
The next chapter is about values that *are* behavior: closures that capture
their surroundings, such as a filter that remembers the minimum level it was
built with, and decorators that wrap a function once so every call goes
through the same code.
