# Strings And Numbers

Aurora supports enough numeric and string behavior for real programs. This chapter covers arithmetic, string operations, parsing, formatting, and the numeric type system.

## Arithmetic

The standard arithmetic operators work on matching numeric types:

```python
a: int32 = 6
b: int32 = 10
print(a + b)    # 16
print(a - b)    # -4
print(a * b)    # 60
print(b // a)   # 1 (floor division)
print(b % a)    # 4
print(-b // a)  # -2
print(-b % a)   # 2
```

Integer `/` is intentionally rejected: it is too easy to misread as either
truncating integer division or floating true division. Use `//` for a floor
quotient. Both `//` and `%` follow the divisor-sign rule, including when either
operand is negative:

```python
print(7 // -3)  # -3
print(7 % -3)   # -2
print(-7 // 3)  # -3
print(-7 % 3)   # 2
```

The identity `a == (a // b) * b + (a % b)` holds for nonzero integer `b`.
Integer `//` and `%` by zero fail at runtime. Floating-point `/` remains true
division:

```python
print(7.0 / 2.0) # 3.5
```

When the inputs are integers and true division is intended, convert both with
`.to_float()`:

```python
numerator: int64 = 7
denominator: int64 = 2
print(numerator.to_float() / denominator.to_float()) # 3.5
```

Every integer type has `.to_float() -> float64`. It rounds to the nearest
representable IEEE-754 value using ties-to-even, so large integers may change:

```python
large: int64 = 9007199254740993
print(large.to_float()) # 9007199254740992.0
```

Floating values also support `//` and `%`. They use the CPython-compatible
floor/divmod correction, so the remainder follows the divisor's sign even
where a naive host remainder would not. Floating `/`, `//`, and `%` by zero
fail at runtime.

```python
print(-10.5 // 3.0) # -4.0
print(-10.5 % 3.0)  # 1.5
```

The matching compound assignments are `+=`, `-=`, `*=`, `/=`, `%=`, and
`//=`. Integer `/=` is rejected for the same reason as integer `/`; floating
`/=` remains true division. There is no `FloorDiv` operator trait for `//`.

Unary minus works on integers and floats:

```python
offset: int32 = -5
temperature: float64 = -3.5
```

See [examples/numbers/unary_minus.au](../examples/numbers/unary_minus.au).

Aurora does not do implicit numeric widening. Mixed expressions like `int32 + int64` are rejected -- use explicit casts instead (see below).

## Floating-Point Math

Integer literals default to `int64`, whose shorter alias is `int`. Floating-point literals default to `float64`. Both adopt a compatible expected numeric type when the surrounding context requires it:

```python
count: int32 = 12
ratio: float32 = 3.25
```

Aurora provides builtin numeric helpers:

```python
print(abs(-7))        # 7
print(min(9, 2))      # 2
print(max(4, 12))     # 12
print(sqrt(81.0))     # 9.0
```

`float64` also has a `.sqrt()` method:

```python
value: float64 = 81.0
print(value.sqrt())   # 9.0
```

Whole-number floats keep a trailing `.0` when printed, so `5.0` stays visually distinct from `5`.

See [examples/numbers/numeric_builtins.au](../examples/numbers/numeric_builtins.au) and [examples/numbers/float_sqrt.au](../examples/numbers/float_sqrt.au).

## `.to_string()`

Primitive numeric and boolean values support `.to_string()`:

```python
count: int32 = 42
ok: bool = true
print(count.to_string())   # "42"
print(ok.to_string())      # "true"
```

## Explicit Numeric Casts

Use `expr as Type` to cast between numeric types:

```python
whole = 7.9 as int32       # 7 (truncates toward zero)
narrowed = 1.25 as float32
widened = 3 as float64
```

Integer casts are range-checked at runtime -- `300 as int8` fails cleanly instead of silently wrapping.

Integer-to-float casts are also exactness-checked at runtime -- Aurora rejects casts that would silently lose integer precision instead of rounding them away.

That strict cast is intentionally different from `.to_float()`. For the
`9007199254740993` value above, `large.to_float()` returns the rounded
`9007199254740992.0`, while `large as float64` fails because the conversion is
not exact.

See [examples/numbers/numeric_casts.au](../examples/numbers/numeric_casts.au).
The combined arithmetic example is
[examples/basics/numbers.au](../examples/basics/numbers.au).

## The Full Numeric Type System

| Signed | Unsigned | Float |
|--------|----------|-------|
| `int8` | `uint8` | `float32` |
| `int16` | `uint16` | `float64` |
| `int32` | `uint32` | |
| `int64` | `uint64` | |
| `int128` | `uint128` | |
| `intsize` | `uintsize` | |

Use `int` (the `int64` alias) and `float64` by default. Other explicit widths are useful when you need control over memory layout, value ranges, or a fixed API contract. APIs declared with `int32` remain `int32`; the new literal default does not widen them. Full-range `uint128` arithmetic is supported:

```python
value: uint128 = 340282366920938463463374607431768211455
print(value)
```

See [examples/numbers/uint128_values.au](../examples/numbers/uint128_values.au).

Annotated integer widths are enforced at runtime. If a value exceeds its annotated type's range, Aurora reports an error instead of silently widening.

The bootstrap compiler also supports `float32` in typed contexts like class fields and function parameters:

```python
class Measurement:
    value: float32

def double(x: float32) -> float32:
    return x + x
```

See [examples/numbers/float32_values.au](../examples/numbers/float32_values.au).

## String Basics

Strings use double quotes. Concatenation uses `+`:

```python
greeting = "hello" + ", aurora"
```

## F-Strings

Interpolated strings use the `f"..."` prefix and produce an owned `String`:

```python
name: String = "Aurora"
answer: int32 = 42
print(f"Hello, {name} {answer}")
```

Interpolations accept any expression, including indexed lookups:

```python
print(f"value: {counts["key"]}")
```

See [examples/strings/f_strings.au](../examples/strings/f_strings.au).

## Borrowed String Parameters

When a function takes a string it only reads, use `borrow str`:

```python
def greet(name: borrow str) -> String:
    return "Hello, " + name
```

See [examples/strings/borrow_str.au](../examples/strings/borrow_str.au).

## String Methods

Aurora provides a rich set of string methods:

```python
text = "  aurora repo  "
print(text.len())                    # 15
print(text.contains("repo"))         # true
print(text.starts_with("  au"))      # true
print(text.ends_with("  "))          # true
trimmed = text.trim()                # "aurora repo"
parts = trimmed.split(" ")           # ["aurora", "repo"]
print(trimmed.replace("repo", "lang"))  # "aurora lang"
print(trimmed.to_lower())           # "aurora repo"
print(trimmed.to_upper())           # "AURORA REPO"
```

`strip_prefix(...)` and `strip_suffix(...)` return `Option[String]`, so they compose with `match`:

```python
match trimmed.strip_prefix("aurora "):
    case Some(rest):
        print(rest)     # "repo"
    case None:
        print("no match")
```

`join(...)` uses the receiver as the separator:

```python
parts = ["aurora", "lang", "tests"]
print("-".join(parts))    # "aurora-lang-tests"
```

`clone()` creates an independent copy of a string (see [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) for why this matters):

```python
text: String = "aurora"
copy = text.clone()
print(text)    # still valid
print(copy)
```

See [examples/strings/string_methods.au](../examples/strings/string_methods.au) and [examples/strings/string_clone.au](../examples/strings/string_clone.au).

## Parsing And Formatting

Aurora provides parsing builtins that return `Result`:

- `parse_int32(text: borrow str) -> Result[int32, String]`
- `parse_int64(text: borrow str) -> Result[int64, String]`
- `parse_float64(text: borrow str) -> Result[float64, String]`

Use `match` to handle success and failure:

```python
match parse_int32("42"):
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

Combined with `.to_string()` and `String.join(...)`, these cover the maintained formatting surface.

See [examples/strings/string_parsing_and_formatting.au](../examples/strings/string_parsing_and_formatting.au).

## String Equality

Strings support `==` and `!=`:

```python
if greeting == "hello, aurora":
    print(greeting)
```

See [examples/strings/greeting.au](../examples/strings/greeting.au).

## Booleans And Comparisons

The comparison operators produce `bool`:

- `==`, `!=`, `<`, `<=`, `>`, `>=`
- `and`, `or`, `not`

```python
if score >= 90 and not failed:
    print("passed")
```

## Duration Literals

Duration literals are used with the concurrency surface (see [13-concurrency.md](13-concurrency.md)):

```python
short_wait: Duration = 5ms
normal_wait: Duration = 1s
long_wait: Duration = 2m
```

When printed, a `Duration` renders in milliseconds with an `ms` suffix.
