# Strings And Numbers

The bootstrap compiler already supports enough numeric and string behavior for useful examples.

## Literal Defaults

In the current compiler:

- integer literals default to `int32`
- floating-point literals default to `float64`

Floating-point literals can also adopt an expected `float32` type when the surrounding annotation or signature provides it:

```python
ratio: float32 = 3.25
```

Integer literals also support the full `uint128` range now, instead of stopping at the signed `i128` ceiling:

```python
value: uint128 = 340282366920938463463374607431768211455
```

## Arithmetic

```python
a: int32 = 6
b: int32 = 10
print(a + b)
```

The implemented arithmetic operators are:

- `+`
- `-`
- `*`
- `/`
- `%`

Unary minus is also supported for integer and floating-point expressions:

```python
a: int32 = -5
b: float64 = -3.5
```

See [examples/numbers/unary_minus.au](../examples/numbers/unary_minus.au).

## Floating-point Math

Aurora currently supports the builtin numeric helpers `abs(...)`, `min(...)`, `max(...)`, and `sqrt(...)`.

```python
print(abs(-7))
print(min(9, 2))
print(max(4, 12))
print(sqrt(81.0))
```

See [examples/numbers/numeric_builtins.au](../examples/numbers/numeric_builtins.au).

`float64` also supports the builtin `.sqrt()` method:

```python
def main():
    value: float64 = 81.0
    print(value.sqrt())
```

See [examples/numbers/float_sqrt.au](../examples/numbers/float_sqrt.au).

Primitive numeric and boolean values also support `.to_string()`:

```python
count: int32 = 42
ok: bool = true
print(count.to_string())
print(ok.to_string())
```

Whole-number floating-point values keep a trailing `.0` when printed, so `5.0` remains visually distinct from `5`.

The bootstrap compiler also supports ordinary `float32` values through typed contexts:

```python
class Measurement:
    value: float32

def double(x: float32) -> float32:
    return x + x
```

See [examples/numbers/float32_values.au](../examples/numbers/float32_values.au).

## Explicit Numeric Casts

Aurora now supports explicit numeric casts with `expr as Type`:

```python
whole = 7.9 as int32
narrowed = 1.25 as float32
widened = 3 as float64
```

The current bootstrap implementation supports casts between numeric primitive types only. Integer targets are range-checked at runtime, so a cast like `source as int8` fails cleanly if the value does not fit.

See [examples/numbers/numeric_casts.au](../examples/numbers/numeric_casts.au).

## String Concatenation

```python
greeting = "hello" + ", aurora"
```

Borrowed string parameters use `borrow str`:

```python
def greet(name: borrow str) -> String:
    return "Hello, " + name
```

See [examples/strings/borrow_str.au](../examples/strings/borrow_str.au).

Aurora also supports interpolated strings with `f"..."`. An f-string produces an owned `String`:

```python
name: String = "Aurora"
answer: int32 = 42
print(f"Hello, {name} {answer}")
```

Interpolations accept ordinary expressions, including indexed lookups such as `f"value: {counts["key"]}"`.

See [examples/strings/f_strings.au](../examples/strings/f_strings.au).

The current compiler supports:

- string literals with double quotes
- string concatenation with `+`
- borrowed `str` parameters
- interpolated `f"..."` strings
- equality and inequality comparisons
- `String.len()`
- `String.contains(...)`
- `String.starts_with(...)`
- `String.ends_with(...)`
- `String.split(...)`
- `String.replace(...)`
- `String.to_lower()`
- `String.to_upper()`
- `String.strip_prefix(...)`
- `String.strip_suffix(...)`
- `String.trim()`
- `String.join(...)`
- `String.clone()`

Example:

```python
def main() -> int32:
    text: String = "aurora"
    copy = text.clone()
    print(copy)
    return 0
```

See [examples/strings/string_clone.au](../examples/strings/string_clone.au).

The maintained string-method surface now looks like this:

```python
def print_string_option(value: Option[String]):
    match value:
        case Some(text):
            print(text)
        case None:
            print("none")

def main() -> int32:
    text = "  aurora repo  "
    print(text.len())
    print(text.contains("repo"))
    print(text.starts_with("  au"))
    print(text.ends_with("  "))
    trimmed = text.trim()
    print(trimmed)
    parts = trimmed.split(" ")
    print(parts.len())
    print(parts[0])
    print(parts[1])
    print(trimmed.replace("repo", "lang"))
    print(trimmed.to_lower())
    print(trimmed.to_upper())
    print_string_option(trimmed.strip_prefix("aurora "))
    print_string_option(trimmed.strip_prefix("repo"))
    print_string_option(trimmed.strip_suffix(" repo"))
    print_string_option(trimmed.strip_suffix("aurora"))
    print(trimmed.clone().len())
    return 0
```

See [examples/strings/string_methods.au](../examples/strings/string_methods.au).

`split(...)` returns `Vec[String]`. `strip_prefix(...)` and `strip_suffix(...)` return `Option[String]`, so they compose naturally with `match`.

`join(...)` uses the receiver as the separator:

```python
parts = ["aurora", "lang", "tests"]
print("-".join(parts))
```

## Parsing And Formatting

Aurora now includes parsing builtins for common scalar text conversion:

- `parse_int32(text: borrow str) -> Result[int32, String]`
- `parse_int64(text: borrow str) -> Result[int64, String]`
- `parse_float64(text: borrow str) -> Result[float64, String]`

Combined with `.to_string()` and `String.join(...)`, they cover the current maintained formatting surface:

```python
def main() -> int32:
    match parse_int32("42"):
        case Result.Ok(value):
            print(value.to_string())
        case Result.Err(message):
            print(message)

    parts = ["aurora", "lang", "tests"]
    print("-".join(parts))
    print(true.to_string())
    return 0
```

See [examples/strings/string_parsing_and_formatting.au](../examples/strings/string_parsing_and_formatting.au).

## String Equality

```python
if greeting == "hello, aurora":
    print(greeting)
```

See [examples/strings/greeting.au](../examples/strings/greeting.au).

## Booleans And Comparisons

The implemented subset supports:

- `==`
- `!=`
- `<`
- `<=`
- `>`
- `>=`
- `and`
- `or`
- `not`

Comparison expressions produce `bool`, which can be used in `if` and `while`.

Aurora does not currently do implicit numeric widening. Mixed expressions like `int32 + int64` still need matching types.
Use explicit numeric casts when you want a conversion instead.

## Duration Literals

Aurora also supports duration literals for the current concurrency surface:

```python
short_wait: Duration = 5ms
normal_wait: Duration = 1s
long_wait: Duration = 2m
```

These values are mainly used with `after(...)` inside `select`.

When printed directly, a `Duration` currently renders in milliseconds with an `ms` suffix.

## Numeric Type Names

The current type checker recognizes these numeric names:

- `int8`, `int16`, `int32`, `int64`, `int128`, `intsize`
- `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize`
- `float32`, `float64`

`float64` remains the default floating literal type. Use an explicit annotation or another expected type context when you want a literal to be treated as `float32`.

Annotated integer widths are enforced in both the checker and the runtime. If a value no longer fits its annotated integer type at runtime, Aurora reports an error instead of silently widening it.

Full-range `uint128` literals and arithmetic are part of the maintained bootstrap surface now:

```python
def main() -> int32:
    value: uint128 = 340282366920938463463374607431768211455
    almost: uint128 = 340282366920938463463374607431768211454
    print(value)
    print(almost + 1)
    return 0
```

See [examples/numbers/uint128_values.au](../examples/numbers/uint128_values.au).
