# Strings And Numbers

Aura supports enough numeric and string behavior for real programs. This chapter covers arithmetic, string operations, parsing, formatting, and the numeric type system.

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

Aura does not do implicit numeric widening. Mixed expressions like `int32 + int64` are rejected -- use explicit casts instead (see below).

## Explicit Integer Arithmetic Modes

Ordinary integer `+`, `-`, and `*` are checked and report `AU4002` if the
mathematical result does not fit the integer type. Every integer type also
provides explicit wrapping and saturating alternatives:

```python
top: int32 = 2147483647

print(top.wrapping_add(1))    # -2147483648
print(top.saturating_add(1))  # 2147483647
print(top.wrapping_sub(-1))   # -2147483648
print(top.saturating_mul(2))  # 2147483647
```

The same six method names are available on integer `Array[T]`. Their right
operand is either another same-shape `Array[T]` or one scalar of exactly `T`.

## Numeric Arrays

`Array[T]` provides fixed-shape, contiguous, row-major numeric storage for
exactly `int32`, `int64`, `float32`, and `float64`:

```python
def square(value: float64) -> float64:
    return value * value

matrix = Array[float64].from_vec([1.0, 2.0, 3.0, 4.0], [2, 2])
squares = matrix.map[float64](square)
first_row = squares[0:1]

print(squares[1, 0])  # 9.0
print(first_row.sum()) # 5.0
print(squares.mean())  # 7.5
```

Array/Array arithmetic requires the same dtype and exact shape. Scalar
arithmetic requires exactly `T`; scalar operands work on either side of
`+`, `-`, and `*`. `/` is available only for floating Arrays. There is no
implicit dtype promotion or array-shape broadcasting.

`sum`, `min`, and `max` return `T`; `mean` always returns `float64`.
Floating `sum`, `min`, and `max` proceed left-to-right in row-major order
with dtype rounding, floating `mean` accumulates as `float64`, and floating
reductions propagate NaN. See
[examples/numbers/numeric_arrays.au](../examples/numbers/numeric_arrays.au)
and [Numeric Arrays](../docs/manual/numeric-arrays.md).

## Floating-Point Math

Integer literals default to `int64`, whose shorter alias is `int`. Floating-point literals default to `float64`. Both adopt a compatible expected numeric type when the surrounding context requires it:

```python
count: int32 = 12
ratio: float32 = 3.25
whole: float64 = 2
```

An integer literal adopts a `float32` or `float64` context only when its value is exactly representable in that type. This also makes mixed-literal arithmetic read naturally: `7.5 // 2` is floating floor division and `-7.5 % 2` is floating remainder. A bound integer variable is never widened this way. For an inexact value, use an explicit floating spelling when literal rounding is intentional, or call `.to_float()` when converting an integer value intentionally.

Aura provides builtin numeric helpers:

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

Integer casts are range-checked at runtime. `300 as int8` fails cleanly and
never wraps.

Integer-to-float casts are also exactness-checked at runtime. Aura rejects a
cast that would lose integer precision.

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

Annotated integer widths are enforced at runtime. If a value exceeds its
annotated type's range, Aura reports an error and preserves the declared type.

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
greeting = 'hello' + ", aura"
apostrophe = 'Aura\'s strings'
quotation = 'the compiler said "ready"'
```

The supported escapes are `\n`, `\t`, `\"`, `\'`, `\\`, `\0`, `\xHH`, and
`\u{H...}`. Triple-quoted, raw, and byte-string literals are not implemented,
and a one-character literal remains a `String`. Aura has no character type.

## F-Strings

Interpolated strings use the double-quoted `f"..."` form and produce an owned
`String`; `f'...'` is not supported:

```python
name: String = "Aura"
answer: int32 = 42
print(f"Hello, {name} {answer}")
```

Interpolations accept any expression, including indexed lookups:

```python
print(f"value: {counts['key']}")
```

See [examples/strings/f_strings.au](../examples/strings/f_strings.au).

## Borrowed String Parameters

When a function takes a string it only reads, use `str`:

```python
def greet(name: str) -> String:
    return "Hello, " + name
```

See [examples/strings/borrow_str.au](../examples/strings/borrow_str.au).

## String Methods

Aura provides a rich set of string methods:

```python
text = "  aura repo  "
print(text.len())                    # 15
print(text.contains("repo"))         # true
print(text.starts_with("  au"))      # true
print(text.ends_with("  "))          # true
trimmed = text.trim()                # "aura repo"
parts = trimmed.split(" ")           # ["aura", "repo"]
print(trimmed.replace("repo", "lang"))  # "aura lang"
print(trimmed.to_lower())           # "aura repo"
print(trimmed.to_upper())           # "AURA REPO"
```

`len()` counts Unicode scalar values, while `byte_len()` reports the number of
bytes in the UTF-8 encoding. Both members return `int64`:

```python
text = 'A🎉'
print(text.len())       # 2; O(n)
print(text.byte_len())  # 5; O(1)
```

Integer indexing on `String` remains unavailable, but one-colon slicing
returns a fresh owned String:

```python
text = "A🎉Z"
print(text[1:2])   # 🎉
print(text[:2])    # A🎉
print(text[-2:])   # 🎉Z
print(text[:])     # A🎉Z
```

Endpoints count Unicode scalar values, matching `len()`. They do not count
UTF-8 bytes or grapheme clusters. Locating scalar boundaries scans the text, so
String slicing is O(n). Written endpoints are exactly `int32`; negatives
normalize once. Both effective endpoints must lie in `0..=len`, and start must
not exceed end. Aura does not clamp invalid bounds like Python: invalid or
reversed ranges trap with `AU4003`.

The result is an owned copy, not a view. Slice steps and slice assignment are
unavailable. Character iteration, `ord()`, and `chr()` are also not
implemented. Strict UTF-8 conversion is available through `text.to_bytes()`
and `String.from_bytes(payload)`; hexadecimal, base64, typed conversion errors,
and SHA-256 are taught in [22-bytes.md](22-bytes.md). An explicit `encoding`
argument remains reserved but unimplemented.

`strip_prefix(...)` and `strip_suffix(...)` return `Option[String]`, so they compose with `match`:

```python
match trimmed.strip_prefix("aura "):
    case Some(rest):
        print(rest)     # "repo"
    case None:
        print("no match")
```

`join(...)` uses the receiver as the separator:

```python
parts = ["aura", "lang", "tests"]
print("-".join(parts))    # "aura-lang-tests"
```

`clone()` creates an independent copy of a string (see [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) for why this matters):

```python
text: String = "aura"
copy = text.clone()
print(text)    # still valid
print(copy)
```

See [examples/strings/string_methods.au](../examples/strings/string_methods.au) and [examples/strings/string_clone.au](../examples/strings/string_clone.au).

## Parsing And Formatting

Aura provides parsing builtins that return `Result`:

- `parse_int32(text: str) -> Result[int32, String]`
- `parse_int64(text: str) -> Result[int64, String]`
- `parse_float64(text: str) -> Result[float64, String]`

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
if greeting == "hello, aura":
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
