# ADR-0110: `math` gains inverse and hyperbolic functions, and exact integer functions

- Status: Accepted
- Date: 2026-11-16

## Context

`math` had eleven functions: rounding, `pow`, `exp`, three logarithms,
and `sin`, `cos`, `tan`. A program could not turn a ratio back into an
angle, and it had no square root in the module, no `hypot`, no way to
ask whether a value is NaN, and no greatest common divisor. Each is a
few lines of user code that is easy to get subtly wrong, most of all
near the edges of the domain.

## Decision

`math` adds, all on `float64`:

- `sqrt`, `cbrt`, `asin`, `acos`, `atan`, `atan2(y, x)`, `sinh`, `cosh`,
  `tanh`, `asinh`, `acosh`, `atanh`, `expm1`, `log1p`, `hypot(x, y)`,
  `copysign(magnitude, sign)`, `degrees`, `radians`;
- the predicates `is_nan`, `is_inf`, `is_finite`, which return `bool`;
- the constant `tau`.

It adds, on `int64` with an `int64` result: `gcd`, `lcm`, `isqrt`,
`factorial`, `comb(count, chosen)`, `perm(count, chosen)`.

The exceptional-value policy is the one the module already had:

- A NaN argument gives NaN. No function traps on NaN.
- An argument outside the function's domain traps with `AU4001`:
  `sqrt` and `log1p` below their domain, `asin` and `acos` outside
  `[-1, 1]`, `acosh` below 1, `atanh` at or outside `±1`, and a
  negative argument of `isqrt`, `factorial`, `comb`, or `perm`.
- A finite argument whose mathematical result is too large traps with
  `AU4002`: `sinh`, `cosh`, `expm1`, `hypot`, `degrees`, and every
  integer function whose result does not fit `int64`.
- `atan2`, `copysign`, and the predicates accept every value.

The integer functions are exact. `comb` computes through partial
products that are themselves binomial coefficients, so it traps only
when the answer itself does not fit.

Every function is a host function shared by the interpreter and
compiled programs, so both give the same result for the same input.

## Why not other ways

- **Return NaN or infinity as C does.** `math.log(0.0)` already traps.
  One module with two policies is harder to learn than either policy.
- **Methods on `float64`.** `value.sqrt()` reads well, but the existing
  functions are module functions, and two spellings of one operation
  is a cost with no gain. Methods can be added later.
- **Generic over `float32`.** The module performs no implicit numeric
  conversion. `float32` versions wait for a decision on generic
  numeric functions.

## Consequences

- Results of the transcendental functions come from the host's math
  library and may differ in the last bit between hosts, as `sin` and
  `exp` already may. `sqrt` is correctly rounded everywhere.
- `aura verify` treats the new functions as it treats `math.sin`: it
  does not reason about their values.
- Freestanding builds do not link `math`, as before.
