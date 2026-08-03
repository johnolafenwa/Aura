# Math Module

The `math` module provides exact binary64 constants plus scalar `float64`
rounding, exponentiation, exponential, logarithmic, and trigonometric
functions. Every function input is explicitly `float64`; the module performs
no implicit numeric conversion.

## Public API

| API | Signature | Contract |
| --- | --- | --- |
| `math.pi` | `float64` constant | Nearest binary64 value to pi, bits `0x400921fb54442d18`. |
| `math.e` | `float64` constant | Nearest binary64 value to Euler's number, bits `0x4005bf0a8b145769`. |
| `math.inf` | `float64` constant | Positive infinity, bits `0x7ff0000000000000`. |
| `math.nan` | `float64` constant | Canonical quiet NaN, bits `0x7ff8000000000000`. |
| `math.floor` | `floor(value: float64) -> int64` | Greatest integer less than or equal to `value`. |
| `math.ceil` | `ceil(value: float64) -> int64` | Least integer greater than or equal to `value`. |
| `math.trunc` | `trunc(value: float64) -> int64` | Integer obtained by discarding the fractional part toward zero. |
| `math.pow` | `pow(base: float64, exponent: float64) -> float64` | Binary64 exponentiation under the exceptional-value policy below. |
| `math.exp` | `exp(value: float64) -> float64` | Binary64 base-e exponential. |
| `math.log` | `log(value: float64) -> float64` | Binary64 natural logarithm. |
| `math.log2` | `log2(value: float64) -> float64` | Binary64 base-2 logarithm. |
| `math.log10` | `log10(value: float64) -> float64` | Binary64 base-10 logarithm. |
| `math.sin` | `sin(value: float64) -> float64` | Binary64 sine with the input measured in radians. |
| `math.cos` | `cos(value: float64) -> float64` | Binary64 cosine with the input measured in radians. |
| `math.tan` | `tan(value: float64) -> float64` | Binary64 tangent with the input measured in radians. |

## IEEE-754, Domain, And Overflow Policy

This table is normative for every maintained backend.

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
| Other libm results, including documented infinities from infinite inputs | The corresponding IEEE-754 binary64 value. |

An exponent is integral for the negative-base rule when its binary64 value is
finite and exactly equal to its truncation. Signed zero follows IEEE-754 sign
rules. Subnormal inputs and results are preserved. Aura does not enable
flush-to-zero as a language behavior.

Finite transcendental results use the maintained target's binary64 math
implementation. Portable programs may depend on the classifications and
identities in the table. Last-bit finite approximation can vary between
maintained target and libm pairs.

## Example

```aura
import math

def main() -> int32:
    print(math.pi)
    print(math.e)
    print(math.inf)
    print(math.nan)
    print(math.floor(-1.25))
    print(math.ceil(-1.25))
    print(math.trunc(-1.75))
    print(math.pow(2.0, -3.0))
    print(math.exp(0.0))
    print(math.log(1.0))
    print(math.log2(8.0))
    print(math.log10(1000.0))
    print(math.sin(0.0))
    print(math.cos(0.0))
    print(math.tan(0.0))
    return 0
```

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

The maintained program is `examples/numbers/scalar_math.au`.

## Grammar

The module adds no source-language grammar. `import math`, qualified member
access, calls, named arguments, and negative numeric expressions use the
ordinary forms defined by this Manual.

## Typing Rules

The four constants have exact type `float64` and the bit patterns shown in the
Public API table. They support qualified reads and direct imports with ordinary
import aliases. Every function parameter has type `float64`. `floor`, `ceil`,
and `trunc` return `int64`; every other function returns `float64`. A value of
any other numeric type requires an explicit conversion before the call. Normal
argument-count, argument-name, and exact-type checks apply.

The module namespace contains every constant and function in the Public API
table.

## Runtime Semantics

Each function applies the IEEE-754, domain, and overflow policy above.
`floor`, `ceil`, and `trunc` first compute the specified mathematical integer
and then require it to fit `int64`. `pow` classifies its identities, NaN,
domain, and finite-overflow cases before returning the maintained binary64
result. The exponential, logarithmic, and trigonometric functions preserve
the table's NaN, infinity, signed-zero, and subnormal outcomes.

Each constant has one immutable module storage location initialized once before
application execution. Every read uses that shared location. Copy-scalar use
preserves the stored binary64 bits, including the canonical NaN payload.

For one maintained target and math implementation, repeated calls with the
same binary64 inputs produce the same binary64 result. The functions perform
no I/O and observe no process-global mutable state.

## Ownership And Evaluation Order

Constant reads are shared and cannot be assigned or used through mutable
access. Call arguments evaluate left to right and exactly once before the
function executes. `math.pow` evaluates `base` before `exponent`. Every
parameter and result is a Copy scalar, so calls do not move or mutate caller
bindings. A failed call leaves all already completed argument effects
observable and produces no result value.

## Diagnostics

- `AU2001` reports an unknown module member.
- `AU2002` reports an argument whose type is not exactly `float64`.
- `AU2004` reports invalid argument binding, including a wrong argument count
  or name.
- `AU4001` reports the domain errors named in the normative table.
- `AU4002` reports a finite overflow or a rounding result that cannot be
  represented as `int64`.

## Backend Support

All listed functions are supported by the MIR runtime and direct native
backend. Both backends use shared exceptional-value classification and must
agree on result classification, signed zero, and diagnostic code. They use
the same maintained host math implementation for finite results on one target.

## Limits And Implementation-Defined Behavior

The module is scalar and `float64` only. It provides no complex, decimal,
arbitrary-precision, vectorized, combinatorial, or random operations. The
logarithm functions accept one value and do not accept an alternate base.

The final bits of finite transcendental approximations can vary across target
and libm pairs. The exact host diagnostic rendering around an `AU4001` or
`AU4002` failure follows the general runtime diagnostic contract.

## Status

The constants, functions, exact bits and signatures, exceptional-value
classifications, initialization and evaluation order, diagnostics, and
MIR/direct backend behavior on this page are implemented and maintained in
Aura 0.3.
