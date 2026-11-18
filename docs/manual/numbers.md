# Numbers Module

Import `numbers` for arbitrary-size integers, exact decimal arithmetic, and
complex arithmetic. Convert values explicitly before combining them. A
literal such as `3` has type `int64`.

<<< ../../examples/numbers/exact_numbers.au{aura}

## BigInt

`BigInt.from_int(n)` takes an `int64`. `BigInt.parse(text)` reads a signed
decimal integer and returns `BigInt | None`. For another base, pass a radix
from 2 through 36 to `BigInt.parse_radix(text, radix)`. Digits may include
upper- or lowercase letters. Omit any base prefix.
`BigInt.from_float(x)` truncates a finite `float64` toward zero and returns
`None` for infinity or NaN.

| Operation | Result |
| --- | --- |
| `a + b`, `a - b`, `a * b`, `-a` | Exact integer arithmetic. |
| `a // b`, `a % b`, `a.divmod(b)` | Floor quotient, remainder with the sign of the divisor, or both. |
| `a.pow(exponent)`, `a.pow_mod(exponent, modulus)` | Integer power with a nonnegative `int64` exponent, or modular power with `BigInt` arguments. |
| `a.gcd(b)`, `a.lcm(b)`, `a.sqrt()` | Nonnegative greatest common divisor, least common multiple, or floor square root. |
| `a.bit_and(b)`, `a.bit_or(b)`, `a.bit_xor(b)` | Bitwise arithmetic with a two's-complement interpretation for negative integers. |
| `a.shift_left(count)`, `a.shift_right(count)` | Shift by a nonnegative `int64`. Right shift rounds toward negative infinity. |
| `a.bit_length()`, `a.is_even()`, `a.sign()`, `a.is_zero()` | Magnitude bits, parity, sign from -1 to 1, or zero test. |
| `a.compare(b)`, `a.abs()`, `a.copy()` | Ordering from -1 to 1, absolute value, or a separate equal value. |
| `a.to_int()`, `a.to_float()`, `a.to_radix(radix)` | `int64 | None`, nearest `float64`, or signed text in the chosen radix. |
| `numbers.factorial(n)` | The factorial of a nonnegative `int64`, returned as a `BigInt`. |

Use `str(a)` for decimal text and the usual comparison operators for two
`BigInt` values.

## Decimal

A `Decimal` holds an integer coefficient times ten raised to a signed
exponent. `Decimal.from_parts(BigInt.from_int(1995), -2)` represents 19.95
exactly. `from_int` and `from_big` lift integers. `Decimal.parse(text)`
accepts a sign, decimal point, and optional exponent and returns
`Decimal | None`. `Decimal.from_float(x)` preserves the exact binary value
of a finite float. To represent a price, start with decimal text or integer
parts.

Addition, subtraction, multiplication and nonnegative integer powers are
exact. The runtime removes trailing zeros from the coefficient, so equal
values compare equal independent of the input scale. Zero has exponent
zero. `a.coefficient()` and `a.exponent()` expose these normalized parts.

| Operation | Result |
| --- | --- |
| `a / b` | Quotient rounded to 28 significant digits, with ties to even. |
| `a.divide(b, digits, rounding)` | Quotient rounded to the requested number of significant digits. |
| `a.round(places, rounding)` | Rounds to decimal places. Negative places round to tens, hundreds, and beyond. |
| `a.sqrt(digits)`, `a.pow(exponent)` | Positive square root rounded to significant digits, or exact nonnegative integer power. |
| `a.to_big(rounding)`, `a.trunc()`, `a.floor()`, `a.ceil()` | An integer rounded in the requested direction. |
| `a.to_plain()`, `a.to_fixed(places)` | Unscaled display text, or fixed-point text rounded half to even with exactly the requested decimal places. |
| `a.to_float()`, `str(a)` | A `float64` or a decimal string which may use exponent notation. |
| `a.compare(b)`, `a.abs()`, `a.copy()`, `a.sign()`, `a.is_zero()` | Ordering, absolute value, a separate equal value, sign, or zero test. |

`Rounding.HalfEven` chooses the nearest value and sends a tie to the even
last digit. `HalfUp` sends a tie away from zero and `HalfDown` toward zero.
`Up` and `Down` always round away from or toward zero. `Floor` and
`Ceiling` round toward negative and positive infinity. Decimal arithmetic
uses the precision and rounding mode of the particular operation.

## Complex

`Complex(re=x, im=y)` stores two `float64` components. Both fields are
public. Use `+`, `-`, `*`, `/`, unary `-`, and equality on complex values.
`from_polar(radius, angle)` takes an angle in radians. `abs()` returns the
magnitude and `arg()` the angle. `conj()` negates the imaginary component.
`exp()`, `log()`, `sqrt()`, and `pow(exponent)` calculate complex functions
using the principal branch. `is_finite()` tests both components.

## Limits

Invalid text gives `None`. An invalid arithmetic argument such as a zero
integer divisor or a negative factorial gives `AU4001`. Decimal division and
square root accept 1 to 1,000,000 significant digits. A normalized decimal
exponent must lie between −2⁶² and 2⁶² inclusive. An exponent outside that
range traps with `AU4002`. A requested power, shift, factorial or decimal
scale which exceeds the runtime's construction limit traps with `AU4005`.
