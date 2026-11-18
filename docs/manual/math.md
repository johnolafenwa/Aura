# Math Module

The `math` module provides exact binary64 constants, scalar `float64`
functions for rounding, powers, roots, exponentials, logarithms, and
trigonometry with its inverse and hyperbolic forms, and exact `int64`
functions such as `gcd` and `factorial`. A function takes `float64`
arguments unless its signature says `int64`. The module performs no implicit
numeric conversion.

## Public API

| API | Signature | Contract |
| --- | --- | --- |
| `math.pi` | `float64` constant | Nearest binary64 value to pi, bits `0x400921fb54442d18`. |
| `math.e` | `float64` constant | Nearest binary64 value to Euler's number, bits `0x4005bf0a8b145769`. |
| `math.inf` | `float64` constant | Positive infinity, bits `0x7ff0000000000000`. |
| `math.nan` | `float64` constant | Canonical quiet NaN, bits `0x7ff8000000000000`. |
| `math.tau` | `float64` constant | Nearest binary64 value to 2 pi, bits `0x401921fb54442d18`. |
| `math.floor` | `floor(value: float64) -> int64` | Greatest integer less than or equal to `value`. |
| `math.ceil` | `ceil(value: float64) -> int64` | Least integer greater than or equal to `value`. |
| `math.trunc` | `trunc(value: float64) -> int64` | The integer left after dropping the fractional part, rounding toward zero. |
| `math.pow` | `pow(base: float64, exponent: float64) -> float64` | Binary64 exponentiation, following the exceptional-value policy below. |
| `math.exp` | `exp(value: float64) -> float64` | Binary64 base-e exponential. |
| `math.log` | `log(value: float64) -> float64` | Binary64 natural logarithm. |
| `math.log2` | `log2(value: float64) -> float64` | Binary64 base-2 logarithm. |
| `math.log10` | `log10(value: float64) -> float64` | Binary64 base-10 logarithm. |
| `math.sin` | `sin(value: float64) -> float64` | Binary64 sine of an angle in radians. |
| `math.cos` | `cos(value: float64) -> float64` | Binary64 cosine of an angle in radians. |
| `math.tan` | `tan(value: float64) -> float64` | Binary64 tangent of an angle in radians. |
| `math.asin` | `asin(value: float64) -> float64` | The angle in `[-pi/2, pi/2]` whose sine is `value`. |
| `math.acos` | `acos(value: float64) -> float64` | The angle in `[0, pi]` whose cosine is `value`. |
| `math.atan` | `atan(value: float64) -> float64` | The angle in `[-pi/2, pi/2]` whose tangent is `value`. |
| `math.atan2` | `atan2(y: float64, x: float64) -> float64` | The angle in `[-pi, pi]` of the point `(x, y)`, with the quadrant taken from both signs. |
| `math.sinh` | `sinh(value: float64) -> float64` | Hyperbolic sine. |
| `math.cosh` | `cosh(value: float64) -> float64` | Hyperbolic cosine. |
| `math.tanh` | `tanh(value: float64) -> float64` | Hyperbolic tangent. |
| `math.asinh` | `asinh(value: float64) -> float64` | Inverse hyperbolic sine. |
| `math.acosh` | `acosh(value: float64) -> float64` | Inverse hyperbolic cosine, for `value >= 1`. |
| `math.atanh` | `atanh(value: float64) -> float64` | Inverse hyperbolic tangent, for `-1 < value < 1`. |
| `math.sqrt` | `sqrt(value: float64) -> float64` | Correctly rounded square root. |
| `math.cbrt` | `cbrt(value: float64) -> float64` | Cube root. Defined for negative values. |
| `math.hypot` | `hypot(x: float64, y: float64) -> float64` | `sqrt(x*x + y*y)` without overflow in the squares. |
| `math.expm1` | `expm1(value: float64) -> float64` | `exp(value) - 1`, accurate for small `value`. |
| `math.log1p` | `log1p(value: float64) -> float64` | `log(1 + value)`, accurate for small `value`. |
| `math.copysign` | `copysign(magnitude: float64, sign: float64) -> float64` | `magnitude` with the sign bit of `sign`. |
| `math.degrees` | `degrees(value: float64) -> float64` | Radians to degrees. |
| `math.radians` | `radians(value: float64) -> float64` | Degrees to radians. |
| `math.is_nan` | `is_nan(value: float64) -> bool` | `true` for NaN. |
| `math.is_inf` | `is_inf(value: float64) -> bool` | `true` for either infinity. |
| `math.is_finite` | `is_finite(value: float64) -> bool` | `true` for a value that is neither NaN nor infinite. |
| `math.gcd` | `gcd(left: int64, right: int64) -> int64` | Greatest common divisor, never negative. `gcd(0, 0)` is `0`. |
| `math.lcm` | `lcm(left: int64, right: int64) -> int64` | Least common multiple, never negative. Zero when either argument is zero. |
| `math.isqrt` | `isqrt(value: int64) -> int64` | The greatest integer whose square is at most `value`. |
| `math.factorial` | `factorial(value: int64) -> int64` | `value!`. Fits `int64` up to `factorial(20)`. |
| `math.comb` | `comb(count: int64, chosen: int64) -> int64` | The number of ways to choose `chosen` of `count` items without order. Zero when `chosen > count`. |
| `math.perm` | `perm(count: int64, chosen: int64) -> int64` | The number of ordered arrangements of `chosen` of `count` items. Zero when `chosen > count`. |

## Example

<<< ../../examples/numbers/scalar_math.au{aura}

This program prints:

```text
3.141592653589793
2.718281828459045
inf
NaN
-2
-1
-1
0.125
1.0
0.0
3.0
3.0
0.0
1.0
0.0
```

The same program is maintained as `examples/numbers/scalar_math.au`.

The inverse functions and the integer functions:

<<< ../../examples/numbers/angles_and_integers.au{aura}

This program prints:

```text
5.0
53
30
true
true
12
12
31622
3628800
2598960
20
```

## IEEE-754, Domain, And Overflow Policy

This table is normative for every maintained backend. `log*` means `log`,
`log2`, and `log10`.

| Operation or input | Result |
| --- | --- |
| `floor`, `ceil`, or `trunc` of finite in-range `x` | Corresponding mathematical integer as `int64`. |
| `floor`, `ceil`, or `trunc` of NaN, infinity, or an out-of-range finite value | `AU4002`. |
| `exp(nan)` | NaN. |
| `exp(+inf)` / `exp(-inf)` | `+inf` / `+0.0`. |
| `exp` of a finite input with finite representable result | Nearest binary64 result. |
| `exp` of a finite input whose result overflows | `AU4002`. Underflow produces the correctly signed zero or subnormal value. |
| `log* (nan)` | NaN. |
| `log* (+inf)` | `+inf`. |
| `log* (x)` for finite `x <= 0.0`, including either zero | `AU4001` domain error. |
| `sin`, `cos`, or `tan` of NaN | NaN. |
| `sin`, `cos`, or `tan` of either infinity | `AU4001` domain error. |
| `pow(x, 0.0)` for any `x`, including NaN | `1.0`. |
| `pow(1.0, y)` for any `y`, including NaN | `1.0`. |
| `pow(nan, y)` or `pow(x, nan)` outside the two identities above | NaN. |
| `pow(0.0, y)` for finite `y < 0.0` | `AU4001` domain error. |
| `pow(x, y)` for finite `x < 0.0` and finite non-integral `y` | `AU4001` domain error. |
| Finite `pow` inputs with an infinite-magnitude mathematical result | `AU4002`. |
| Any `float64` function below with a NaN argument | NaN. |
| `sqrt(x)` for `x < 0.0`, including `-inf`. `sqrt(-0.0)` is `-0.0` | `AU4001` domain error. |
| `asin(x)` or `acos(x)` for `x` outside `[-1, 1]` | `AU4001` domain error. |
| `acosh(x)` for `x < 1.0` | `AU4001` domain error. |
| `atanh(x)` for `x <= -1.0` or `x >= 1.0` | `AU4001` domain error. |
| `log1p(x)` for `x <= -1.0` | `AU4001` domain error. |
| `sinh`, `cosh`, `expm1`, `hypot`, or `degrees` of finite input whose result overflows | `AU4002`. |
| `atan`, `atan2`, `tanh`, `asinh`, `cbrt`, `copysign`, `radians`, and the three predicates | Defined for every input. |
| `isqrt`, `factorial`, `comb`, or `perm` with a negative argument | `AU4001` domain error. |
| `gcd`, `lcm`, `factorial`, `comb`, or `perm` whose exact result does not fit `int64` | `AU4002`. `gcd` overflows only when the answer is 2^63. |
| Other libm results, including documented infinities from infinite inputs | The corresponding IEEE-754 binary64 value. |

For the negative-base `pow` rule, an exponent is integral when its binary64
value is finite and exactly equal to its truncation.

Signed zero follows the IEEE-754 sign rules. Subnormal inputs and results are
kept. Aura does not enable flush-to-zero.

Finite transcendental results come from the target's binary64 math library.
Portable programs can rely on the classifications and identities in the table.
The last bit of a finite approximation can differ between target and libm
pairs.

## Grammar

The module adds no grammar. `import math`, qualified member access, calls,
named arguments, and negative numeric expressions use the ordinary forms
defined in this Manual.

## Typing Rules

The five constants have type `float64` and the exact bit patterns in
[Public API](#public-api). You can read them qualified, or import them directly
with ordinary import aliases.

`gcd`, `lcm`, `isqrt`, `factorial`, `comb`, and `perm` take and return
`int64`. Every other function parameter is `float64`. `floor`, `ceil`, and
`trunc` return `int64`, the three `is_` predicates return `bool`, and every
other function returns `float64`. A value of any other numeric type needs an
explicit conversion before the call. The normal checks on
argument count, argument names, and exact types apply.

The module namespace contains every constant and function in the Public API
table.

## Runtime Semantics

Each function follows the IEEE-754, domain, and overflow policy above.

- `floor`, `ceil`, and `trunc` compute the mathematical integer first, then
  require it to fit in `int64`.
- `pow` handles its identities, NaN, domain errors, and finite overflow before
  it returns the binary64 result.
- The exponential, logarithmic, and trigonometric functions keep the NaN,
  infinity, signed-zero, and subnormal outcomes from the table.

Each constant has one immutable module storage location, initialized once
before the program starts. Every read uses that location. Because the
constants are Copy scalars, each use keeps the stored binary64 bits, including
the canonical NaN payload.

On one target with one math library, repeated calls with the same binary64
inputs give the same binary64 result. The functions do no I/O and read no
global mutable state.

## Ownership And Evaluation Order

Constant reads are shared. You cannot assign to a constant or use it through
mutable access.

Call arguments evaluate from left to right, exactly once, before the function
runs. `math.pow` evaluates `base` before `exponent`. Every parameter and
result is a Copy scalar, so a call never moves or changes a caller's binding.
When a call fails, the effects of arguments already evaluated remain, and the
call produces no value.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU2001` | Unknown module member. |
| `AU2002` | An argument whose type is not exactly the parameter's type, `float64` or `int64`. |
| `AU2004` | Invalid argument binding, such as a wrong argument count or name. |
| `AU4001` | A domain error listed in the policy table. |
| `AU4002` | Finite overflow, or a rounding result that does not fit in `int64`. |

## Backend Support

The MIR runtime and the direct native backend support every listed function.
Both use the same exceptional-value classification. They must agree on the
result classification, signed zero, and diagnostic code. On one target, both
use the same host math library for finite results.

## Limits And Implementation-Defined Behavior

The module covers scalar `float64` and `int64` only. It has no `float32`
forms and no complex, decimal, arbitrary-precision, vectorized, or random
operations. The integer functions do not widen: a result outside `int64` is
`AU4002`. The logarithm functions take one value and have no alternate-base
form.

The final bits of finite transcendental results can differ across target and
libm pairs. The host's rendering around an `AU4001` or `AU4002` failure
follows the general runtime diagnostic rules.

## Status

`tau` and the functions from `asin` to `perm` in the Public API table are new
after Aura 0.16. Everything on this page is implemented on both the MIR and direct
backends: the constants and functions, their exact bits and signatures, the
exceptional-value classifications, initialization and evaluation order, and
diagnostics.
