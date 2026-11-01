# Recoverable errors with `Result`

When `parse_line` meets a line without `=`, the right response depends on who
is asking. A command-line tool should print a helpful message and exit with a
non-zero status. An editor plugin should underline the line. A server reloading
its configuration should keep the old settings and log a warning. `parse_line`
can't know which of these it's part of, so it shouldn't decide. Its job is to
say what went wrong and hand that answer back.

In Aura that answer is a value of type `Result[T, E]`. A `Result` is a generic
enum with two variants: `Result.Ok(value)` holds a successful value of type
`T`, and `Result.Err(error)` holds an error of type `E`. Because it's an
ordinary enum, everything you learned about enums and `match` in [Chapter
6](/book/ch06-00-enums-and-match) applies to it.

## Returning a `Result`

Let's change `parse_line` so a malformed line is an error the caller receives, instead of a trap:

*Filename: src/main.au*

```aura
def parse_line(line: str) -> Result[(str, str), str]:
    parts = line.split("=")
    if parts.len() != 2:
        return Result.Err(f"expected `key = value`, found `{line}`")
    return Result.Ok((parts[0].trim(), parts[1].trim()))

def main():
    lines = ["port = 9000", "workers = 4", "verbose"]
    for line in lines:
        match parse_line(line):
            case Result.Ok((key, value)):
                print(f"{key} -> {value}")
            case Result.Err(message):
                print(f"skipping line: {message}")
    print("loaded")
```

<span class="caption">Listing 9-2: `parse_line` returns a `Result` and `main` decides what to do with each outcome</span>

The return type now reads as a sentence: on success, a pair of strings; on
failure, a `str` explaining why. Before indexing, the function checks the one
thing that made Listing 9-1 trap, and returns `Result.Err` with a message if
the line doesn't split into exactly two parts. The success path wraps the pair
in `Result.Ok`.

```console
$ aura run src/main.au
port -> 9000
workers -> 4
skipping line: expected `key = value`, found `verbose`
loaded
```

This time the program reached the end. `main` chose to skip the bad line and
say so, which is a policy, and the policy lives in `main` where it belongs.

### The compiler keeps you honest

A `Result[(str, str), str]` is not a pair of strings, and Aura won't let you
pretend it is. If we keep the old line from Listing 9-1 that destructured the
result directly, the checker stops us:

```aura check-fail:AU2999
def parse_line(line: str) -> Result[(str, str), str]:
    parts = line.split("=")
    if parts.len() != 2:
        return Result.Err(f"expected `key = value`, found `{line}`")
    return Result.Ok((parts[0].trim(), parts[1].trim()))

def main():
    lines = ["port = 9000", "workers = 4", "verbose"]
    for line in lines:
        (key, value) = parse_line(line)
        print(f"{key} -> {value}")
```

```console
$ aura check src/main.au
error[AU2999]: tuple binding requires a tuple value, found `Result[(str, str), str]`
 --> src/main.au:10:10
   |
10 |         (key, value) = parse_line(line)
   |          ^
```

You have to look inside the `Result` before you can use the pair. And when you
do look with `match`, you have to handle both variants. Here's what happens if
we only write the case we're hoping for:

```aura check-fail:AU2999
def parse_line(line: str) -> Result[(str, str), str]:
    parts = line.split("=")
    if parts.len() != 2:
        return Result.Err(f"expected `key = value`, found `{line}`")
    return Result.Ok((parts[0].trim(), parts[1].trim()))

def main():
    match parse_line("verbose"):
        case Result.Ok((key, value)):
            print(f"{key} -> {value}")
```

```console
$ aura check src/main.au
error[AU2999]: non-exhaustive match over `Result`: missing `Err(_)`
 --> src/main.au:8:5
  |
8 |     match parse_line("verbose"):
  |     ^
```

This is the core difference from exceptions. In Python, the fact that
`int(text)` can raise `ValueError` lives in the documentation, and forgetting
about it costs nothing until the day it happens. In Aura, the possibility of
failure is part of the return type, so you can't reach the value without
passing the place where the error would be.

## Parsing returns a `Result` too

Our values are numbers, so the next step is turning `"9000"` into an `int64`.
The builtin parsers `parse_int32`, `parse_int64`, and `parse_float64` all
return a `Result` whose error is a `str`:

```aura
def main():
    print(parse_int64("9000"))
    print(parse_int64("90o0"))
    print(parse_int64(""))
```

```console
$ aura run src/main.au
Result.Ok(9000)
Result.Err(invalid digit found in string)
Result.Err(cannot parse integer from empty string)
```

Printing a `Result` shows its variant and payload, which is handy while you're
exploring. Let's combine the two steps into a function that turns a line into a
key and a number:

```aura
def parse_line(line: str) -> Result[(str, str), str]:
    parts = line.split("=")
    if parts.len() != 2:
        return Result.Err(f"expected `key = value`, found `{line}`")
    return Result.Ok((parts[0].trim(), parts[1].trim()))

def parse_setting(line: str) -> Result[(str, int64), str]:
    match own parse_line(line):
        case Result.Ok((key, text)):
            match own parse_int64(text):
                case Result.Ok(number):
                    return Result.Ok((key, number))
                case Result.Err(message):
                    return Result.Err(message)
        case Result.Err(message):
            return Result.Err(message)

def main():
    print(parse_setting("port = 9000"))
    print(parse_setting("port = nine"))
    print(parse_setting("verbose"))
```

<span class="caption">Listing 9-3: Passing errors up by hand with nested `match`</span>

```console
$ aura run src/main.au
Result.Ok((port, 9000))
Result.Err(invalid digit found in string)
Result.Err(expected `key = value`, found `verbose`)
```

Two details are new here. The first is `match own`. A plain `match` on a value
looks at it through shared access, which is fine when you only read the
payload, as `main` did in Listing 9-2 when it printed `message`. Here we want
to move `key` into a new `Result.Ok` and move `message` into a new
`Result.Err`, and you can only move something out of a value you own. `match
own` consumes the scrutinee, so its payloads become owned bindings we're free
to hand on. If you leave `own` off, the compiler names the problem and suggests
both fixes: `match own`, or `.clone()` to move a copy instead. We'll see that
diagnostic in full a little later.

The second detail is the shape. Most of Listing 9-3 is plumbing: unwrap the
success, or return the error unchanged. The two lines that do the actual work
are buried in it. That pattern is so common that Aura has an operator for it.

## Propagating errors with `try`

`try expr` evaluates `expr`, which must produce a `Result`. If it's
`Result.Ok(value)`, the `try` expression produces `value` and execution
continues. If it's `Result.Err(error)`, the current function returns
`Result.Err(error)` immediately. Here's Listing 9-3 again with `try`:

```aura
def parse_line(line: str) -> Result[(str, str), str]:
    parts = line.split("=")
    if parts.len() != 2:
        return Result.Err(f"expected `key = value`, found `{line}`")
    return Result.Ok((parts[0].trim(), parts[1].trim()))

def parse_setting(line: str) -> Result[(str, int64), str]:
    (key, text) = try parse_line(line)
    number = try parse_int64(text)
    return Result.Ok((key, number))

def main():
    print(parse_setting("port = 9000"))
    print(parse_setting("port = nine"))
    print(parse_setting("verbose"))
```

<span class="caption">Listing 9-4: The same function with `try`</span>

The output is identical. `parse_setting` now reads top to bottom as the steps
it performs, and each `try` marks a place where it might return early. That
last part matters. A Python function can stop at any call that raises, and
nothing at the call site tells you. In Aura every early exit is either a
`return` or a `try`, so you can find all of them by reading the function.

`try` can't make a function fallible behind its caller's back. It only works
inside a function whose return type is a `Result`:

```aura check-fail:AU2999
def main():
    number = try parse_int64("9000")
    print(number)
```

```console
$ aura check src/main.au
error[AU2999]: `try` requires the enclosing function to return `Result`, found `None`
 --> src/main.au:2:14
  |
2 |     number = try parse_int64("9000")
  |              ^
```

This `main` returns nothing, so a failed parse would have nowhere to go. At the
top of the program there's no caller left to pass the error to, which is why
`main` uses `match` and decides for itself.

## An error type of our own

So far every error has been a `str`. Strings are fine for a quick program, but
they have a cost: the only thing a caller can do with a string is show it to
someone. A caller that wants to treat a missing setting differently from a typo
has to search the message text, and the message text isn't a contract.

The better tool is an enum that lists the ways our loader can fail. Let's also
record line numbers, because "line 2: expected `key = value`" is far more
useful to whoever edits the file:

```aura
enum ConfigError:
    MissingEquals(int64)
    BadNumber(int64, str)
    MissingKey(str)
```

`MissingEquals` carries the line number. `BadNumber` carries the line number
and the parser's message. `MissingKey` names a required setting that never
appeared. Each variant holds exactly the data needed to describe that failure,
and nothing more.

Now `parse_line` returns `Result[(str, str), ConfigError]`. When we try to use
`try` on `parse_int64` in a function that returns that type, the compiler
objects:

```aura check-fail:AU2002
enum ConfigError:
    MissingEquals(int64)
    BadNumber(int64, str)

def parse_line(number: int64, line: str) -> Result[(str, str), ConfigError]:
    parts = line.split("=")
    if parts.len() != 2:
        return Result.Err(ConfigError.MissingEquals(number))
    return Result.Ok((parts[0].trim(), parts[1].trim()))

def parse_setting(number: int64, line: str) -> Result[(str, int64), ConfigError]:
    (key, text) = try parse_line(number, line)
    value = try parse_int64(text)
    return Result.Ok((key, value))
```

```console
$ aura check src/main.au
error[AU2002]: `try` error type `str` does not match enclosing `Result` error type `ConfigError`
 --> src/main.au:13:13
   |
13 |     value = try parse_int64(text)
   |             ^
```

The first `try` is fine, because `parse_line` already returns a `ConfigError`.
The second would return a `str` from a function that promised a `ConfigError`.
`try` never changes an error's type silently, so we have two ways forward.

The first is to convert the error ourselves, at the point where we know the
line number. A small helper does it:

```aura check-fail:AU3002
enum ConfigError:
    MissingEquals(int64)
    BadNumber(int64, str)

def parse_value(number: int64, text: str) -> Result[int64, ConfigError]:
    match parse_int64(text):
        case Result.Ok(value):
            return Result.Ok(value)
        case Result.Err(message):
            return Result.Err(ConfigError.BadNumber(number, message))
```

```console
$ aura check src/main.au
error[AU3002]: cannot move `message` out of a shared match on `parse_int64(...)`
 --> src/main.au:10:61
   |
10 |             return Result.Err(ConfigError.BadNumber(number, message))
   |                                                             ^
  = related src/main.au:9:25: value is borrowed here
  = help: write `match own parse_int64(...)` to consume the scrutinee, or call `.clone()` to consume an independent copy
  = fix: replace src/main.au:10:68-10:68 with `.clone()`
```

This is the diagnostic promised earlier. We forgot `own`, so `message` is only
borrowed from the `Result`, and we're trying to move it into a new enum value.
The `related` line points at the binding, and the help line offers `match own`
or a clone. The parser's `Result` is a temporary we never use again, so
consuming it is the right fix: write `match own parse_int64(text):` and the
helper compiles.

The second way is to teach Aura how to turn a `str` into a `ConfigError`, so
`try` can do the conversion for us. When the error types differ, `try` looks
for an implementation of a trait named `From` whose source is the inner error
type. Aura doesn't predeclare `From`, so a module that wants this conversion
declares the trait itself and then implements it:

```aura
trait From[Source]:
    def from(value: own Source) -> Self

enum ConfigError:
    MissingEquals(int64)
    BadNumber(int64, str)
    Invalid(str)

impl From[str] for ConfigError:
    def from(value: own str) -> ConfigError:
        return ConfigError.Invalid(value)

def parse_line(number: int64, line: str) -> Result[(str, str), ConfigError]:
    parts = line.split("=")
    if parts.len() != 2:
        return Result.Err(ConfigError.MissingEquals(number))
    return Result.Ok((parts[0].trim(), parts[1].trim()))

def parse_setting(number: int64, line: str) -> Result[(str, int64), ConfigError]:
    (key, text) = try parse_line(number, line)
    value = try parse_int64(text)
    return Result.Ok((key, value))

def main():
    print(parse_setting(1, "port = 9000"))
    print(parse_setting(1, "port = nine"))
    print(parse_setting(2, "verbose"))
```

```console
$ aura run src/main.au
Result.Ok((port, 9000))
Result.Err(ConfigError.Invalid(invalid digit found in string))
Result.Err(ConfigError.MissingEquals(2))
```

Now `try parse_int64(text)` compiles. On failure, Aura calls `ConfigError.from`
with the `str` error and returns the converted value. Look at what we lost,
though. `from` receives only the string, so the `Invalid` variant has no line
number. `From` is the right tool when one error type maps naturally onto
another, such as a module's own I/O error wrapped into an application error.
When the conversion needs context that only the caller has, a helper like
`parse_value` keeps that context. We'll use the helper. Traits, including how
`From` fits into conversions generally, are the subject of [Chapter
10](/book/ch10-00-generics-traits-conversions).

## Looking up a required key

Once each line is parsed, we need to check that the required settings were
present. We'll collect the values in a `dict[str, int64]` and look up each
required key.

You already know from [Chapter 8](/book/ch08-03-dicts-and-sets) that
`values["port"]` traps when the key is missing, and that `get` returns a
`Lookup[T]` instead: `Lookup.Found(value)` or `Lookup.Missing`. `Lookup` isn't
a `Result`, because a missing key isn't an error in itself. It's a fact about
the dictionary, and the caller decides whether it's a problem:

```aura fragment
def require(values: dict[str, int64], key: own str) -> Result[int64, ConfigError]:
    match values.get(key):
        case Lookup.Found(value):
            return Result.Ok(value)
        case Lookup.Missing:
            return Result.Err(ConfigError.MissingKey(key))
```

For our loader a missing `port` is an error, so `require` turns
`Lookup.Missing` into `ConfigError.MissingKey`. The parameter is `own str`
because the error keeps the key; when we call `require(values, "port")`, the
string literal moves in.

This is also why `get` doesn't return `int64 | None`, even though the optional
values from Chapter 6 are the right type for "maybe there's a value" in your
own code. A dictionary can store optional values too. If a `dict[str, int64 |
None]` holds `None` for some key, `get` must be able to tell you "the key is
present and its value is `None`", which is `Lookup.Found(None)`, apart from
"the key isn't there", which is `Lookup.Missing`. A plain `T | None` would
collapse those two answers into one.

## The finished loader

Here is the whole program, with the error enum, both helpers, and a `main` that
runs four sample files:

*Filename: src/main.au*

```aura
enum ConfigError:
    MissingEquals(int64)
    BadNumber(int64, str)
    MissingKey(str)

class Settings:
    port: int64
    workers: int64

def parse_line(number: int64, line: str) -> Result[(str, str), ConfigError]:
    parts = line.split("=")
    if parts.len() != 2:
        return Result.Err(ConfigError.MissingEquals(number))
    return Result.Ok((parts[0].trim(), parts[1].trim()))

def parse_value(number: int64, text: str) -> Result[int64, ConfigError]:
    match own parse_int64(text):
        case Result.Ok(value):
            return Result.Ok(value)
        case Result.Err(message):
            return Result.Err(ConfigError.BadNumber(number, message))

def require(values: dict[str, int64], key: own str) -> Result[int64, ConfigError]:
    match values.get(key):
        case Lookup.Found(value):
            return Result.Ok(value)
        case Lookup.Missing:
            return Result.Err(ConfigError.MissingKey(key))

def parse_settings(lines: list[str]) -> Result[Settings, ConfigError]:
    mut values: dict[str, int64] = {}
    for (index, line) in enumerate(lines):
        number = index + 1
        (key, text) = try parse_line(number, line)
        values[key] = try parse_value(number, text)
    port = try require(values, "port")
    workers = try require(values, "workers")
    return Result.Ok(Settings(port=port, workers=workers))

def describe(error: ConfigError) -> str:
    match error:
        case ConfigError.MissingEquals(number):
            return f"line {number}: expected `key = value`"
        case ConfigError.BadNumber(number, message):
            return f"line {number}: value is not a number ({message})"
        case ConfigError.MissingKey(key):
            return f"missing required setting `{key}`"

def load(lines: list[str]) -> int32:
    match parse_settings(lines):
        case Result.Ok(settings):
            print(f"listening on {settings.port} with {settings.workers} workers")
            return 0
        case Result.Err(error):
            print(f"config error: {describe(error)}")
            return 2

def main() -> int32:
    samples = [
        ["port = 9000", "workers = 4"],
        ["port = 9000", "verbose"],
        ["port = 90o0", "workers = 4"],
        ["port = 9000"],
    ]
    mut status: int32 = 0
    for sample in samples:
        if load(sample) != 0:
            status = 2
    return status
```

<span class="caption">Listing 9-5: A settings loader where every input mistake is a `ConfigError`</span>

```console
$ aura run src/main.au; echo $?
listening on 9000 with 4 workers
config error: line 2: expected `key = value`
config error: line 1: value is not a number (invalid digit found in string)
config error: missing required setting `workers`
2
```

Read `parse_settings` again and notice how little error handling it seems to
contain. Four `try` expressions mark the four places it can stop early, and
every one of them returns a `ConfigError` the caller can inspect. `describe`
turns each variant into a message, and because it's a `match` over the enum,
adding a fifth variant later will make the compiler point at `describe` until
we handle it.

`load` is where the policy lives. It prints the error and returns `2`, and
`main` passes a non-zero status back to the shell when any sample failed. A
`main` declared `-> int32` sets the process exit status to the value it
returns, which lets a script tell "the settings file was wrong" (status 2, from
our code) apart from "the program hit a bug" (status 1, from a trap).

The loader no longer traps on anything a user could type into a settings file.
It still contains code that could trap, though: `parts[0]` in `parse_line`
would trap on an empty list, and `index + 1` would trap on overflow. Those
aren't input problems. `split` always returns at least one part, and no list is
long enough for its index to overflow. Deciding which failures belong in a
`ConfigError` and which can safely stay traps is the question of the next
section.
