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
`/=` remains true division. `//` can also use the `FloorDiv` operator trait
when no builtin numeric or Duration rule applies.

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
whole: float64 = 2
```

An integer literal adopts a `float32` or `float64` context only when its value is exactly representable in that type. This also makes mixed-literal arithmetic read naturally: `7.5 // 2` is floating floor division and `-7.5 % 2` is floating remainder. A bound integer variable is never widened this way. For an inexact value, use an explicit floating spelling when literal rounding is intentional, or call `.to_float()` when converting an integer value intentionally.

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

Printed `float32` and `float64` values use the shortest decimal spelling that round-trips to the same source type. Whole-number floats keep a trailing `.0`, signed zero stays `-0.0`, and large or tiny values use concise scientific notation. For example, `9007199254740992.0`, `1e300`, and `1e-300` print without being routed through lower `float32` precision.

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

Ordinary strings use matching single or double quotes. Both forms produce the
same `String`, support the same escapes, and concatenate with `+`:

```python
greeting = 'hello' + ", aurora"
apostrophe = 'Aurora\'s strings'
quotation = 'the compiler said "ready"'
```

The supported escapes are `\n`, `\t`, `\"`, `\'`, `\\`, `\0`, `\xHH`, and
`\u{H...}`. Triple-quoted, raw, and byte-string literals are not implemented,
and a one-character literal is still a `String` rather than a character type.

## F-Strings

Interpolated strings use the double-quoted `f"..."` form and produce an owned
`String`; `f'...'` is not supported:

```python
name: String = "Aurora"
answer: int32 = 42
print(f"Hello, {name} {answer}")
```

Interpolations accept any expression, including indexed lookups:

```python
print(f"value: {counts['key']}")
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

`len()` counts Unicode scalar values, while `byte_len()` reports the number of
bytes in the UTF-8 encoding. Both members return `int64`:

```python
text = 'A🎉'
print(text.len())       # 2; O(n)
print(text.byte_len())  # 5; O(1)
```

Aurora 0.1 does not support integer indexing or slicing on `String`.
Character iteration, `ord()`, and `chr()` are not implemented. Strict UTF-8
conversion is available now through `text.to_bytes()` and
`String.from_bytes(payload)`; hexadecimal, base64, typed conversion errors,
and SHA-256 are taught in [22-bytes.md](22-bytes.md). An explicit `encoding`
argument is reserved but not implemented, and slicing waits for Phase 7.

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

## Duration Values

Duration literals are used with the concurrency surface (see [13-concurrency.md](13-concurrency.md)):

```python
short_wait: Duration = 5ms
normal_wait: Duration = 1s
long_wait: Duration = 2m
```

The stored value is an exact signed i128 count of nanoseconds. Literals are
non-negative integral counts with `ms`, `s`, or `m`; there is no `ns` suffix,
fractional literal, or unary minus for Duration. Use the signed associated
constructors when the count is computed:

```python
attempt: int64 = 3
base = Duration.ms(125)
backoff = attempt * base
split = 1ms // attempt

print(backoff)                         # 375ms
print(split)                           # 0.333333ms
print(Duration.seconds(2) + 500ms)     # 2500ms
print(Duration.minutes(-1) < 0ms)      # true
print(Duration.ms(1500).to_seconds())  # 1.5
```

Duration supports checked `+` and `-` with another Duration, `* int64` in
either operand order, `// int64`, and all comparisons. `to_ms()` and
`to_seconds()` return the nearest representable IEEE-754 binary64 value using
ties-to-even and may round. Printing uses exact decimal milliseconds with at
most six fractional digits and trimmed zeros.
Negative values are useful in calculations but are rejected as sleeps,
timeouts, deadlines, and restart backoffs.

See [examples/concurrency/duration_arithmetic.au](../examples/concurrency/duration_arithmetic.au).
