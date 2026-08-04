# Converting Between Types

Aura never converts a number behind your back. An `int32` does not quietly
become an `int64`, and an integer does not drift into a float because it was
convenient. Every conversion is written down, and there are three ways to
write one.

## `as` For Numbers, Checked At Runtime

`expr as Type` converts between numeric types:

```aura
small: int32 = 7
wide = small as int64        # 7

big: int64 = 300
narrow = big as int32        # 300

exact = 3 as float64         # 3.0
truncated = 3.9 as int64     # 3, toward zero
```

`as` is exact or it fails. If the value does not fit the target, the program
stops with a diagnostic instead of wrapping around:

```aura
big: int64 = 5000000000
narrow = big as int32
```

```text
error[AU4002]: integer value `5000000000` does not fit in `int32`
```

The same rule applies to floats. An integer too large to be represented
precisely as a `float64` is a trap, not a silent rounding:

```aura
n: int64 = 9007199254740993
f = n as float64
```

```text
error[AU4002]: integer value `9007199254740993` cannot be represented exactly
as `float64`
```

That is the design: `as` means "this fits, and I am telling you it fits."

## `.to_float()` When Rounding Is The Point

Sometimes you *want* the nearest representable float — computing a ratio, say.
`.to_float()` rounds instead of trapping:

```aura
n: int64 = 9007199254740993
print(n.to_float() == 9007199254740992.0)   # true, rounded to nearest
```

This is also how you divide integers, since `/` on two integers is rejected:

```aura
ratio = 7.to_float() / 2.to_float()   # 3.5
```

Use `//` when you want the floor instead:

```aura
whole = 7 // 2        # 3
```

Pick by intent: `as float64` asserts exactness, `.to_float()` accepts rounding.

## Parsing And Rendering Text

Text is not a numeric type, so `as` does not apply:

```aura
s = "12"
n = s as int64
```

```text
error[AU2002]: casts are only supported between numeric types, found `str`
and `int64`
```

Text can always fail to parse, so parsing returns a `Result` you must handle:

```aura
match parse_int64("123"):
    case Result.Ok(value):
        print(value + 1)
    case Result.Err(message):
        print(message)
```

`parse_int32`, `parse_int64`, and the float parsers all follow this shape.

Going the other way never fails, so it needs no `Result` — use `str(value)` or
put the value straight into an f-string:

```aura
n: int64 = 42
print(str(n))
print(f"as text: {n}")
```

## Why No Implicit Conversion

The rule that catches Python developers is that passing an `int32` to a
function expecting `int64` is an error rather than a widening:

```aura
def f(x: int64) -> int64:
    return x

y: int32 = 5
print(f(y))       # error: expected `int64`, found `int32`
```

Write `f(y as int64)`. The reason is that implicit numeric conversion is where
overflow and precision bugs hide — a language that widens silently in one
direction eventually narrows silently in another. Aura's default integer type
is `int64` and its default float is `float64`, so most code never mixes widths
in the first place.

The one exception is deliberate and narrow: an index position accepts smaller
integer types, because widening an index can never lose information.

## Quick Reference

| Goal | Write |
| --- | --- |
| Widen or narrow a number, exactly | `value as int64` |
| Integer to float, rounding allowed | `value.to_float()` |
| Integer division | `a // b` |
| True division | `a.to_float() / b.to_float()` |
| Float to integer, toward zero | `value as int64` |
| Text to number | `parse_int64(text)`, handle the `Result` |
| Number to text | `str(value)` or `f"{value}"` |

The [Types](/manual/types) chapter gives the full conversion table and the
exact trap conditions.
