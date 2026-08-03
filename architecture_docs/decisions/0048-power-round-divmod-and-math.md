# ADR-0048: Power, rounding, divmod, and the math module

- Status: Accepted
- Date: 2026-08-02
- Roadmap decision: Batch S1, S4.3
- Builds on: ADR-0002, ADR-0016, ADR-0019, and ADR-0047

## Context

Aura's numeric core needs exponentiation, explicit rounding, quotient/remainder
pairs, and standard transcendental functions. The surface must preserve exact
integer types, checked integer arithmetic, deterministic evaluation order, and
one documented policy for IEEE-754 exceptional values across both backends.

## Decision

### Power syntax and precedence

`**` is the power operator. It associates right:

```aura
2 ** 3 ** 2       # 2 ** (3 ** 2)
```

Power binds more tightly than a unary operator on its left, while the right
operand may begin with a unary operator. Consequently:

```aura
-2 ** 2           # -(2 ** 2)
(-2) ** 2         # 4
2.0 ** -2.0       # 0.25
```

The base evaluates exactly once before the exponent. A compound `**=` uses the
ordinary single-selected-place compound-assignment contract and stores only
after successful exponentiation.

### Integer power

Two operands of the same exact integer type produce that integer type. The
exponent must be non-negative. A negative exponent that is established by the
source expression is rejected with `AU2003`; a negative value discovered at
runtime traps with `AU4001`. Both diagnostics explain that fractional power
requires explicit floating operands, such as `base.to_float() **
exponent.to_float()`.

Integer power is checked. It computes the exact mathematical result and traps
with `AU4002` when the result is outside the operand type. The implementation
may use exponentiation by squaring but may not expose a different intermediate
overflow result. For every integer type, `x ** 0` is `1`, including `0 ** 0`.
Zero raised to a positive exponent is zero.

### Floating power

Two operands of the same exact floating type produce that type. `float32`
power is evaluated as the correctly rounded result required for its binary32
destination; `float64` power uses the maintained libm `pow` contract.

The floating operator follows the `math.pow` exceptional-value policy below.
A finite result that overflows the destination traps with `AU4002`. A domain
error traps with `AU4001`. Specified NaN and infinity results are values, not
diagnostics.

### `round`

The builtin overloads are:

```aura
round(value: T) -> T          # every integer type T
round(value: float32) -> int64
round(value: float64) -> int64
```

An integer is returned unchanged with its exact type. Floating values round to
the nearest mathematical integer, with an exact halfway case choosing the even
integer. Positive and negative zero both produce integer zero. NaN, either
infinity, and a finite rounded result outside `int64` trap with `AU4002`.
There is no digit-count parameter.

### `divmod`

The builtin overloads are:

```aura
divmod(left: T, right: T) -> (T, T)   # every integer type T
divmod(left: F, right: F) -> (F, F)   # float32 or float64 F
```

Operands have one exact type. The result is `(quotient, remainder)` and is
identical to evaluating Aura's `left // right` and `left % right` contract
from the same captured operands. Integer quotient rounds toward negative
infinity. A nonzero remainder has the divisor's sign and satisfies
`left == quotient * right + remainder` mathematically. Floating results use
the maintained corrected floor-divmod algorithm, including signed-zero rules.
The divisor is evaluated once; zero reports `AU4004`.

### The `math` module

The module exports these functions:

```aura
math.floor(value: float64) -> int64
math.ceil(value: float64) -> int64
math.trunc(value: float64) -> int64
math.pow(base: float64, exponent: float64) -> float64
math.exp(value: float64) -> float64
math.log(value: float64) -> float64
math.log2(value: float64) -> float64
math.log10(value: float64) -> float64
math.sin(value: float64) -> float64
math.cos(value: float64) -> float64
math.tan(value: float64) -> float64
```

It also exports immutable `float64` constants `math.pi`, `math.e`,
`math.inf`, and `math.nan`. `pi` and `e` are the nearest binary64 values to the
mathematical constants. `inf` is positive infinity. `nan` is a quiet NaN;
programs must not depend on a NaN payload or sign.

No implicit numeric conversion is introduced. A caller with a different
numeric type uses an explicit conversion before calling `math`.

### IEEE, domain, and overflow policy

The following table is normative:

| Operation/input | Result |
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
| finite `pow` inputs with an infinite-magnitude mathematical result | `AU4002`. |
| other libm results, including documented infinities from infinite inputs | The corresponding IEEE-754 binary64 value. |

An exponent is integral for the negative-base rule when its binary64 value is
finite and exactly equal to its truncation. Signed zero follows IEEE sign
rules. Subnormal inputs and results are preserved; Aura does not enable
flush-to-zero as a language behavior.

Every function evaluates arguments left to right and exactly once. These
functions are deterministic for one maintained target/libm pair; the test
suite pins portable identities and agreed error classification, and uses a
documented tolerance for transcendental finite approximations.

## Diagnostics

- `AU2002` reports unequal or incompatible operand/argument types.
- `AU2003` reports unsupported numeric domains and a statically established
  negative integer exponent, with explicit floating guidance.
- `AU4001` reports a runtime negative integer exponent or documented floating
  domain error and names the operation and operands.
- `AU4002` reports checked integer power overflow, floating finite overflow,
  and invalid/out-of-range float-to-`int64` rounding.
- `AU4004` reports a zero `divmod` divisor.

Constant folding preserves these exact categories and source locations.

## Backend requirements

MIR and direct execution call shared semantic helpers for integer power,
ties-to-even conversion, divmod correction, exceptional-value classification,
and libm result validation. Both backends must agree byte-for-byte on integer
results and diagnostics, and bit-for-bit on specified floating constants,
NaN/infinity classification, signed zero, and results supplied by the common
libm implementation.

## Limits

The `math` module is `float64` only. This decision adds no complex numbers,
decimal arithmetic, arbitrary-precision integers, vectorized math, random
functions, factorial/combinatorics, `round(value, digits)`, alternate-base
logarithm argument, or user-overloadable power protocol.

## Consequences

Aura gains the numeric operations expected in data preparation, evaluation,
backoff calculations, and scientific control code while keeping type changes
explicit. Domain and overflow behavior is a language rule, not a host-libm
accident.

## Completion test matrix

- parser tests for `**`, `**=`, right associativity, unary precedence on both
  sides, parentheses, and interaction with multiplicative/additive/bitwise
  operators
- static tests for every integer and floating width, exact-type requirements,
  result types, negative literal exponents, `round` overloads, `divmod`
  overloads, and the exact `math` signatures/constants
- integer runtime tests for exponents zero/one, `0 ** 0`, negative bases,
  every type boundary, checked overflow, runtime negative exponent, and
  unchanged `**=` targets on failure
- `round` tests for positive/negative halfway cases, even selection, signed
  zero, integer identity/type preservation, int64 boundaries, NaN, and
  infinities
- integer and floating `divmod` tests for every sign combination, invariant,
  signed zero, NaN/infinity behavior inherited from the operators, one-time
  evaluation, and zero divisors
- table-driven `math` tests covering every row of the exceptional-value table,
  constants by bits, subnormals, representative finite accuracy, and
  left-to-right argument effects
- constant-folding, MIR/direct diagnostic and output parity, compiler
  analysis, completion, hover, go-to-definition for `math`, language-server,
  bundled-editor, maintained-example, and executable Manual coverage

## Ratification

Batch S1 accepts this as Aura 0.3's power, rounding, divmod, and scalar math
contract. Frontend, shared helpers, both backends, module metadata,
diagnostics, reference, examples, and tooling land together.
