# JSON

Two things about `checkup` are now awkward. The config format is ours, and it
cannot say much: an argument with a space in it cannot be written, because we
split on whitespace, and every new option would mean more parsing code. The
report is text meant for people, so a dashboard or a CI job that wants to know
which checks failed has to parse `name: detail` lines and hope nobody puts a
colon in a name. JSON fixes both. It is the format other tools already read
and write, and Aura's `json` module can convert between JSON text and your
own classes and enums, with errors that name the field that was wrong.

## Deriving a codec

Here is the configuration we want to be able to read. It has the same three
checks as before, plus a setting that the text format had no room for:

*Filename: checks.json*

```json
{
  "timeout_ms": 1500,
  "checks": [
    {"name": "config", "probe": {"type": "File", "path": "checks.json"}},
    {"name": "notes", "probe": {"type": "File", "path": "notes.txt"}},
    {"name": "greeting", "probe": {"type": "Command", "argv": ["/bin/echo", "hello"]}},
    {"name": "web", "probe": {"type": "Http", "url": "http://127.0.0.1:8080/health"}},
    {"name": "admin", "probe": {"type": "Http", "url": "http://127.0.0.1:8080/admin"}}
  ]
}
```

We could walk this document by hand, and later in this section we will, to
see what that is like. But the shape of the document is the shape of our
types, and Aura can write the conversion for us. We met the `json.Codec`
trait in [Chapter 13](/book/ch13-02-decorators), where it decoded a tool
call's arguments: it describes a type that can be encoded to JSON and decoded
from it, and writing an `impl` with no body asks the compiler to derive all of
it:

```aura
import json

enum Probe:
    File(path: str)
    Command(argv: list[str])
    Http(url: str)

impl json.Codec for Probe

class Check:
    name: str
    probe: Probe

impl json.Codec for Check

class Config:
    timeout_ms: int64 = 2000
    checks: list[Check]

impl json.Codec for Config
```

<span class="caption">Listing 16-12: Deriving JSON codecs for the config types</span>

`Probe`, `Check`, and `Config` each get three derived methods: `encode`, which
builds a `json.Value` tree from a value;
`decode`, which builds a value from a tree or explains why it cannot; and
`schema`, which returns a JSON Schema describing what `decode` accepts.

A class becomes a JSON object with one key per field. An enum whose variants
carry payloads becomes an object with a `"type"` key naming the variant and
one key per named payload, which is why our config says
`{"type": "Command", "argv": [...]}`. You can see it by encoding a value:

```aura fragment
print(json.dumps(json.encode(Probe.Command(argv=["/bin/echo", "hello world"]))))
```

```console
$ aura run probe.au
{"argv":["/bin/echo","hello world"],"type":"Command"}
```

That also solves the space problem: `"hello world"` is one element of `argv`
because JSON says so, not because of how we split a line.

Payloads have to be named for this to work. If `File` were declared as
`File(str)`, there would be no key to put the path under, and the compiler
says so:

```aura check-fail:AU2046
import json

enum Probe:
    File(str)
    Http(url: str)

impl json.Codec for Probe
```

```console
$ aura check pos.au
error[AU2046]: cannot derive `json.Codec` for `Probe`: variant `File` has a positional payload, which has no JSON key
 --> pos.au:4:5
  |
4 |     File(str)
  |     ^
  = help: name the payload, such as `File(value: str)`
```

The other types a derived codec refuses are the ones with no single JSON
form, such as sets, `Result`, callables, and `uint64`, and generic classes
and enums. The [JSON Module](/manual/json#derived-codecs)
page has the full table.

## Decoding the config

`json.decode_text[Config](text)` parses the text and decodes it in one step,
returning `Result[Config, json.DecodeError]`. Our `ConfigError` loses the
`BadLine` variant, which the decoder now covers, and gains one that carries
the decode error:

```aura fragment
enum ConfigError:
    Unreadable(path: str, error: io.Error)
    Invalid(path: str, error: json.DecodeError)

def load_config(path: str) -> Result[Config, ConfigError]:
    match own fs.read_to_string(path):
        case Result.Ok(text):
            match own json.decode_text[Config](text):
                case Result.Ok(config):
                    return Result.Ok(config)
                case Result.Err(error):
                    return Result.Err(ConfigError.Invalid(path=path.clone(), error=error))
        case Result.Err(error):
            return Result.Err(ConfigError.Unreadable(path=path.clone(), error=error))
```

That is the whole parser. The `parse_line` function and the line-counting
loop are gone.

The derived decoder is strict, and the strictness is the point. A key that
names no field is an error, not something to ignore. A missing field is an
error unless the field has a default. A value of the wrong JSON kind is an
error, and so is a number that does not fit the field's type. Here is
`checks.json` with one typo, `"address"` where `"url"` belongs:

```console
$ aura run src/main.au -- typo.json
typo.json: $.checks[3].probe: unknown field `address`
```

The error's `path` field, printed first, says exactly where the problem is:
`$` is the root of the document, `.checks` a key, `[3]` an array position. A
lenient decoder would have ignored `address`, then perhaps used an empty URL,
and the check would have failed at run time with a confusing connection
error. Here are a few more mistakes and what the decoder says about each,
taken from runs against small test documents:

```text
$.checks[0].probe.type: unknown variant `Ftp`
$.checks[0]: missing field `probe`
$: unknown field `timeout`
$.checks[0].name: expected string, found integer
$: invalid JSON at line 1, column 13: EOF while parsing a list
```

The last one is text that is not JSON at all. `decode_text` reports it with
path `$` and the parser's line and column.

`timeout_ms` has a default of `2000` in the class, so a config that leaves it
out still decodes. We turn it into a `Duration` with `Duration.ms(...)` and
pass that to `run_command` and `fetch` instead of the fixed `2s` we had
before. A setting that was hard-coded is now configurable, and adding the next
one is one field with a default.

## Encoding the report

The report gets the same treatment in the other direction. Two more classes
describe it, each with a derived codec:

```aura
import json

class Outcome:
    name: str
    passed: bool
    detail: str

impl json.Codec for Outcome

class Report:
    passed: int64
    failed: int64
    outcomes: list[Outcome]

impl json.Codec for Report
```

and saving it is `json.encode` to build the tree, `json.dumps` to turn the
tree into text, and the atomic `save` from earlier in the chapter:

```aura fragment
save("report.json", json.dumps(json.encode(report), indent=2))
```

Encoding reads the value and leaves it usable, which is why `main` can still
print `report.passed` afterwards.

Here is the complete program. Compared with Listing 16-4 and the additions of
the last two sections, the text parser is gone, the timeout comes from the
config, the report is JSON, and `main` prints only the failures and a
summary line. The exit status is `1` if any check failed, so `checkup` can
gate a CI job or a deploy script.

*Filename: src/main.au*

```aura
import fs
import io
import json
import net
import process
import sys

enum Probe:
    File(path: str)
    Command(argv: list[str])
    Http(url: str)

impl json.Codec for Probe

class Check:
    name: str
    probe: Probe

impl json.Codec for Check

class Config:
    timeout_ms: int64 = 2000
    checks: list[Check]

impl json.Codec for Config

class Outcome:
    name: str
    passed: bool
    detail: str

impl json.Codec for Outcome

class Report:
    passed: int64
    failed: int64
    outcomes: list[Outcome]

impl json.Codec for Report

enum ConfigError:
    Unreadable(path: str, error: io.Error)
    Invalid(path: str, error: json.DecodeError)

def load_config(path: str) -> Result[Config, ConfigError]:
    match own fs.read_to_string(path):
        case Result.Ok(text):
            match own json.decode_text[Config](text):
                case Result.Ok(config):
                    return Result.Ok(config)
                case Result.Err(error):
                    return Result.Err(ConfigError.Invalid(path=path.clone(), error=error))
        case Result.Err(error):
            return Result.Err(ConfigError.Unreadable(path=path.clone(), error=error))

def run_check(check: Check, timeout: Duration) -> str:
    match check.probe:
        case Probe.File(path):
            if fs.exists(path):
                return "ok"
            return "missing"
        case Probe.Command(argv):
            return run_command(argv, timeout)
        case Probe.Http(url):
            return fetch(url, timeout)

def run_command(argv: list[str], timeout: Duration) -> str:
    match own process.run(argv, timeout=timeout, group=true):
        case Result.Ok(completed):
            if completed.success():
                return "ok"
            return f"failed ({completed.status()})"
        case Result.Err(error):
            return f"error ({error})"

def fetch(url: str, timeout: Duration) -> str:
    headers: dict[str, str] = {}
    match own net.http_request_text_timeout("GET", url, "", headers, timeout):
        case Result.Ok(response):
            status = response.status()
            if status >= 200 and status < 400:
                return "ok"
            return f"failed (HTTP {status})"
        case Result.Err(error):
            return f"error ({error})"

def save(path: str, text: str) -> Result[None, io.Error]:
    temporary = f"{path}.tmp"
    with file = try fs.create(temporary):
        try file.write_all(text)
        try file.sync()
    try fs.rename(temporary, path)
    return Result.Ok(None)

def config_path() -> str:
    mut args = sys.args()
    if args.len() > 0:
        return args.pop(0)
    return "checks.json"

def main() -> int32:
    path = config_path()
    mut config = Config(checks=[])
    match own load_config(path):
        case Result.Ok(loaded):
            config = loaded
        case Result.Err(ConfigError.Unreadable(file, error)):
            print(f"cannot read {file}: {error}")
            return 2
        case Result.Err(ConfigError.Invalid(file, error)):
            print(f"{file}: {error}")
            return 2

    timeout = Duration.ms(config.timeout_ms)
    mut outcomes: list[Outcome] = []
    mut failed: int64 = 0
    for check in config.checks:
        detail = run_check(check, timeout)
        passed = detail == "ok"
        if not passed:
            failed += 1
            print(f"{check.name}: {detail}")
        outcomes.append(Outcome(name=check.name.clone(), passed=passed, detail=detail))

    report = Report(passed=outcomes.len() - failed, failed=failed, outcomes=outcomes)
    match save("report.json", json.dumps(json.encode(report), indent=2)):
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(f"cannot write report: {error}")
            return 1
    print(f"{report.passed} passed, {report.failed} failed")
    if failed > 0:
        return 1
    return 0
```

<span class="caption">Listing 16-13: `checkup` with a JSON config and a JSON report</span>

One line deserves a comment. `Outcome(name=check.name.clone(), ...)` clones
the name because the loop only has shared access to each `check`, and the
`Outcome` needs a name of its own. `detail` is a fresh string returned by
`run_check`, so it moves into the `Outcome` without a copy.

With the HTTP server from the previous section running:

```console
$ aura run src/main.au
notes: missing
admin: failed (HTTP 404)
3 passed, 2 failed
$ cat report.json
{
  "failed": 2,
  "outcomes": [
    {
      "detail": "ok",
      "name": "config",
      "passed": true
    },
    {
      "detail": "missing",
      "name": "notes",
      "passed": false
    },
    {
      "detail": "ok",
      "name": "greeting",
      "passed": true
    },
    {
      "detail": "ok",
      "name": "web",
      "passed": true
    },
    {
      "detail": "failed (HTTP 404)",
      "name": "admin",
      "passed": false
    }
  ],
  "passed": 3
}
```

The keys come out sorted, not in field order. `json.dumps` always sorts object
keys, so the same value always produces the same text. That makes reports
diffable and lets you use dumped JSON as a cache key or a test fixture
without worrying about dictionary order. `indent=2` gives two spaces per
level; leave it out for compact output on one line.

## Working with `json.Value` directly

A derived codec is the right tool when you know the shape. Sometimes you do
not, or you only care about a corner of a large document. For that, the
module has a dynamic tree. `json.parse(text)` returns
`Result[json.Value, json.Error]`, and `json.Value` is an ordinary enum with
seven variants: `Null`, `Bool`, `Int`, `Float`, `String`, `Array`, and
`Object`. Because it is an enum, you take it apart with `match`, and the
compiler checks that every variant is handled, either by name or by a `_`
arm.

Here is a separate program that reads `report.json` without knowing about our
classes and prints the names of the failed checks. It is what a monitoring
script written by somebody else might do.

*Filename: src/summary.au*

```aura
import fs
import json

def failed_name(entry: json.Value) -> str | None:
    match entry:
        case json.Value.Object(fields):
            match fields.get("passed"):
                case Lookup.Found(json.Value.Bool(false)):
                    match fields.get("name"):
                        case Lookup.Found(json.Value.String(name)):
                            return name.clone()
                        case _:
                            return "<unnamed>"
                case _:
                    return None
        case _:
            return None

def failed_names(report: json.Value) -> list[str]:
    mut names: list[str] = []
    match report:
        case json.Value.Object(fields):
            match fields.get("outcomes"):
                case Lookup.Found(json.Value.Array(outcomes)):
                    for entry in outcomes:
                        match own failed_name(entry):
                            case str as name:
                                names.append(name)
                            case None:
                                pass
                case _:
                    pass
        case _:
            pass
    return names

def main() -> int32:
    match own fs.read_to_string("report.json"):
        case Result.Ok(text):
            match json.parse(text):
                case Result.Ok(report):
                    print(failed_names(report))
                    return 0
                case Result.Err(error):
                    print(f"report.json is not JSON: {error}")
                    return 1
        case Result.Err(error):
            print(f"cannot read report.json: {error}")
            return 1
```

<span class="caption">Listing 16-14: Pulling failed names out of a report with `json.Value`</span>

```console
$ aura run src/summary.au
[notes, admin]
```

Compare it with `load_config`. Every level of the document is a `match`, and
every unexpected shape needs an arm. Patterns can nest, which helps:
`Lookup.Found(json.Value.Bool(false))` matches only when the key is present
and holds `false`. The matches are plain ones, because we only read the tree,
and the one string we keep is cloned out of it. This is about as short as
dynamic JSON gets in a typed language, and it is still far more code than
`impl json.Codec for Report`. Reach for `json.Value` when the shape really
is dynamic, and for a codec when it is not.

For inspecting values there are helpers that do not consume anything:
`json.as_int`, `json.as_float`, `json.as_bool`, and `json.is_null`. For taking
owned payloads out there are `json.into_string`, `json.into_array`, and
`json.into_object`, which consume the value and return the payload or `None`.

Numbers deserve a note, because JSON has only one number type and Aura has
two that matter here. The parser looks at the exact number in the text. If it
is a whole number that fits in an `int64`, it becomes `Value.Int`, however it
was spelled. Otherwise it becomes `Value.Float`:

```aura
import json

def main():
    match json.parse("{\"b\": 1.0, \"a\": [1.5, 1e2, -0.0, null, true], \"c\": \"hi\"}"):
        case Result.Ok(value):
            print(json.dumps(value))
        case Result.Err(error):
            print(error)
```

```console
$ aura run numbers.au
{"a":[1.5,100,0,null,true],"b":1,"c":"hi"}
```

`1.0` and `1e2` became the integers `1` and `100`, and `-0.0` became `0`.
Only `1.5` stayed a float. A value that came in as an integer never turns into
a rounded float by accident. `json.as_float` does not convert an `Int`, so a
field that may be written either way needs both cases handled.

Parse errors are values, because malformed input is normal at a boundary:

```aura
import json

def main():
    match json.parse("[1, 2,]"):
        case Result.Ok(value):
            print(json.dumps(value))
        case Result.Err(json.Error.Syntax(message, line, column)):
            print(f"{line}:{column}: {message}")
        case Result.Err(error):
            print(error)
```

```console
$ aura run trailing.au
1:7: expected value
```

The parser is strict JSON: no comments, no trailing commas, no `NaN`. The
other `json.Error` variants report numbers out of range, nesting deeper than
128 levels, and input larger than 64 MiB. `json.dumps`, on the other hand,
returns a plain `str`. Its failures, such as a `Float` holding infinity that
you built by hand, are bugs in the program rather than bad input, so they
trap instead of returning an error.

## Summary

`checkup` started as a program that echoed a text file and ends as a tool you
could run from cron or CI. Along the way you have used each of Aura's modules
for the outside world. `fs` reads and writes files, with `with` closing
handles and the temporary-file-and-rename recipe keeping readers from ever
seeing half a report. `process` runs commands from an explicit argv with no
shell in between, captures their output, talks to live children through pipes,
and supervises children with restart policies. `net` serves and calls HTTP
and TCP, keeping each socket on the task that opened it. And `json` turns
documents into your own types and back, with errors that point at a field.

The same three rules held throughout: failures are `Result` values with
specific error types, resources live in `with` blocks, and every wait has a
timeout. They are the reason `checkup` can tell "the command failed" from
"the command never started" from "the command is stuck", and why it leaves no
processes or open files behind on any path out. The full contracts are in the
Manual pages for [I/O](/manual/io), [the filesystem](/manual/filesystem),
[processes](/manual/process), [networking](/manual/network), and
[JSON](/manual/json).

So far we have run everything with `aura run`. In the next chapter we will
build `checkup` into a native binary you can copy to another machine, step
through it in a debugger, and look at what Aura code costs when it runs.
