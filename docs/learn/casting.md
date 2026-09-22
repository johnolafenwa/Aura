# Converting Between Types

Aura never converts a number for you. An `int32` does not become an `int64`
on its own, and an integer does not turn into a float. You write every
conversion, and there are three ways to write one.

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

`as` is exact or it fails. If the value does not fit the target type, the
program stops with a diagnostic. It never wraps around:

```aura
big: int64 = 5000000000
narrow = big as int32
```

```text
error[AU4002]: integer value `5000000000` does not fit in `int32`
```

Floats follow the same rule. If a `float64` cannot hold an integer exactly,
the conversion traps. It does not round:

```aura
n: int64 = 9007199254740993
f = n as float64
```

```text
error[AU4002]: integer value `9007199254740993` cannot be represented exactly
as `float64`
```

Read `as` as a promise: "this value fits the target."

## `.to_float()` When Rounding Is The Point

Sometimes you want the nearest float, for example when you compute a ratio.
`.to_float()` rounds instead of trapping:

```aura
n: int64 = 9007199254740993
print(n.to_float() == 9007199254740992.0)   # true, rounded to nearest
```

`.to_float()` is also how you divide integers, because the checker rejects
`/` on two integers:

```aura
ratio = 7.to_float() / 2.to_float()   # 3.5
```

Use `//` for floor division:

```aura
whole = 7 // 2        # 3
```

Choose by intent. `as float64` asserts that the value is exact.
`.to_float()` accepts rounding.

## Parsing And Rendering Text

`as` does not apply to text, because `str` is not a numeric type:

```aura
s = "12"
n = s as int64
```

```text
error[AU2002]: casts are only supported between numeric types, found `str`
and `int64`
```

Parsing text can always fail, so a parser returns a `Result` that you must
handle:

```aura
match parse_int64("123"):
    case Result.Ok(value):
        print(value + 1)
    case Result.Err(message):
        print(message)
```

`parse_int32`, `parse_int64`, and the float parsers all work this way.

Turning a number into text never fails, so it needs no `Result`. Use
`str(value)`, or put the value in an f-string:

```aura
n: int64 = 42
print(str(n))
print(f"as text: {n}")
```

## Why No Implicit Conversion

Python developers most often trip on this rule: passing an `int32` to a
function that expects `int64` is an error. Aura does not widen the value:

```aura
def f(x: int64) -> int64:
    return x

y: int32 = 5
print(f(y))       # error: expected `int64`, found `int32`
```

Write `f(y as int64)` instead.

Implicit numeric conversion is where overflow and precision bugs hide. A
language that widens silently in one direction ends up narrowing silently in
another. Aura's default integer type is `int64` and its default float type is
`float64`, so most code never mixes widths.

There is one deliberate exception. An index position accepts smaller integer
types, because widening an index never loses information.

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
exact conditions that trap.
