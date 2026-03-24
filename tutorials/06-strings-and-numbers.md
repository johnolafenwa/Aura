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

`float64` currently supports the builtin `.sqrt()` method:

```python
def main():
    value: float64 = 81.0
    print(value.sqrt())
```

See [examples/numbers/float_sqrt.au](../examples/numbers/float_sqrt.au).

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

The current compiler supports:

- string literals with double quotes
- string concatenation with `+`
- equality and inequality comparisons
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
