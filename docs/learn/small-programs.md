# The First Program

The best way to meet a language is to write a program that actually reports something. In this chapter we will build a small classifier: it takes a list of numbers, sorts each one into a category, counts how often each category appears, and prints a report.

Along the way we will meet bindings, functions, control flow, integer parsing, maps, and `match`. Nothing here is advanced, but everything here shows up in real programs.

## Running A Script

Aurora files run top to bottom. A script can mix prints, bindings, and computation:

```python
print('aurora')
print(40 + 2)
```

Ordinary strings may use matching single or double quotes, and both forms have
the same escape rules. F-strings remain double-quoted as `f"..."`.

Save that as `greeting.au` and run `aura run greeting.au`. Scripts are useful for quick tools and examples. When a program benefits from an explicit entry point — especially when it will be built as a native binary that should return an exit code — use `main`:

```python
def main() -> int32:
    print("ready")
    return 0
```

`main` takes no parameters and returns `int32` or `None`. A file may use either style, but not both at once.

## Bindings

Use `name = expression` when the type of the right-hand side is clear:

```python
limit = 10
label = "jobs"
enabled = true
```

Bindings are immutable by default. When a binding will be reassigned, mark it `mut`:

```python
mut count = 0
count = count + 1
count += 1
```

Aurora infers the type of most bindings from their initial value. Add an explicit annotation when the compiler cannot work it out on its own — especially for empty collection literals, which have no elements to guess from:

```python
values: Vec[int32] = []
counts: Map[String, int32] = {}
seen: Set[String] = {}
```

Annotations are also a useful discipline at module boundaries and in function signatures, where the type is part of the program's contract rather than a local detail.

## Functions

A function declares its parameters and its return type:

```python
def classify(value: int32) -> String:
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

```python
def log_value(value: int32):
    print(value)
```

Parameters may have defaults, so callers can omit them:

```python
def classify_with_limit(value: int32, limit: int32 = 10) -> String:
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

## Control Flow

`if`, `elif`, and `else` chain as you would expect:

```python
if value < 0:
    print("negative")
elif value == 0:
    print("zero")
else:
    print("positive")
```

`for value in range(n)` counts from zero up to (but not including) `n`. With two arguments, `range(start, stop)` uses an explicit start:

```python
mut total = 0
for value in range(5):
    total += value
print(total)

for value in range(-2, 3):
    print(value)
```

Use `while` when the stop condition is not a simple range:

```python
mut current = 1
while current < 100:
    current = current * 2
print(current)
```

`break` exits the nearest loop; `continue` skips to the next iteration:

```python
for value in range(10):
    if value == 2:
        continue
    if value == 6:
        break
    print(value)
```

## `match`

`match` is the tool for decisions with a shape. It can be used as a statement or as an expression that produces a value.

```python
def status_name(code: int32) -> String:
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

Integer and `String` matches use `_` as a wildcard because their value spaces are open. Boolean matches are exhaustive when both `true` and `false` are covered:

```python
def enabled_name(enabled: bool) -> String:
    return match enabled:
        case true:
            "enabled"
        case false:
            "disabled"
```

For enums, `match` becomes even more useful: the compiler will tell you when a variant is missing. [Shaping Data](/learn/data-modeling) shows that form in detail.

## Turning Text Into Numbers

Aurora expresses parsing with `Result`, so a bad input is ordinary control flow rather than an exception:

```python
def parse_count(text: String) -> int32:
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

```python
def classify(value: int32) -> String:
    if value < 0:
        return "negative"
    elif value == 0:
        return "zero"
    elif value < 10:
        return "small"
    else:
        return "large"

def bump(counts: borrow mut Map[String, int32], key: String):
    match counts.get(key.clone()):
        case Some(value):
            counts.set(key, value + 1)
        case None:
            counts.set(key, 1)

values = [-3, 0, 1, 2, 10, 18, 21]
mut counts: Map[String, int32] = {}

for value in values:
    label = classify(value)
    bump(counts, label)

for entry in counts.items():
    print(f"{entry.key}: {entry.value}")
```

There are two details in `bump` worth slowing down for.

`counts: borrow mut Map[String, int32]` says the helper will mutate a map owned by its caller. The parameter type is where the borrow is declared; the caller does not write `borrow` at the call site. Aurora supplies it from the signature.

`key.clone()` appears because `Map.get` takes its key by value. The function still needs `key` afterwards for `counts.set`, so it clones before the first call and moves the original into the second. Clones in Aurora are explicit, which is a feature: you see where the program deliberately keeps two copies.

Run the program and you should see a tally for each category that appeared in `values`.

## A Rule Of Thumb

Small Aurora programs read well when type boundaries line up with data boundaries:

- parse input into typed values as early as possible
- use enums for states that have names
- use `Option[T]` when a value may be missing
- use `Result[T, E]` when an operation may fail
- borrow values for helpers that do not need ownership

Every one of those rules is still the right rule when the program grows. The next chapter puts them to work on richer data types.

Reference: [Statements](/manual/statements), [Functions](/manual/functions), [Expressions](/manual/expressions).
