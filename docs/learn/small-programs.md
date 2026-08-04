# The First Program

The best way to meet a language is to write a program that actually reports something. In this chapter we will build a small classifier: it takes a list of numbers, sorts each one into a category, counts how often each category appears, and prints a report.

Along the way we will meet bindings, functions, control flow, integer parsing, maps, and `match`. Nothing here is advanced, but everything here shows up in real programs.

## Running A Script

Aura files run top to bottom. A script can mix prints, bindings, and computation:

```aura
print('aura')
print(40 + 2)
```

Ordinary strings may use matching single or double quotes, and both forms have
the same escape rules. F-strings remain double-quoted as `f"..."`.

Save that as `greeting.au` and run `aura run greeting.au`. Scripts are useful for quick tools and examples. When a program benefits from an explicit entry point — especially when it will be built as a native binary that should return an exit code — use `main`:

```aura
def main() -> int32:
    print("ready")
    return 0
```

`main` takes no parameters and returns `int32` or `None`. A file may use either style, but not both at once.

## Bindings

Use `name = expression` when the type of the right-hand side is clear:

```aura
limit = 10
label = "jobs"
enabled = true
```

Bindings are immutable by default. When a binding will be reassigned, mark it `mut`:

```aura
mut count = 0
count = count + 1
count += 1
```

Aura infers the type of most bindings from their initial value. Add an explicit annotation when the compiler cannot work it out on its own — especially for empty collection literals, which have no elements to guess from:

```aura
values: list[int32] = []
counts: dict[str, int32] = {}
seen = set[str]()
```

Annotations are also useful at module boundaries and in function signatures,
where the type forms part of the program's public contract.

## Functions

A function declares its parameters and its return type:

```aura
def classify(value: int32) -> str:
    if value < 0:
        return "negative"
    elif value == 0:
        return "zero"
    elif value < 10:
        return "small"
    else:
        return "large"
```

Functions that do not return a meaningful value may omit the return type:

```aura
def log_value(value: int32):
    print(value)
```

Parameters may have defaults, so callers can omit them:

```aura
def classify_with_limit(value: int32, limit: int32 = 10) -> str:
    if value < 0:
        return "negative"
    elif value < limit:
        return "small"
    else:
        return "large"

print(classify_with_limit(4))
print(classify_with_limit(40, limit=100))
```

Named arguments are always available and are worth reaching for whenever a call would otherwise be hard to read.

## Small Callbacks With Lambdas

When a callback is one expression, write a contextually typed lambda:

```aura
offset: int32 = 40
add: def(int32) -> int32 = lambda value: value + offset

print(add(2))
```

The `def(int32) -> int32` annotation tells the compiler the parameter and
result types. Lambdas do not repeat those types inline. The outer `offset` is a
Copy value, so the closure snapshots it when the lambda is created. A
non-Copy owned value instead moves into the closure; clone first when the outer
scope also needs an owner.

Read-only closures may be called repeatedly. A closure that consumes a
non-Copy capture is single-use. Use a named function when the callback needs
multiple statements.

A zero-parameter lambda can infer its result from the body. Lambdas with
parameters need their parameter types from context. Capture-free lambdas may
be stored anywhere a function value can; capturing closures stay in immutable
locals, direct calls, compiler-known callbacks, or one qualifying task start.

## Control Flow

`if`, `elif`, and `else` chain as you would expect:

```aura
if value < 0:
    print("negative")
elif value == 0:
    print("zero")
else:
    print("positive")
```

`for value in range(n)` counts from zero up to (but not including) `n`. With two arguments, `range(start, stop)` uses an explicit start:

```aura
mut total = 0
for value in range(5):
    total += value
print(total)

for value in range(-2, 3):
    print(value)
```

Use `while` when the stop condition is not a simple range:

```aura
mut current = 1
while current < 100:
    current = current * 2
print(current)
```

`break` exits the nearest loop; `continue` skips to the next iteration:

```aura
for value in range(10):
    if value == 2:
        continue
    if value == 6:
        break
    print(value)
```

## `match`

`match` is the tool for decisions with a shape. It can be used as a statement or as an expression that produces a value.

```aura
def status_name(code: int32) -> str:
    return match code:
        case 0:
            "ok"
        case 1:
            "retry"
        case 2:
            "degraded"
        case _:
            "failed"
```

Integer and `str` matches use `_` as a wildcard because their value spaces are open. Boolean matches are exhaustive when both `true` and `false` are covered:

```aura
def enabled_name(enabled: bool) -> str:
    return match enabled:
        case true:
            "enabled"
        case false:
            "disabled"
```

For enums, `match` becomes even more useful: the compiler will tell you when a variant is missing. [Shaping Data](/learn/data-modeling) shows that form in detail.

## Turning Text Into Numbers

Aura expresses parsing with `Result`, so a bad input becomes explicit control
flow:

```aura
def parse_count(text: str) -> int32:
    match parse_int32(text):
        case Result.Ok(value):
            return value
        case Result.Err(message):
            print(f"bad count: {message}")
            return 0

print(parse_count("42"))
print(parse_count("forty-two"))
```

`Result.Ok` carries the parsed value; `Result.Err` carries a message. When an operation can fail in a way the caller should care about, this is the shape the library will usually hand back.

## Putting It Together: A Classification Report

This program classifies a list of numbers, counts how often each category appears, and prints the totals.

```aura
def classify(value: int32) -> str:
    if value < 0:
        return "negative"
    elif value == 0:
        return "zero"
    elif value < 10:
        return "small"
    else:
        return "large"

def bump(counts: mut dict[str, int32], key: own str):
    match counts.get(key):
        case Some(value):
            counts[key] = value + 1
        case None:
            counts[key] = 1

values = [-3, 0, 1, 2, 10, 18, 21]
mut counts: dict[str, int32] = {}

for value in values:
    label = classify(value)
    bump(counts, label)

for key, value in counts.items():
    print(f"{key}: {value}")
```

There are two details in `bump` worth slowing down for.

`counts: mut dict[str, int32]` says the helper will mutate a dictionary owned by its
caller. The parameter declaration selects mutable access; the caller writes no
capability prefix at the call site.

`dict.get` borrows its key, so the same owned `key` can be moved into the later
`counts.set`. The `own` annotation says `bump` takes responsibility for storing
the category string.

Run the program and you should see a tally for each category that appeared in `values`.

## A Rule Of Thumb

Small Aura programs read well when type boundaries line up with data boundaries:

- parse input into typed values as early as possible
- use enums for states that have names
- use `Option[T]` when a value may be missing
- use `Result[T, E]` when an operation may fail
- borrow values for helpers that do not need ownership

Every one of those rules is still the right rule when the program grows. The next chapter puts them to work on richer data types.

Reference: [Statements](/manual/statements), [Functions](/manual/functions),
[Closures](/manual/closures), [Expressions](/manual/expressions).
