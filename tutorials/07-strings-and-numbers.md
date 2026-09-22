# Strings And Numbers

This chapter covers arithmetic, the numeric types and conversions between
them, numeric arrays, strings, parsing, formatting, booleans, and `Duration`
values.

## Arithmetic

The arithmetic operators work on two values of the same numeric type:

```aura check-pass
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

Aura does not widen numbers implicitly. It rejects a mixed expression such as
`int32 + int64`. Use an explicit cast instead, as shown in
[Explicit Numeric Casts](#explicit-numeric-casts).

Integer `/` is rejected on purpose, because it is easy to misread as either
truncating integer division or floating true division. Use `//` for a floor
quotient. Both `//` and `%` follow the divisor-sign rule, including when
either operand is negative:

```aura check-pass
print(7 // -3)  # -3
print(7 % -3)   # -2
print(-7 // 3)  # -3
print(-7 % 3)   # 2
```

For nonzero integer `b`, the identity `a == (a // b) * b + (a % b)` holds.
Integer `//` and `%` by zero fail at runtime.

Floating-point `/` is true division:

```aura check-pass
print(7.0 / 2.0) # 3.5
```

For true division of two integers, convert both with `.to_float()`:

```aura check-pass
numerator: int64 = 7
denominator: int64 = 2
print(numerator.to_float() / denominator.to_float()) # 3.5
```

Every integer type has `.to_float() -> float64`. It rounds to the nearest
representable IEEE-754 value using ties-to-even, so a large integer may
change:

```aura check-pass
large: int64 = 9007199254740993
print(large.to_float()) # 9007199254740992.0
```

Floating values also support `//` and `%`. They use the CPython-compatible
floor and divmod correction, so the remainder follows the divisor's sign even
where a naive host remainder would not. Floating `/`, `//`, and `%` by zero
fail at runtime.

```aura check-pass
print(-10.5 // 3.0) # -4.0
print(-10.5 % 3.0)  # 1.5
```

The compound assignments are `+=`, `-=`, `*=`, `**=`, `/=`, `%=`, and `//=`.
Integer `/=` is rejected for the same reason as integer `/`. Floating `/=` is
true division. `//` can also use the `FloorDiv` operator trait when no builtin
numeric or Duration rule applies.

Unary minus works on integers and floats:

```aura check-pass
offset: int32 = -5
temperature: float64 = -3.5
```

See [examples/numbers/unary_minus.au](../examples/numbers/unary_minus.au).

## Integer Literal Bases

Integer literals can be decimal, hexadecimal, binary, or octal. Underscores
group digits without changing the value:

```aura check-pass
requests = 1_000_000
red: uint32 = 0xFF
permissions: uint16 = 0o755
flags: uint8 = 0b1010_0110
```

- The prefixes are case-insensitive.
- An underscore must sit between two digits that are valid in the selected
  base.
- The sign is a unary operator, so `-0x7F` means unary minus applied to
  `0x7F`.
- Contextual typing and integer range checks are the same for every literal
  spelling.

## Bitwise Operators And Shifts

Every integer type supports `&`, `|`, `^`, `~`, `<<`, and `>>`. Both operands
of a binary operation have the same exact integer type, and that includes a
shift count:

```aura check-pass
value: uint32 = 0b1010_0000
mask: uint32 = 0b1111_0000
four: uint32 = 4

print(value & mask)       # 160
print(value | 0b0000_1111) # 175
print(value ^ mask)       # 80
print(~value)             # 4294967135
print(value >> four)      # 10
print(value << four)      # 2560
```

- A shift count must be in `0..width`.
- Signed right shift extends the sign bit. Unsigned right shift fills with
  zero.
- Ordinary left shift is checked. It reports `AU4002` when the mathematical
  result does not fit.
- The compound forms are `&=`, `|=`, `^=`, `<<=`, and `>>=`.

## Power, Rounding, And Divmod

`**` is right-associative. It binds more tightly than a unary minus on its
left:

```aura check-pass
print(2 ** 3 ** 2) # 512
print(-2 ** 2)     # -4
print((-2) ** 2)   # 4
print(2.0 ** -2.0) # 0.25
```

Integer power keeps the exact integer type, rejects negative exponents, and
checks overflow. Floating power keeps the exact floating type and reports
defined domain and overflow failures.

`round` rounds a floating input to the nearest integer, with ties to even, and
returns `int64`. It returns an integer input unchanged, with its exact type:

```aura check-pass
print(round(2.5)) # 2
print(round(3.5)) # 4
```

`divmod(left, right)` evaluates both values once. It returns the floor
quotient and the divisor-signed remainder together:

```aura check-pass
quotient, remainder = divmod(-17, 5)
print(quotient)  # -4
print(remainder) # 3
```

Both arguments must have one exact integer or floating type. A zero divisor
reports `AU4004`.

## Explicit Integer Arithmetic Modes

Ordinary integer `+`, `-`, and `*` are checked. They report `AU4002` if the
mathematical result does not fit the integer type. Every integer type also
provides explicit wrapping and saturating methods:

```aura check-pass
top: int32 = 2147483647

print(top.wrapping_add(1))    # -2147483648
print(top.saturating_add(1))  # 2147483647
print(top.wrapping_sub(-1))   # -2147483648
print(top.saturating_mul(2))  # 2147483647
```

Shifts have the same explicit modes. The count has the receiver's exact type
and must stay below its bit width:

```aura check-pass
high: uint8 = 0b1000_0000
one: uint8 = 1

print(high.wrapping_shl(one))   # 0
print(high.saturating_shl(one)) # 255
print(high.wrapping_shr(one))   # 64
print(high.saturating_shr(one)) # 64
```

The two right-shift methods match ordinary `>>` once the count is valid.

The same six method names work on integer `Array[T]`. The right operand is
either another `Array[T]` of the same shape or one scalar of exactly `T`.

## Numeric Arrays

`Array[T]` is fixed-shape, contiguous, row-major numeric storage. `T` must be
exactly `int32`, `int64`, `float32`, or `float64`:

```aura check-pass
def square(value: float64) -> float64:
    return value * value

matrix = Array[float64].from_list([1.0, 2.0, 3.0, 4.0], [2, 2])
squares = matrix.map[float64](square)
first_row = squares[0:1]

print(squares[1, 0])  # 9.0
print(first_row.sum()) # 5.0
print(squares.mean())  # 7.5
```

The element type is called the dtype. Arithmetic follows these rules:

- Array-with-Array arithmetic requires the same dtype and the exact same
  shape.
- Scalar arithmetic requires a scalar of exactly `T`. The scalar can be on
  either side of `+`, `-`, and `*`.
- `/` is available only for floating arrays.
- There is no implicit dtype promotion and no array-shape broadcasting.

`sum`, `min`, and `max` return `T`. `mean` always returns `float64`. For
floating arrays:

- `sum`, `min`, and `max` proceed left to right in row-major order, with dtype
  rounding.
- `mean` accumulates as `float64`.
- Reductions propagate NaN.

See [examples/numbers/numeric_arrays.au](../examples/numbers/numeric_arrays.au)
and [Numeric Arrays](../docs/manual/numeric-arrays.md).

## Floating-Point Math

Integer literals default to `int64`, whose shorter alias is `int`.
Floating-point literals default to `float64`. Both adopt a compatible expected
numeric type when the surrounding context requires one:

```aura check-pass
count: int32 = 12
ratio: float32 = 3.25
whole: float64 = 2
```

An integer literal adopts a `float32` or `float64` context only when its value
is exactly representable in that type. So mixed-literal arithmetic reads
naturally: `7.5 // 2` is floating floor division, and `-7.5 % 2` is floating
remainder. A bound integer variable is never widened this way. For an inexact
value, write a floating literal when the rounding is intended. To convert an
integer value on purpose, call `.to_float()`.

Aura provides builtin numeric helpers:

```aura check-pass
print(abs(-7))        # 7
print(min(9, 2))      # 2
print(max(4, 12))     # 12
print(sqrt(81.0))     # 9.0
```

`float64` also has a `.sqrt()` method:

```aura check-pass
value: float64 = 81.0
print(value.sqrt())   # 9.0
```

A printed `float32` or `float64` uses the shortest decimal spelling that
round-trips to the same source type:

- A whole-number float keeps a trailing `.0`.
- Signed zero stays `-0.0`.
- Large or tiny values use concise scientific notation.

For example, `9007199254740992.0`, `1e300`, and `1e-300` print without being
routed through lower `float32` precision.

See [examples/numbers/numeric_builtins.au](../examples/numbers/numeric_builtins.au),
[examples/numbers/float_sqrt.au](../examples/numbers/float_sqrt.au), and
[examples/numbers/bit_packing.au](../examples/numbers/bit_packing.au).

## `.to_string()`

Primitive numeric and boolean values support `.to_string()`:

```aura check-pass
count: int32 = 42
ok: bool = true
print(count.to_string())   # "42"
print(ok.to_string())      # "true"
```

## Explicit Numeric Casts

Use `expr as Type` to cast between numeric types:

```aura check-pass
whole = 7.9 as int32       # 7 (truncates toward zero)
narrowed = 1.25 as float32
widened = 3 as float64
```

Integer casts are range-checked and never wrap:

- The checker rejects a provably invalid literal, such as `300 as int8`.
- A cast of a value computed at runtime is checked when it executes. It traps
  cleanly if the value does not fit.

Integer-to-float casts follow the same split. The checker can reject a
literal, and a runtime value is checked for exactness when the cast runs. Aura
rejects a cast that would lose integer precision.

This strict cast differs from `.to_float()` on purpose. Take the
`9007199254740993` value from [Arithmetic](#arithmetic).
`large.to_float()` returns the rounded `9007199254740992.0`, but
`large as float64` fails because the conversion is not exact.

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

Use `int`, the `int64` alias, and `float64` by default. Choose another width
when you need control over memory layout, value ranges, or a fixed API
contract. An API declared with `int32` stays `int32`, because literal
defaulting does not widen it.

`uint128` supports full-range arithmetic:

```aura check-pass
value: uint128 = 340282366920938463463374607431768211455
print(value)
```

See [examples/numbers/uint128_values.au](../examples/numbers/uint128_values.au).

Annotated integer widths are enforced at runtime. If a value exceeds its
annotated type's range, Aura reports an error and keeps the declared type.

`float32` also works in typed contexts such as class fields and function
parameters:

```aura check-pass
class Measurement:
    value: float32

def double(x: float32) -> float32:
    return x + x
```

See [examples/numbers/float32_values.au](../examples/numbers/float32_values.au).

## str Basics

An ordinary string uses matching single or double quotes. Both forms produce
the same `str` and support the same escapes. Join strings with `+`:

```aura check-pass
greeting = 'hello' + ", aura"
apostrophe = 'Aura\'s strings'
quotation = 'the compiler said "ready"'
```

The supported escapes are `\n`, `\t`, `\"`, `\'`, `\\`, `\0`, `\xHH`, and
`\u{H...}`. A one-character literal is still a `str`, because Aura has no
character type.

Use three matching quotes for exact multiline text. The compiler keeps the
first newline, the last newline, indentation, spaces, and physical tabs:

```aura check-pass
prompt = """Summarize the request.
Return JSON with a label and reason.
"""
```

Use a lowercase `r` prefix for a single-line value where backslashes are
data:

```aura check-pass
model_dir = r"C:\models\agent"
number_pattern = r'\d+\.\d+'
```

A raw string cannot end in an odd run of backslashes. Raw triple strings and
byte strings are not available.

## F-Strings

An f-string uses the double-quoted `f"..."` form and produces an owned `str`.
The `f'...'` form is not supported:

```aura check-pass
name: str = "Aura"
answer: int32 = 42
print(f"Hello, {name} {answer}")
print(f"{name:·^16.8s} {answer:>8,d}")
print(f"success rate: {0.875:+.1%}")
print(f"delta: {-1.25:09.3f}")
```

An interpolation accepts any expression, including an indexed lookup:

```aura fragment
print(f"value: {counts['key']}")
```

A static format specification follows a top-level colon. It supports:

- a fill of one scalar
- `<`, `^`, and `>` alignment
- numeric signs
- a minimum width
- comma grouping
- precision
- the type codes `d`, `f`, `e`, `x`, `X`, `b`, `o`, `%`, and `s`

The specification follows these rules:

- Width counts Unicode scalars.
- String precision truncates by Unicode scalar count.
- Numeric precision rounds ties to even.
- A numeric width that begins with `0` pads after the sign.
- Decimal grouping always uses an explicit `d`, `f`, or `%` code.
- The checker validates each specification against the interpolation's
  static type before the program runs.

See [examples/strings/f_strings.au](../examples/strings/f_strings.au).

## Borrowed str Parameters

When a function only reads a string, declare the parameter as `str`:

```aura check-pass
def greet(name: str) -> str:
    return "Hello, " + name
```

See [examples/strings/borrow_str.au](../examples/strings/borrow_str.au).

## str Methods

These are the common string methods:

```aura check-pass
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

`len()` counts Unicode scalar values. `byte_len()` reports the number of bytes
in the UTF-8 encoding. Both return `int64`:

```aura check-pass
text = 'A🎉'
print(text.len())       # 2; O(n)
print(text.byte_len())  # 5; O(1)
```

A `str` has no integer indexing. A one-colon slice returns a fresh owned
`str`:

```aura check-pass
text = "A🎉Z"
print(text[1:2])   # 🎉
print(text[:2])    # A🎉
print(text[-2:])   # 🎉Z
print(text[:])     # A🎉Z
```

- Endpoints count Unicode scalar values, matching `len()`. They do not count
  UTF-8 bytes or grapheme clusters.
- Slicing is O(n), because locating scalar boundaries scans the text.
- Written endpoints use `int64`. A negative endpoint is normalized once.
- Both effective endpoints must lie in `0..=len`, and the start must not
  exceed the end.
- Aura does not clamp invalid bounds as Python does. An invalid or reversed
  range traps with `AU4003`.
- The result is an owned copy, not a view.

Slice steps and slice assignment are not available. Character iteration,
`ord()`, and `chr()` are not implemented.

`text.to_bytes()` and `str.from_bytes(payload)` convert with strict UTF-8.
[22-bytes.md](22-bytes.md) covers hexadecimal, base64, typed conversion
errors, and SHA-256. An explicit `encoding` argument is reserved but not
implemented.

`strip_prefix(...)` and `strip_suffix(...)` return `str | None`, so they work
with `is not None` or a `match` type pattern:

```aura fragment
match trimmed.strip_prefix("aura "):
    case str as rest:
        print(rest)     # "repo"
    case None:
        print("no match")
```

`join(...)` uses the receiver as the separator:

```aura check-pass
parts = ["aura", "lang", "tests"]
print("-".join(parts))    # "aura-lang-tests"
```

`clone()` creates an independent copy of a string.
[06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) explains why
this matters.

```aura check-pass
text: str = "aura"
copy = text.clone()
print(text)    # still valid
print(copy)
```

See [examples/strings/string_methods.au](../examples/strings/string_methods.au) and [examples/strings/string_clone.au](../examples/strings/string_clone.au).

## Parsing And Formatting

These parsing builtins return a `Result`:

- `parse_int32(text: str) -> Result[int32, str]`
- `parse_int64(text: str) -> Result[int64, str]`
- `parse_float64(text: str) -> Result[float64, str]`

Use `match` to handle success and failure:

```aura check-pass
match parse_int32("42"):
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

Together with `.to_string()` and `str.join(...)`, these builtins cover the
supported conversions between text and numbers.

See [examples/strings/string_parsing_and_formatting.au](../examples/strings/string_parsing_and_formatting.au).

## str Equality

Strings support `==` and `!=`:

```aura fragment
if greeting == "hello, aura":
    print(greeting)
```

See [examples/strings/greeting.au](../examples/strings/greeting.au).

## Booleans And Comparisons

These operators produce `bool`:

- `==`, `!=`, `<`, `<=`, `>`, `>=`
- `and`, `or`, `not`

```aura fragment
if score >= 90 and not failed:
    print("passed")
```

## Duration Values

Duration literals work with the concurrency features in
[13-concurrency.md](13-concurrency.md):

```aura check-pass
short_wait: Duration = 5ms
normal_wait: Duration = 1s
long_wait: Duration = 2m
```

A `Duration` stores an exact signed i128 count of nanoseconds. A literal is a
non-negative whole count with `ms`, `s`, or `m`. There is no `ns` suffix, no
fractional literal, and no unary minus for Duration. Use the signed
associated constructors when the count is computed:

```aura check-pass
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

Duration supports:

- checked `+` and `-` with another Duration
- `* int64` in either operand order
- `// int64`
- all comparisons

`to_ms()` and `to_seconds()` return the nearest representable IEEE-754
binary64 value using ties-to-even, so they may round. Printing uses exact
decimal milliseconds, with at most six fractional digits and trailing zeros
trimmed.

Negative values are useful in calculations. They are rejected as sleeps,
timeouts, deadlines, and restart backoffs.

See [examples/concurrency/duration_arithmetic.au](../examples/concurrency/duration_arithmetic.au).
