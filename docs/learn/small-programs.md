# The First Program

This chapter builds a small classifier. It takes a list of numbers, sorts each one into a category, counts how often each category appears, and prints a report.

On the way it covers bindings, functions, control flow, integer parsing, maps, and `match`. None of it is advanced, and all of it appears in real programs.

## Running A Script

An Aura file runs top to bottom. A script can mix prints, bindings, and computation:

```aura
print('aura')
print(40 + 2)
```

Ordinary strings may use matching single or double quotes. Both forms have the same escape rules. F-strings are always double-quoted, as `f"..."`.

Save the script as `greeting.au` and run `aura run greeting.au`.

Scripts suit quick tools and examples. When a program needs an explicit entry point, use `main`. This matters most for a native binary that should return an exit code:

```aura
def main() -> int32:
    print("ready")
    return 0
```

`main` takes no parameters and returns `int32` or `None`. A file may use either style, but not both at once.

## Bindings

Write `name = expression` when the type of the right-hand side is clear:

```aura
limit = 10
label = "jobs"
enabled = true
```

Bindings are immutable by default. Mark a binding `mut` when you will reassign it:

```aura
mut count = 0
count = count + 1
count += 1
```

Aura infers the type of most bindings from their initial value. Add an annotation when the compiler cannot infer it. Empty collection literals are the common case, because they have no elements to infer from:

```aura
values: list[int32] = []
counts: dict[str, int32] = {}
seen = set[str]()
```

Annotations also help at module boundaries and in function signatures, where the type is part of the program's public contract.

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

A function that returns no meaningful value may omit the return type:

```aura
def log_value(value: int32):
    print(value)
```

A parameter may have a default, so callers can omit it:

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

Named arguments are always available. Use them whenever a call would otherwise be hard to read.

## Small Callbacks With Lambdas

When a callback is one expression, write a lambda and give it a type from context:

```aura
offset: int32 = 40
add: def(int32) -> int32 = lambda value: value + offset

print(add(2))
```

The `def(int32) -> int32` annotation gives the parameter and result types. A lambda does not repeat those types inline.

- **Captures:** `offset` is a copy value, so the closure takes a snapshot of it when the lambda is created. A non-copy owned value moves into the closure instead. Clone it first when the outer scope also needs an owner.
- **Repeat calls:** a read-only closure may be called many times. A closure that consumes a non-copy capture is single-use.
- **Types:** a zero-parameter lambda can infer its result from the body. A lambda with parameters needs its parameter types from context.
- **Storage:** a capture-free lambda may be stored anywhere a function value can. A capturing closure is limited to immutable locals, direct calls, compiler-known callbacks, or one qualifying task start.

Use a named function when the callback needs more than one statement.

## Control Flow

`if`, `elif`, and `else` chain in the usual way:

```aura
if value < 0:
    print("negative")
elif value == 0:
    print("zero")
else:
    print("positive")
```

`for value in range(n)` counts from zero up to `n`, excluding `n`. `range(start, stop)` takes an explicit start:

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

`break` exits the nearest loop. `continue` skips to the next iteration:

```aura
for value in range(10):
    if value == 2:
        continue
    if value == 6:
        break
    print(value)
```

## `match`

`match` makes a decision based on the shape of a value. It works as a statement or as an expression that produces a value.

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

Integer and `str` matches need a `_` wildcard because their sets of values are open. A boolean match is exhaustive when it covers both `true` and `false`:

```aura
def enabled_name(enabled: bool) -> str:
    return match enabled:
        case true:
            "enabled"
        case false:
            "disabled"
```

On an enum, the compiler reports any missing variant. [Shaping Data](/learn/data-modeling) shows that form in detail.

## Turning Text Into Numbers

Parsing returns a `Result`, so bad input becomes explicit control flow:

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

`Result.Ok` carries the parsed value. `Result.Err` carries a message. The library usually returns this shape when an operation can fail in a way the caller should care about.

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
        case Lookup.Found(value):
            counts[key] = value + 1
        case Lookup.Missing:
            counts[key] = 1

values: list[int32] = [-3, 0, 1, 2, 10, 18, 21]
mut counts: dict[str, int32] = {}

for value in values:
    label = classify(value)
    bump(counts, label)

for key, value in counts.items():
    print(f"{key}: {value}")
```

Two details in `bump` deserve a closer look:

- **`counts: mut dict[str, int32]`** says the helper changes a dictionary its caller owns. The parameter declaration selects mutable access. The caller writes no prefix at the call site.
- **`key: own str`** says `bump` takes responsibility for storing the category string. `dict.get` only borrows its key and returns `Lookup[int32]`: `Lookup.Found(value)` when the category was seen before, and `Lookup.Missing` otherwise. The same owned `key` can then move into the `counts[key] = ...` assignment.

Run the program to see a tally for each category that appears in `values`.

## A Rule Of Thumb

Small Aura programs read well when type boundaries line up with data boundaries:

- parse input into typed values as early as possible
- use enums for states that have names
- use `T | None` when a value may be missing
- use `Result[T, E]` when an operation may fail
- borrow values for helpers that do not need ownership

Each rule still holds as the program grows. The next chapter applies them to richer data types.

Reference: [Statements](/manual/statements), [Functions](/manual/functions),
[Closures](/manual/closures), [Expressions](/manual/expressions).
