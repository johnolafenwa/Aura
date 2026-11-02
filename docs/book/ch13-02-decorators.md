# Decorators

The last section ended with `slower_than`, a function that took a
limit and returned a closure built around it. Now suppose the thing we hand in
is not a number but another function, and what we get back is a new function
that does a little extra work around each call to the old one: print a trace
line, count the calls, retry on failure. Writing that by hand at every place we
define a function would be tedious and easy to forget. A *decorator* does it
once, at the definition.

We'll build up to a small typed tool runner, the core of a program that an
AI agent or a script talks to. It receives a tool call as JSON text, such as
`{"tool": "count", "args": {"level": "Error"}}`, decodes the arguments into a
class, runs the matching tool, and replies with JSON. The decorator puts the
same tracing around every tool, and derived JSON codecs do the decoding and
encoding, including precise errors for calls that don't match.

## A first decorator

Here is a decorator that traces each call of a function that takes a line of
text and returns a number:

*Filename: tools.au*

```aura
type Handler = Callable[def(line: str) -> int64]

def traced(f: own Handler) -> Handler:
    print("wrapping a handler")
    return Handler(lambda [own f] line: report(f, line))

def report(f: Handler, line: str) -> int64:
    print(f"-> {line}")
    result = f(line)
    print(f"<- {result}")
    return result

@traced
def field_count(line: str) -> int64:
    return line.split_whitespace().len()

def main():
    print("main starts")
    field_count("2026-10-31T09:14:09 ERROR db connection reset")
    field_count("connection pool exhausted")
```

<span class="caption">Listing 13-7: A decorator that wraps a function with trace output</span>

```console
$ aura run tools.au
wrapping a handler
main starts
-> 2026-10-31T09:14:09 ERROR db connection reset
<- 5
-> connection pool exhausted
<- 3
```

The line `@traced` above `def field_count` means: define `field_count` as
written, then call `traced` with it, and let the name `field_count` refer to
whatever `traced` returns. Every later call of `field_count`, including from
other modules, goes through the wrapper.

Read the output from the top. `wrapping a handler` appears once, *before*
`main starts`. Decorators run when the module initializes, in the order the
decorated functions are declared, not on each call. Each call then goes
through the closure that `traced` returned, which is why every call prints its
`->` and `<-` lines.

Everything inside `traced` is something we met in the last section.
`Handler` is an owned callable type, `Callable[def(line: str) -> int64]`, and
the decorated name `field_count` has exactly this type, because Aura builds it
from the function's parameter names, modes, and result. `traced` takes `f`
with `own` because the closure it returns has to keep `f`, and the capture
list `[own f]` moves the original function into that closure. Since a
lambda's body is one expression, the real work happens in `report`, a named
function that receives the captured `f` and the call's argument. This split
between a one-line lambda and a helper is the usual shape of an Aura
decorator.

That `own` is required, and if you forget it the compiler says what to write:

```aura check-fail:AU3002
type Handler = Callable[def(line: str) -> int64]

def traced(f: Handler) -> Handler:
    print("wrapping a handler")
    return Handler(lambda [own f] line: report(f, line))

def report(f: Handler, line: str) -> int64:
    print(f"-> {line}")
    result = f(line)
    print(f"<- {result}")
    return result

@traced
def field_count(line: str) -> int64:
    return line.split_whitespace().len()

def main():
    print("main starts")
    field_count("2026-10-31T09:14:09 ERROR db connection reset")
    field_count("connection pool exhausted")
```

```console
$ aura check tools.au
error[AU3002]: parameter `f` is borrowed; declare it as `own Callable[def(line: str) -> int64]` to take ownership
 --> tools.au:5:32
  |
5 |     return Handler(lambda [own f] line: report(f, line))
  |                                ^
  = related tools.au:3:12: parameter `f` is borrowed here
  = help: declare the parameter as `own Callable[def(line: str) -> int64]` when the function should consume this non-cloneable value
```

A bare parameter only borrows `f` for the duration of the call to `traced`, but
the closure we return outlives that call. A packed callable can't be cloned
either, so the only way for the wrapper to have `f` later is to own it.

## What a decorator can wrap

Because the decorator's type names the parameter, the decorated function must
use the same name. Rename `line` to `text` in `field_count` and the decorator
no longer fits:

```aura check-fail:AU2015
type Handler = Callable[def(line: str) -> int64]

def traced(f: own Handler) -> Handler:
    print("wrapping a handler")
    return Handler(lambda [own f] line: report(f, line))

def report(f: Handler, line: str) -> int64:
    print(f"-> {line}")
    result = f(line)
    print(f"<- {result}")
    return result

@traced
def field_count(text: str) -> int64:
    return text.split_whitespace().len()

def main():
    print("main starts")
    field_count("2026-10-31T09:14:09 ERROR db connection reset")
    field_count("connection pool exhausted")
```

```console
$ aura check tools.au
error[AU2015]: decorator on `field_count` must take and return `Callable[def(text: str) -> int64]`: callable contract mismatch: parameter `text` would be renamed to `line`; renaming requires an explicit wrapper
 --> tools.au:13:1
   |
13 | @traced
   | ^
  = help: a decorator's callable type names each parameter, so the decorated function must use the same names
```

The message states the rule in its first clause: a decorator on `field_count`
must take and return a callable with `field_count`'s exact contract. Here the
contract has a parameter named `text`, and `Handler` has one named `line`.
This is the same contract rule as in the last section. A caller could write
`field_count(text="...")`, and a wrapper that only answers to `line` would
break that call, so the compiler insists the two agree.

Some functions can't be decorated at all. A parameter with a default is one:

```aura check-fail:AU2047
type Handler = Callable[def(line: str) -> int64]

def traced(f: own Handler) -> Handler:
    print("wrapping a handler")
    return Handler(lambda [own f] line: report(f, line))

def report(f: Handler, line: str) -> int64:
    print(f"-> {line}")
    result = f(line)
    print(f"<- {result}")
    return result

@traced
def field_count(line: str, sep: str = " ") -> int64:
    return line.split_whitespace().len()

def main():
    print("main starts")
    field_count("2026-10-31T09:14:09 ERROR db connection reset")
    field_count("connection pool exhausted")
```

```console
$ aura check tools.au
error[AU2047]: decorated function `field_count` cannot give parameter `sep` a default
 --> tools.au:14:28
   |
14 | def field_count(line: str, sep: str = " ") -> int64:
   |                            ^
  = help: a decorator returns a closure, and a closure has no parameter defaults
```

The help line gives the reason. What replaces a decorated function is a
closure, and closures have no default values for their parameters, so a
caller who left out `sep` would have nothing to fall back on. The same check,
`AU2047`, rejects decorating `main`, a generic function, or a function that
returns a view. Decorators also apply only to module-level functions, not to
methods, classes, or functions inside other functions.

Python programmers sometimes use a decorator to register a function in a
global table as a side effect. That doesn't carry over, because Aura modules
have no mutable state:

```aura check-fail:AU3003
mut calls = 0

def main():
    calls += 1
```

```console
$ aura check tools.au
error[AU3003]: module bindings are immutable; `mut` module state is not supported
 --> tools.au:1:5
  |
1 | mut calls = 0
  |     ^
  = help: put mutable state in a local value owned by `main` or another explicit owner
```

A decorator in Aura transforms one function and nothing else. When we need a
table of tools later, we'll write it out explicitly.

## Stacking decorators

More than one decorator can sit above a function. The decorator expressions
are evaluated top to bottom, and the results are applied bottom to top, so the
one nearest `def` wraps the function first. A decorator can also be a call,
such as `@tag("outer")`, that returns the real decorator. This program shows
both orders at once:

*Filename: tools.au*

```aura
type Step = Callable[def(n: int64) -> int64]
type Wrapper = Callable[def(f: own Step) -> Step]

def tag(label: own str) -> Wrapper:
    print(f"evaluate {label}")
    return Wrapper(lambda [own label] own f: wrap(label.clone(), f))

def wrap(label: own str, f: own Step) -> Step:
    print(f"apply {label}")
    return Step(lambda [own label, own f] n: show(label, f, n))

def show(label: str, f: Step, n: int64) -> int64:
    print(f"call {label}")
    return f(n)

@tag("outer")
@tag("inner")
def double(n: int64) -> int64:
    return n * 2

def main():
    print(double(21))
```

<span class="caption">Listing 13-8: A decorator factory, stacked twice</span>

```console
$ aura run tools.au
evaluate outer
evaluate inner
apply inner
apply outer
call outer
call inner
42
```

`tag` is a *decorator factory*: it takes a label and returns a `Wrapper`, a
callable that takes a `Step` and returns a `Step`. Both `tag` calls run first,
in source order. Then the inner wrapper is applied to `double` and the outer
wrapper to that result. At call time, the outermost wrapper runs first, so the
trace reads `call outer`, then `call inner`, then the real `double`.

The lambda inside `tag` is worth reading slowly:
`lambda [own label] own f: wrap(label.clone(), f)`. It captures `label`, and it
takes its own parameter `f` with `own`, matching `Wrapper`'s `own Step`. Its
body clones `label` before passing it on. That clone matters: if the lambda
moved `label` out, it would become a consuming closure, callable once, and
`Wrapper` describes a callable that can be called any number of times.

## One decorator for every tool

`traced` and `tag` work only for one exact signature. A tool runner has many
tools with different argument and result types, and we don't want a separate
tracing decorator for each. A generic decorator solves this. Its type
parameters are inferred from each function it decorates:

*Filename: tools.au*

```aura
type Tool[A, R] = Callable[def(args: A) -> R]
type Decorator[A, R] = Callable[def(f: own Tool[A, R]) -> Tool[A, R]]

def traced[A, R](name: own str) -> Decorator[A, R]:
    return Decorator[A, R](lambda [own name] own f: wrap(name.clone(), f))

def wrap[A, R](name: own str, f: own Tool[A, R]) -> Tool[A, R]:
    return Tool[A, R](lambda [own name, own f] args: call_traced(name, f, args))

def call_traced[A, R](name: str, f: Tool[A, R], args: A) -> R:
    print(f"call {name}")
    return f(args)

@traced("fields")
def field_count(args: str) -> int64:
    return args.split_whitespace().len()

@traced("double")
def double(args: int64) -> int64:
    return args * 2

def main():
    print(field_count("connection pool exhausted"))
    print(double(21))
```

<span class="caption">Listing 13-9: A generic decorator factory that fits any one-argument tool</span>

```console
$ aura run tools.au
call fields
3
call double
42
```

`traced("fields")` can't work out `A` and `R` from its own argument, a string.
The function under it supplies them: for `field_count`, `A` is `str` and `R` is
`int64`; for `double`, both are `int64`. Each use is inferred on its own.

A type parameter stands for a type, never for a whole parameter list, so
`Tool[A, R]` still fixes the shape of the function: exactly one parameter,
named `args`. That is why both tools call their parameter `args`. A tool that
names it anything else gets the same `AU2015` we saw earlier:

```console
$ aura check tools.au
error[AU2015]: decorator on `double` must take and return `Callable[def(n: int64) -> int64]`: callable contract mismatch: parameter `n` would be renamed to `args`; renaming requires an explicit wrapper
 --> tools.au:18:1
   |
18 | @traced("double")
   | ^
  = help: a decorator's callable type names each parameter, so the decorated function must use the same names
```

One parameter called `args` sounds limiting until you make `A` a class. A tool
that needs a level and an optional service takes one argument of a class with
those two fields. And a class is something the compiler can teach to read and
write JSON.

## Deriving a JSON codec

The `json` module's `json.Codec` trait describes a type that converts to and
from `json.Value`, the module's tree of JSON data. We'll use only a few of the
module's functions here; [Chapter 16](/book/ch16-04-json) covers it in full. You can implement it by
hand, but for an ordinary class or enum you write the `impl` line with no body
and the compiler writes the implementation:

*Filename: tools.au*

```aura
import json

class CountArgs:
    """Counts log records at one level, optionally for one service."""

    level: str
    service: str | None = None

impl json.Codec for CountArgs

def main():
    inputs = [
        '{"level": "ERROR", "service": "db"}',
        '{"level": "ERROR"}',
        '{"level": 3}',
        '{"service": "db"}',
        '{"level": "ERROR", "limit": 5}',
        '{"level": ',
    ]
    for text in inputs:
        match own json.decode_text[CountArgs](text):
            case Ok(args):
                print(f"ok level={args.level} service={args.service}")
            case Err(error):
                print(f"error {error}")
    print(json.dumps(json.encode(CountArgs(level="WARN", service=None))))
```

<span class="caption">Listing 13-10: Decoding and encoding a class through a derived codec</span>

```console
$ aura run tools.au
ok level=ERROR service=db
ok level=ERROR service=None
error $.level: expected string, found integer
error $: missing field `level`
error $: unknown field `limit`
error $: invalid JSON at line 1, column 11: EOF while parsing a value
{"level":"WARN","service":null}
```

One empty `impl` gave `CountArgs` three things: `encode`, which turns a value
into a JSON tree; `decode`, which goes the other way; and `schema`, which we'll
come to shortly. `json.decode_text[CountArgs](text)` parses the text and
decodes it in one step, returning `Result[CountArgs, json.DecodeError]`.

The decoder is strict, and each error names the place in the document it was
reading, as a path from the root `$`:

- `{"level": 3}` has the right key with the wrong kind of value, so the error
  is at `$.level`.
- `{"service": "db"}` leaves out `level`, which has no default, so it is a
  missing field.
- `{"level": "ERROR", "limit": 5}` has a key `CountArgs` doesn't declare.
  Rather than ignore it, the decoder rejects it as an unknown field. That
  catches a misspelled key, which would otherwise be silently dropped and
  replaced with a default.
- `{"level": ` isn't JSON at all, so the error is a parse error at the root,
  with a line and column.

`service` is declared `str | None = None`. The `= None` default is what makes
it optional in the input; a `T | None` field without a default must still be
present, even if its value is `null`. On the way out, `encode` writes every
field, so `None` comes out as `null`.

Decoding stops at the first error. The result is either a complete, valid
`CountArgs` or an error, never a half-filled value, which is the property we
want at a boundary where text from outside enters the program.

The field types themselves need a JSON form. Strings, numbers, booleans,
lists, `dict[str, T]`, optionals, and other classes and enums with a codec all
have one. A field of a class without a codec stops the derivation:

```aura check-fail:AU2002
import json

class Window:
    start: str
    end: str

class CountArgs:
    level: str
    window: Window

impl json.Codec for CountArgs
```

```console
$ aura check tools.au
error[AU2002]: derived `json.Codec` for `CountArgs`: type `Window` does not implement trait `json.Codec`
 --> tools.au:11:1
   |
11 | impl json.Codec for CountArgs
   | ^
```

Adding `impl json.Codec for Window` fixes it. The [Manual](/manual/json#derived-codecs)
lists every field type and its JSON form.

## Telling a caller what a tool accepts

The third derived method is `schema`. It returns a JSON Schema document that
describes exactly what `decode` accepts:

```aura
import json

class CountArgs:
    """Counts log records at one level, optionally for one service."""

    level: str
    service: str | None = None

impl json.Codec for CountArgs

def main():
    print(json.dumps(json.schema[CountArgs](), indent=2))
```

```console
$ aura run tools.au
{
  "additionalProperties": false,
  "description": "Counts log records at one level, optionally for one service.",
  "properties": {
    "level": {
      "type": "string"
    },
    "service": {
      "anyOf": [
        {
          "type": "string"
        },
        {
          "type": "null"
        }
      ]
    }
  },
  "required": [
    "level"
  ],
  "type": "object"
}
```

The class docstring became the schema's `description`. Only `level` is
`required`, because `service` has a default, and `additionalProperties` is
`false`, matching the decoder's refusal of unknown keys. This is the document
you would hand to a language model or another program that wants to call the
tool: because it is derived from the same class as the decoder, the two can't
disagree about what a valid call looks like.

An enum without payloads encodes as its variant name. That lets us use our
`Level` enum from Chapter 12 directly as a field type instead of a string, and
the schema then lists the allowed names, `"Debug"`, `"Info"`, `"Warn"`, and
`"Error"`. A caller that sends `"ERROR"` gets an error rather than a level we
didn't define.

## The tool runner

Now we can put the pieces together. Each tool is a decorated function that
takes one argument class and returns one result class, both with derived
codecs. A `ToolCall` class decodes the outer envelope. Its `args` field has
the type `json.Value`, which passes whatever JSON it finds through unchanged,
so the envelope can be decoded before we know which tool's argument class to
use.

*Filename: tools.au*

```aura
import json

enum Level:
    Debug
    Info
    Warn
    Error

impl json.Codec for Level

class Record:
    level: Level
    service: str

class CountArgs:
    """Counts log records at one level, optionally for one service."""

    level: Level
    service: str | None = None

impl json.Codec for CountArgs

class Count:
    matched: int64

impl json.Codec for Count

class ServicesArgs:
    """Lists the services that logged at one level."""

    level: Level

impl json.Codec for ServicesArgs

class Services:
    names: list[str]

impl json.Codec for Services

class ToolCall:
    tool: str
    args: json.Value

impl json.Codec for ToolCall

type Tool[A, R] = Callable[def(args: A) -> R]
type Decorator[A, R] = Callable[def(f: own Tool[A, R]) -> Tool[A, R]]

def traced[A, R](name: own str) -> Decorator[A, R]:
    return Decorator[A, R](lambda [own name] own f: wrap(name.clone(), f))

def wrap[A, R](name: own str, f: own Tool[A, R]) -> Tool[A, R]:
    return Tool[A, R](lambda [own name, own f] args: call_traced(name, f, args))

def call_traced[A, R](name: str, f: Tool[A, R], args: A) -> R:
    print(f"call {name}")
    return f(args)

def sample() -> list[Record]:
    return [
        Record(level=Level.Info, service="api"),
        Record(level=Level.Error, service="api"),
        Record(level=Level.Error, service="db"),
        Record(level=Level.Warn, service="db"),
        Record(level=Level.Error, service="worker"),
    ]

@traced("count")
def count(args: CountArgs) -> Count:
    mut matched = 0
    for record in sample():
        if record.level != args.level:
            continue
        match args.service:
            case str as wanted:
                if record.service == wanted:
                    matched += 1
            case None:
                matched += 1
    return Count(matched=matched)

@traced("services")
def services(args: ServicesArgs) -> Services:
    mut names: list[str] = []
    for record in sample():
        if record.level == args.level and record.service not in names:
            names.append(record.service.clone())
    return Services(names=names)

def run[A: json.Codec, R: json.Codec](f: Tool[A, R], args: own json.Value) -> Result[json.Value, str]:
    match own json.decode[A](args):
        case Ok(decoded):
            return Ok(json.encode(f(decoded)))
        case Err(error):
            return Err(f"bad arguments: {error}")

def dispatch(text: str) -> Result[json.Value, str]:
    match own json.decode_text[ToolCall](text):
        case Ok(call):
            if call.tool == "count":
                return run(count, call.args)
            if call.tool == "services":
                return run(services, call.args)
            return Err(f"unknown tool `{call.tool}`")
        case Err(error):
            return Err(f"bad call: {error}")

def main():
    calls = [
        '{"tool": "count", "args": {"level": "Error"}}',
        '{"tool": "count", "args": {"level": "Error", "service": "db"}}',
        '{"tool": "services", "args": {"level": "Error"}}',
        '{"tool": "count", "args": {"level": "ERROR"}}',
        '{"tool": "count", "args": {"level": "Error", "limit": 5}}',
        '{"tool": "grep", "args": {}}',
        '{"args": {}}',
    ]
    for text in calls:
        match dispatch(text):
            case Ok(value):
                print(f"result {json.dumps(value)}")
            case Err(message):
                print(f"error {message}")
```

<span class="caption">Listing 13-11: A typed tool runner with decorated tools and derived codecs</span>

```console
$ aura run tools.au
call count
result {"matched":3}
call count
result {"matched":1}
call services
result {"names":["api","db","worker"]}
error bad arguments: $.level: unknown variant `ERROR`
error bad arguments: $: unknown field `limit`
error unknown tool `grep`
error bad call: $: missing field `tool`
```

Let's trace one call through the program. `dispatch` decodes the text as a
`ToolCall`. If the envelope is malformed, as in the last call that has no
`tool` key, it stops there with the decoder's message. Otherwise it picks the
tool by name and hands it, with the still-undecoded `args`, to `run`.

`run` is generic over the argument type `A` and the result type `R`, and its
bounds `A: json.Codec` and `R: json.Codec` say that both must have codecs. The
type arguments are inferred from the tool we pass: `run(count, call.args)`
fixes `A = CountArgs` and `R = Count`. So `json.decode[A](args)` decodes the
arguments as exactly the class `count` expects, and
`json.encode(f(decoded))` encodes whatever the tool returned. `f` is a bare
parameter, so `run` borrows the decorated binding for the call and leaves it
in place for the next one.

Each successful call prints `call count` or `call services` before its result.
Those lines come from `traced`, and neither tool contains any tracing code. The
two bad-argument calls never reach a tool at all, and never print a `call`
line, because the decoder rejects them first. A tool body can rely on its
argument being a valid `CountArgs`: `args.level` is a real `Level`, and an
unknown key in the JSON can't have slipped through as a silent default.

`dispatch` lists the tools with plain `if` tests. As we saw, a decorator can't
add a function to a module-level registry, so the list of tools lives in one
function where you can read it. Adding a tool means writing the decorated
function, its two classes, and one more `if`. The reference agent in the
repository, under `examples/agents/tool_runner`, grows this same design into a
package with a separate tool layer and more tools.

## Summary

Functions in Aura are values with a full contract: parameter names, modes,
types, and result. Named functions and capture-free lambdas are thin values
you can store and pass anywhere. A lambda that captures becomes a closure,
and you choose how each capture is held: copied or moved in by default,
borrowed with `[name]` or `[mut name]`, or taken with `[own name]`. What the
body does with its captures makes it callable any number of times, once, or
only through a `mut` binding. To return or store a capturing closure, you pack
it into a `Callable[...]` type, explicitly.

Decorators are built from those pieces. A decorator is a function from a
`Callable` to a `Callable` of the same contract, applied once when the module
starts, and a generic decorator factory such as `traced[A, R](name)` serves
every function with one parameter named `args`. Paired with derived
`json.Codec` implementations, that gives a typed boundary for JSON: input is
decoded strictly into classes, with path-precise errors and a schema that
describes it, before any of your code runs. The [Manual](/manual/functions#decorators)
has the full decorator rules.

Several of the values in this chapter owned something that needed to be given
back at the end, a borrowed list or a captured counter. The next chapter looks
at resources that need cleanup at a known moment, such as files and network
connections, and at how `with` makes that moment explicit.
