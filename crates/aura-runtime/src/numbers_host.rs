//! The arbitrary-precision kernels behind the `numbers` module.
//!
//! `numbers.BigInt` and `numbers.Decimal` are Aura classes. A value that
//! fits `int64` uses the scalar member of the private storage union. The
//! other member keeps the magnitude in a packed list. Most host calls take
//! a scalar plus a list (empty for small values).
//!
//! A result travels back as one `list[uint64]`, the wire form:
//!
//! | index | meaning |
//! | --- | --- |
//! | 0 | 0 when the integer fits `int64`, otherwise 1 |
//! | 1 | 1 when the integer is negative |
//! | 2 | 1 when the decimal exponent is negative |
//! | 3 | the magnitude of the decimal exponent |
//! | 4.. | the magnitude, least significant 64 bits first, at least one |
//!
//! The list member of a big value is the wire form itself, so a result is
//! stored without a copy. A function that cannot produce a value, such as a
//! parse of malformed text, returns an empty list.

use std::borrow::Cow;
use std::cmp::Ordering;

use num_bigint::{BigInt, BigUint, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::diag::{Diagnostic, Result};
use crate::integer::IntegerValue;
use crate::list_elements::ListElements;
use crate::runtime_value::{SharedType, Value, VecValue};
use std::sync::OnceLock;

/// The words before the magnitude in the wire form.
const HEADER: usize = 4;

/// The largest result, in bits, that `pow`, a shift, or `factorial` may
/// build: 512 MiB of magnitude.
const MAX_RESULT_BITS: u64 = 1 << 32;

/// The largest power of ten, as an exponent, that aligning or scaling a
/// decimal may build. A decimal with more digits than a `str` can hold
/// cannot be written out, so the limit is the string limit.
const MAX_SCALE_DIGITS: u64 = crate::runtime_value::MAX_STRING_BYTES as u64;

/// The most digits a magnitude of `bits` bits has in `radix`, rounded up.
fn digits_in_radix(bits: u64, radix: u32) -> u64 {
    (bits as f64 / f64::from(radix).log2()).ceil() as u64 + 1
}

/// Refuses a text of up to `bytes` bytes that the string limit does not
/// allow, before the text is built.
fn text_fits(bytes: u64, what: &str) -> Result<()> {
    if bytes > crate::runtime_value::MAX_STRING_BYTES as u64 {
        Err(too_large(what))
    } else {
        Ok(())
    }
}

fn domain(message: impl Into<String>) -> Diagnostic {
    Diagnostic::coded("AU4001", message)
}

fn overflow(message: impl Into<String>) -> Diagnostic {
    Diagnostic::coded("AU4002", message)
}

fn too_large(what: &str) -> Diagnostic {
    Diagnostic::coded(
        "AU4005",
        format!("`numbers`: {what} is larger than the runtime's size limit"),
    )
}

fn expect_arity(name: &str, args: &[Value], count: usize) -> Result<()> {
    if args.len() == count {
        Ok(())
    } else {
        Err(domain(format!(
            "`{name}` expects {count} arguments, found {}",
            args.len()
        )))
    }
}

fn int_argument(name: &str, value: &Value) -> Result<i64> {
    match value {
        Value::Int(value) => value.as_i128().and_then(|value| i64::try_from(value).ok()),
        _ => None,
    }
    .ok_or_else(|| domain(format!("`{name}` expects an `int64` argument")))
}

fn float_argument(name: &str, value: &Value) -> Result<f64> {
    match value {
        Value::Float(value) => Ok(*value),
        _ => Err(domain(format!("`{name}` expects a `float64` argument"))),
    }
}

fn text_argument<'a>(name: &str, value: &'a Value) -> Result<&'a str> {
    match value {
        Value::String(text) => Ok(text.as_str()),
        _ => Err(domain(format!("`{name}` expects a `str` argument"))),
    }
}

/// The elements of a `list[uint64]` argument.
fn words<'a>(name: &str, value: &'a Value) -> Result<Cow<'a, [u64]>> {
    let Value::Vec(list) = value else {
        return Err(domain(format!(
            "`{name}` expects a `list[uint64]` argument"
        )));
    };
    if let ListElements::Uint64(words) = &list.elements {
        return Ok(Cow::Borrowed(words));
    }
    list.elements
        .iter()
        .map(|element| match &*element {
            Value::Int(word) => word.as_i128().and_then(|word| u64::try_from(word).ok()),
            _ => None,
        })
        .collect::<Option<Vec<u64>>>()
        .map(Cow::Owned)
        .ok_or_else(|| domain(format!("`{name}` expects a `list[uint64]` argument")))
}

fn magnitude(limbs: &[u64]) -> BigUint {
    let mut halves = Vec::with_capacity(limbs.len() * 2);
    for limb in limbs {
        halves.push(*limb as u32);
        halves.push((*limb >> 32) as u32);
    }
    BigUint::new(halves)
}

/// The integer in `args[index]` (the `small` field) and `args[index + 1]`
/// (the `digits` field).
fn integer(name: &str, args: &[Value], index: usize) -> Result<BigInt> {
    let small = int_argument(name, &args[index])?;
    let digits = words(name, &args[index + 1])?;
    if digits.is_empty() {
        return Ok(BigInt::from(small));
    }
    if digits.len() <= HEADER {
        return Err(domain(format!("`{name}` was given malformed digits")));
    }
    let sign = if digits[1] == 1 {
        Sign::Minus
    } else {
        Sign::Plus
    };
    Ok(BigInt::from_biguint(sign, magnitude(&digits[HEADER..])))
}

/// The decimal in `args[index..index + 3]`: `small`, `digits`, `exponent`.
fn decimal(name: &str, args: &[Value], index: usize) -> Result<(BigInt, i64)> {
    Ok((
        integer(name, args, index)?,
        int_argument(name, &args[index + 2])?,
    ))
}

fn word_list(words: Vec<u64>) -> Value {
    let elements = if words.is_empty() {
        ListElements::new()
    } else {
        ListElements::Uint64(words.into())
    };
    static WORD_TYPE: OnceLock<SharedType> = OnceLock::new();
    Value::Vec(VecValue {
        element_type: WORD_TYPE
            .get_or_init(|| SharedType::named("uint64"))
            .clone(),
        elements,
        str_view: Default::default(),
    })
}

fn no_value() -> Value {
    word_list(Vec::new())
}

/// The wire form of `value` with decimal exponent `exponent`.
fn wire(value: &BigInt, exponent: i64) -> Value {
    let limbs = value.iter_u64_digits();
    let length = HEADER + limbs.len().max(1);
    if !crate::builtin_limit::fits(length as u64 * 8) {
        return no_value();
    }
    let mut out = Vec::with_capacity(length);
    out.push(u64::from(value.to_i64().is_none()));
    out.push(u64::from(value.is_negative()));
    out.push(u64::from(exponent < 0));
    out.push(exponent.unsigned_abs());
    if value.is_zero() {
        out.push(0);
    } else {
        out.extend(limbs);
    }
    word_list(out)
}

fn int_value(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn power_of_ten(digits: u64) -> Result<BigInt> {
    // 3322 / 1000 bounds log2(10) from above.
    crate::builtin_limit::check(digits.saturating_mul(3322).div_ceil(8000))?;
    if digits > MAX_SCALE_DIGITS {
        return Err(too_large("a power of ten"));
    }
    Ok(BigInt::from(10u8).pow(digits as u32))
}

// 30103 / 100000 is greater than log10(2).
fn decimal_digit_bound(value: &BigInt) -> u64 {
    value.bits().saturating_mul(30103).div_ceil(100000).max(1)
}

/// How many decimal digits `value` has. Zero has one.
fn decimal_digits(value: &BigInt) -> Result<u64> {
    if value.is_zero() {
        return Ok(1);
    }
    let bits = value.bits();
    if bits < 64 {
        let mut magnitude = value.magnitude().to_u64().unwrap_or(0);
        let mut digits = 0;
        while magnitude > 0 {
            magnitude /= 10;
            digits += 1;
        }
        return Ok(digits);
    }
    crate::builtin_limit::check(decimal_digit_bound(value))?;
    Ok(value.magnitude().to_str_radix(10).len() as u64)
}

/// Removes trailing decimal zeros from `coefficient`, raising `exponent`
/// by one for each. Zero becomes `0` with exponent 0.
fn normalized(mut coefficient: BigInt, mut exponent: i64) -> Result<(BigInt, i64)> {
    if coefficient.is_zero() {
        return Ok((coefficient, 0));
    }
    // A number divisible by 10^k is divisible by 2^k, so the trailing zero
    // bits bound the zeros and most odd numbers skip the search.
    let budget = coefficient.trailing_zeros().unwrap_or(0);
    if budget == 0 {
        return Ok((coefficient, exponent));
    }
    // Divide by 10, 10^2, 10^4, ... while each divides, then by the same
    // powers from the largest down, so that k zeros take about 2 log k
    // divisions instead of k.
    let mut removed: u64 = 0;
    let mut powers = vec![BigInt::from(10u8)];
    loop {
        let step = 1u64 << (powers.len() - 1);
        if removed + step > budget {
            break;
        }
        let power = powers.last().expect("one power");
        let (quotient, remainder) = coefficient.div_rem(power);
        if !remainder.is_zero() {
            break;
        }
        coefficient = quotient;
        removed += step;
        if power.bits() > coefficient.bits() {
            break;
        }
        let square = power * power;
        powers.push(square);
    }
    for (index, power) in powers.iter().enumerate().rev() {
        let step = 1u64 << index;
        if removed + step > budget || power.bits() > coefficient.bits() + 1 {
            continue;
        }
        let (quotient, remainder) = coefficient.div_rem(power);
        if remainder.is_zero() {
            coefficient = quotient;
            removed += step;
        }
    }
    exponent = exponent
        .checked_add_unsigned(removed)
        .ok_or_else(|| overflow("`numbers.Decimal` exponent overflow"))?;
    Ok((coefficient, exponent))
}

/// The largest magnitude of a decimal exponent: 2^62.
const MAX_EXPONENT: u64 = 1 << 62;

/// A normalized word-sized result, or None when the caller must use the
/// arbitrary-precision operation (which also handles out-of-range exponents).
pub(crate) fn small_decimal_binary(
    multiply: bool,
    a: i64,
    a_power: i64,
    b: i64,
    b_power: i64,
) -> Option<[i64; 2]> {
    let (mut value, mut power) = if multiply {
        (a.checked_mul(b)?, a_power.checked_add(b_power)?)
    } else if a == 0 {
        (b, b_power)
    } else if b == 0 {
        (a, a_power)
    } else {
        let power = a_power.min(b_power);
        let scale = |value: i64, exponent: i64| {
            let gap = u32::try_from(exponent.abs_diff(power)).ok()?;
            value.checked_mul(10i64.checked_pow(gap)?)
        };
        (scale(a, a_power)?.checked_add(scale(b, b_power)?)?, power)
    };
    if value == 0 {
        return Some([0, 0]);
    }
    while value % 10 == 0 {
        value /= 10;
        power = power.checked_add(1)?;
    }
    (power.unsigned_abs() <= MAX_EXPONENT).then_some([value, power])
}

fn decimal_wire(coefficient: BigInt, exponent: i64) -> Result<Value> {
    let (coefficient, exponent) = normalized(coefficient, exponent)?;
    if exponent.unsigned_abs() > MAX_EXPONENT {
        return Err(overflow("`numbers.Decimal` exponent overflow"));
    }
    Ok(wire(&coefficient, exponent))
}

/// `numerator / denominator` rounded to an integer by `mode`. `sticky`
/// says the true numerator is a little further from zero than given.
fn rounded_quotient(
    numerator: &BigInt,
    denominator: &BigInt,
    mode: i64,
    sticky: bool,
) -> Result<BigInt> {
    let negative = numerator.is_negative() != denominator.is_negative();
    let (quotient, remainder) = numerator.div_rem(denominator);
    let inexact = !remainder.is_zero() || sticky;
    let half = (remainder.magnitude() << 1u32).cmp(denominator.magnitude());
    let away = match mode {
        // Half to even.
        0 => match half {
            Ordering::Greater => true,
            Ordering::Equal => sticky || quotient.is_odd(),
            Ordering::Less => false,
        },
        // Half away from zero.
        1 => half != Ordering::Less,
        // Half toward zero.
        2 => half == Ordering::Greater || (half == Ordering::Equal && sticky),
        // Toward zero.
        3 => false,
        // Away from zero.
        4 => inexact,
        // Toward negative infinity.
        5 => inexact && negative,
        // Toward positive infinity.
        6 => inexact && !negative,
        _ => return Err(domain("`numbers.Decimal`: unknown rounding mode")),
    };
    Ok(match (away, negative) {
        (false, _) => quotient,
        (true, false) => quotient + BigInt::one(),
        (true, true) => quotient - BigInt::one(),
    })
}

/// `coefficient * 10^exponent` rounded to `digits` significant digits.
fn rounded_to_digits(
    coefficient: BigInt,
    exponent: i64,
    digits: i64,
    mode: i64,
    sticky: bool,
) -> Result<(BigInt, i64)> {
    let have = decimal_digits(&coefficient)?;
    let digits = digits as u64;
    if have <= digits {
        return Ok((coefficient, exponent));
    }
    let drop = have - digits;
    let quotient = rounded_quotient(&coefficient, &power_of_ten(drop)?, mode, sticky)?;
    let exponent = i64::try_from(drop)
        .ok()
        .and_then(|drop| exponent.checked_add(drop))
        .ok_or_else(|| overflow("`numbers.Decimal` exponent overflow"))?;
    Ok((quotient, exponent))
}

fn positive_digits(name: &str, digits: i64) -> Result<i64> {
    if (1..=1_000_000).contains(&digits) {
        Ok(digits)
    } else {
        Err(domain(format!(
            "`{name}` needs between 1 and 1000000 digits, found {digits}"
        )))
    }
}

fn result_bits_fit(bits: u64) -> Result<()> {
    crate::builtin_limit::check(bits.div_ceil(8))?;
    if bits > MAX_RESULT_BITS {
        Err(too_large("a result"))
    } else {
        Ok(())
    }
}

/// `numbers::__aura_big_binary(op, a_small, a_digits, b_small, b_digits)`.
pub fn host_big_binary(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "numbers.BigInt";
    expect_arity(name, &args, 5)?;
    let op = int_argument(name, &args[0])?;
    let left = integer(name, &args, 1)?;
    let right = integer(name, &args, 3)?;
    let wider = left.bits().max(right.bits());
    let both = left.bits().saturating_add(right.bits());
    if crate::builtin_limit::active() {
        let bits = if matches!(op, 2 | 12) {
            left.bits().saturating_add(right.bits())
        } else {
            left.bits().max(right.bits()).saturating_add(1)
        };
        crate::builtin_limit::check((HEADER as u64 + bits.div_ceil(64)) * 8)?;
    }
    let result = match op {
        0 | 1 => {
            result_bits_fit(wider.saturating_add(1))?;
            if op == 0 {
                left + right
            } else {
                left - right
            }
        }
        2 => {
            result_bits_fit(both)?;
            left * right
        }
        3 | 4 => {
            if right.is_zero() {
                return Err(domain("division by zero"));
            }
            let (quotient, remainder) = left.div_mod_floor(&right);
            if op == 3 {
                quotient
            } else {
                remainder
            }
        }
        5 => left.gcd(&right),
        6 => {
            let exponent = right
                .to_u32()
                .ok_or_else(|| domain("`BigInt.pow` needs an exponent from 0 to 4294967295"))?;
            if left.bits() > 1 {
                result_bits_fit(left.bits().saturating_mul(u64::from(exponent)))?;
            }
            left.pow(exponent)
        }
        7 | 8 => {
            let count = right
                .to_u64()
                .ok_or_else(|| domain("a `BigInt` shift count must not be negative"))?;
            if op == 7 {
                if !left.is_zero() {
                    result_bits_fit(left.bits().saturating_add(count))?;
                }
                left << count
            } else if count >= left.bits() {
                // An arithmetic shift of everything leaves the sign.
                if left.is_negative() {
                    -BigInt::one()
                } else {
                    BigInt::zero()
                }
            } else {
                left >> count
            }
        }
        9 => left & right,
        10 => left | right,
        11 => left ^ right,
        12 => {
            result_bits_fit(both)?;
            left.lcm(&right)
        }
        _ => return Err(domain("`numbers.BigInt`: unknown operation")),
    };
    Ok(wire(&result, 0))
}

/// `numbers::__aura_big_compare(a_small, a_digits, b_small, b_digits)`:
/// -1, 0, or 1.
pub fn host_big_compare(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "numbers.BigInt";
    expect_arity(name, &args, 4)?;
    let order = integer(name, &args, 0)?.cmp(&integer(name, &args, 2)?);
    Ok(int_value(order as i64))
}

/// Adds the two non-inline operands without importing them into a second
/// array of digits. The common sign and limb order are already known.
pub(crate) fn add_big_words(left: &Value, right: &Value) -> Result<Value> {
    let a = words("BigInt.add", left)?;
    let b = words("BigInt.add", right)?;
    if a.len() <= HEADER || b.len() <= HEADER {
        return Err(domain("BigInt.add received an incomplete magnitude"));
    }
    if a[1] != b[1] {
        let signed = |words: &[u64]| {
            let sign = if words[1] == 1 {
                Sign::Minus
            } else {
                Sign::Plus
            };
            BigInt::from_biguint(sign, magnitude(&words[HEADER..]))
        };
        return Ok(wire(&(signed(&a) + signed(&b)), 0));
    }
    let negative = u64::from(a[1] == 1);
    let (longer, shorter) = if a.len() >= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    // The copy preserves the longer operand’s unmatched high words. Read
    // both addends independently so the carry loop can write sums from registers.
    let mut result = Vec::with_capacity(longer.len() + 1);
    result.extend_from_slice(longer);
    result[..HEADER].copy_from_slice(&[1, negative, 0, 0]);
    let (common, remaining) = result[HEADER..].split_at_mut(shorter.len() - HEADER);
    #[inline]
    fn add_digit(carry: u8, lhs: u64, rhs: u64, out: &mut u64) -> u8 {
        #[cfg(target_arch = "x86_64")]
        {
            std::arch::x86_64::_addcarry_u64(carry, lhs, rhs, out)
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let (value, next) = lhs.carrying_add(rhs, carry != 0);
            *out = value;
            u8::from(next)
        }
    }
    let mut carry = 0;
    // A fixed group carries the processor flag through consecutive digits.
    let (groups, tail) = common.as_chunks_mut::<16>();
    let (lhs_groups, lhs_tail) = longer[HEADER..shorter.len()].as_chunks::<16>();
    let (rhs_groups, rhs_tail) = shorter[HEADER..].as_chunks::<16>();
    for ((out, lhs), rhs) in groups.iter_mut().zip(lhs_groups).zip(rhs_groups) {
        for index in 0..16 {
            carry = add_digit(carry, lhs[index], rhs[index], &mut out[index]);
        }
    }
    for ((out, &lhs), &rhs) in tail.iter_mut().zip(lhs_tail).zip(rhs_tail) {
        carry = add_digit(carry, lhs, rhs, out);
    }
    for digit in remaining {
        if carry == 0 {
            break;
        }
        let (value, next) = digit.overflowing_add(1);
        *digit = value;
        carry = u8::from(next);
    }
    if carry != 0 {
        result.push(1);
    }
    // The private host function may also be called on two small wire
    // values. Preserve the canonical result at that boundary.
    if let [_, _, _, _, limb] = result.as_slice() {
        result[0] = u64::from(*limb > (i64::MAX as u64) + negative);
    }
    Ok(word_list(result))
}

pub fn host_big_add_words(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    expect_arity("BigInt.add", &args, 2)?;
    add_big_words(&args[0], &args[1])
}

pub(crate) fn small_pow_mod(base: i64, exponent: i64, modulus: i64) -> Result<i64> {
    let exponent = u64::try_from(exponent)
        .map_err(|_| domain("`BigInt.pow_mod` needs an exponent of 0 or more"))?;
    let magnitude = modulus.unsigned_abs();
    if magnitude == 0 {
        return Err(domain("division by zero"));
    }
    fn power(
        mut base: u64,
        mut exponent: u64,
        modulus: u64,
        multiply: impl Fn(u64, u64) -> u64,
    ) -> u64 {
        let mut result = 1 % modulus;
        while exponent != 0 {
            if exponent & 1 != 0 {
                result = multiply(result, base);
            }
            exponent >>= 1;
            if exponent != 0 {
                base = multiply(base, base);
            }
        }
        result
    }
    let factor = i128::from(base).rem_euclid(i128::from(magnitude)) as u64;
    // Every reduced product fits a word for these moduli. Larger moduli
    // use a double-width product before reduction.
    let result = if u32::try_from(magnitude).is_ok() {
        power(factor, exponent, magnitude, |x, y| (x * y) % magnitude)
    } else {
        power(factor, exponent, magnitude, |x, y| {
            ((u128::from(x) * u128::from(y)) % u128::from(magnitude)) as u64
        })
    };
    if modulus < 0 && result != 0 {
        Ok((i128::from(result) - i128::from(magnitude)) as i64)
    } else {
        Ok(result as i64)
    }
}

pub fn host_small_pow_mod(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "BigInt.pow_mod";
    expect_arity(name, &args, 3)?;
    small_pow_mod(
        int_argument(name, &args[0])?,
        int_argument(name, &args[1])?,
        int_argument(name, &args[2])?,
    )
    .map(int_value)
}

/// `numbers::__aura_big_pow_mod(base.., exponent.., modulus..)`.
pub fn host_big_pow_mod(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "BigInt.pow_mod";
    expect_arity(name, &args, 6)?;
    let base = integer(name, &args, 0)?;
    let exponent = integer(name, &args, 2)?;
    let modulus = integer(name, &args, 4)?;
    if exponent.is_negative() {
        return Err(domain("`BigInt.pow_mod` needs an exponent of 0 or more"));
    }
    if modulus.is_zero() {
        return Err(domain("division by zero"));
    }
    Ok(wire(&base.modpow(&exponent, &modulus), 0))
}

/// `numbers::__aura_big_unary(op, small, digits)`: 0 is the integer square
/// root, and 1 the factorial.
pub fn host_big_unary(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "numbers.BigInt";
    expect_arity(name, &args, 3)?;
    let value = integer(name, &args, 1)?;
    let result = match int_argument(name, &args[0])? {
        0 => {
            if value.is_negative() {
                return Err(domain("`BigInt.sqrt` of a negative number"));
            }
            value.sqrt()
        }
        1 => {
            let count = value
                .to_u64()
                .ok_or_else(|| domain("`numbers.factorial` of a negative number"))?;
            // log2(n!) is below n * log2(n).
            result_bits_fit(count.saturating_mul(u64::from(64 - count.leading_zeros())))?;
            factorial(count)
        }
        _ => return Err(domain("`numbers.BigInt`: unknown operation")),
    };
    Ok(wire(&result, 0))
}

/// Multiply the odd parts in a balanced tree, then insert all powers of
/// two at once. Each leaf accumulates in a word when its product fits.
fn factorial(count: u64) -> BigInt {
    fn odd_product(low: u64, high: u64) -> BigUint {
        let terms = (high - low) / 2 + 1;
        if terms * u64::from(high.ilog2() + 1) <= 64 {
            return BigUint::from((low..=high).step_by(2).product::<u64>());
        }
        let middle = low + 2 * (terms / 2);
        odd_product(low, middle - 2) * odd_product(middle, high)
    }
    if count < 2 {
        return BigInt::one();
    }
    let mut inner = BigUint::one();
    let mut outer = BigUint::one();
    let mut first_odd = 3;
    for shift in (0..count.ilog2()).rev() {
        let limit = count >> shift;
        let last_odd = (limit - 1) | 1;
        if last_odd >= first_odd {
            inner *= odd_product(first_odd, last_odd);
            first_odd = last_odd + 2;
        }
        outer *= &inner;
    }
    BigInt::from(outer << (count - u64::from(count.count_ones())))
}

/// `numbers::__aura_big_bits(small, digits)`: the bits of the magnitude.
pub fn host_big_bits(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "BigInt.bit_length";
    expect_arity(name, &args, 2)?;
    Ok(int_value(integer(name, &args, 0)?.bits() as i64))
}

fn valid_digits(text: &str, radix: u32) -> bool {
    !text.is_empty() && text.bytes().all(|byte| char::from(byte).is_digit(radix))
}

/// `numbers::__aura_big_parse(text, radix)`: the wire form, or an empty
/// list when `text` is not an integer in that radix.
pub fn host_big_parse(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "BigInt.parse";
    expect_arity(name, &args, 2)?;
    let text = text_argument(name, &args[0])?;
    let radix = int_argument(name, &args[1])?;
    if !(2..=36).contains(&radix) {
        return Err(domain(format!(
            "`BigInt` radix must be from 2 to 36, found {radix}"
        )));
    }
    let radix = radix as u32;
    let (sign, digits) = match text.as_bytes().first() {
        Some(b'-') => (Sign::Minus, &text[1..]),
        Some(b'+') => (Sign::Plus, &text[1..]),
        _ => (Sign::Plus, text),
    };
    if !valid_digits(digits, radix) {
        return Ok(no_value());
    }
    Ok(match BigUint::parse_bytes(digits.as_bytes(), radix) {
        Some(magnitude) => wire(&BigInt::from_biguint(sign, magnitude), 0),
        None => no_value(),
    })
}

/// `numbers::__aura_big_format(small, digits, radix)`.
pub fn host_big_format(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "BigInt.to_radix";
    expect_arity(name, &args, 3)?;
    let value = integer(name, &args, 0)?;
    let radix = int_argument(name, &args[2])?;
    if !(2..=36).contains(&radix) {
        return Err(domain(format!(
            "`BigInt` radix must be from 2 to 36, found {radix}"
        )));
    }
    text_fits(
        digits_in_radix(value.bits(), radix as u32).saturating_add(1),
        "the text of a `BigInt`",
    )?;
    let bits_per_digit = u64::from((radix as u32).ilog2());
    crate::builtin_limit::check(value.bits().div_ceil(bits_per_digit).saturating_add(1))?;
    Ok(Value::string(value.to_str_radix(radix as u32)))
}

/// `numbers::__aura_big_to_float(small, digits)`: the nearest `float64`,
/// which is an infinity for a value beyond its range.
pub fn host_big_to_float(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "BigInt.to_float";
    expect_arity(name, &args, 2)?;
    let value = integer(name, &args, 0)?;
    Ok(Value::Float(value.to_f64().unwrap_or(f64::NAN)))
}

/// The exact value of a finite `float64` as `coefficient * 10^exponent`.
fn exact_float(value: f64) -> Option<(BigInt, i64)> {
    if !value.is_finite() {
        return None;
    }
    let bits = value.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i64;
    let fraction = bits & ((1 << 52) - 1);
    let (mantissa, binary) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), biased - 1075)
    };
    let mut coefficient = BigInt::from(mantissa);
    if value.is_sign_negative() {
        coefficient = -coefficient;
    }
    if binary >= 0 {
        return Some((coefficient << binary as u64, 0));
    }
    // 2^-k is 5^k * 10^-k.
    let five = BigInt::from(5u8).pow((-binary) as u32);
    Some((coefficient * five, binary))
}

/// `numbers::__aura_big_from_float(value)`: `value` with its fraction
/// dropped, or an empty list for NaN or an infinity.
pub fn host_big_from_float(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "BigInt.from_float";
    expect_arity(name, &args, 1)?;
    let Some((coefficient, exponent)) = exact_float(float_argument(name, &args[0])?.trunc()) else {
        return Ok(no_value());
    };
    // A truncated float is an integer, so the scale divides it exactly.
    let scale = power_of_ten(exponent.unsigned_abs())?;
    Ok(wire(&(coefficient / scale), 0))
}

/// Brings two decimals to the smaller of their exponents.
fn aligned(left: (BigInt, i64), right: (BigInt, i64)) -> Result<(BigInt, BigInt, i64)> {
    let (left_coefficient, left_exponent) = left;
    let (right_coefficient, right_exponent) = right;
    // Zero has no digits to line up.
    if left_coefficient.is_zero() {
        return Ok((left_coefficient, right_coefficient, right_exponent));
    }
    if right_coefficient.is_zero() {
        return Ok((left_coefficient, right_coefficient, left_exponent));
    }
    // Two `i64` exponents are at most `u64::MAX` apart.
    let gap = left_exponent.abs_diff(right_exponent);
    let scaled = if left_exponent >= right_exponent {
        &left_coefficient
    } else {
        &right_coefficient
    };
    // 10^gap has under 3.33 * gap bits.
    result_bits_fit(scaled.bits().saturating_add(gap.saturating_mul(10) / 3 + 1))?;
    let scale = power_of_ten(gap)?;
    Ok(if left_exponent >= right_exponent {
        (left_coefficient * scale, right_coefficient, right_exponent)
    } else {
        (left_coefficient, right_coefficient * scale, left_exponent)
    })
}

fn exponent_sum(terms: &[i128]) -> Result<i64> {
    i64::try_from(terms.iter().sum::<i128>())
        .map_err(|_| overflow("`numbers.Decimal` exponent overflow"))
}

/// `left / right` to `digits` significant digits.
fn divided(
    left: (BigInt, i64),
    right: (BigInt, i64),
    digits: i64,
    mode: i64,
) -> Result<(BigInt, i64)> {
    let (numerator, numerator_exponent) = left;
    let (denominator, denominator_exponent) = right;
    if denominator.is_zero() {
        return Err(domain("division by zero"));
    }
    if numerator.is_zero() {
        return Ok((numerator, 0));
    }
    // The quotient of the coefficients has `gap` or `gap + 1` digits before
    // its point. Scale so that the integer quotient has `digits` or
    // `digits + 1` digits, then round once.
    let gap = decimal_digits(&numerator)? as i128 - decimal_digits(&denominator)? as i128;
    let shift = i128::from(digits) - gap;
    let scale = power_of_ten(
        u64::try_from(shift.unsigned_abs()).map_err(|_| too_large("a power of ten"))?,
    )?;
    let (scaled_numerator, scaled_denominator) = if shift >= 0 {
        (numerator * scale, denominator)
    } else {
        (numerator, denominator * scale)
    };
    let (quotient, remainder) = scaled_numerator.div_rem(&scaled_denominator);
    let exponent = exponent_sum(&[
        i128::from(numerator_exponent),
        -i128::from(denominator_exponent),
        -shift,
    ])?;
    if decimal_digits(&quotient)? <= digits as u64 {
        // No digit to drop: round the last place from the remainder.
        let rounded = rounded_quotient(&scaled_numerator, &scaled_denominator, mode, false)?;
        return Ok((rounded, exponent));
    }
    rounded_to_digits(quotient, exponent, digits, mode, !remainder.is_zero())
}

/// `numbers::__aura_dec_binary(op, a.., b.., digits, rounding)`: 0 adds,
/// 1 subtracts, 2 multiplies, 3 divides to `digits` significant digits, and
/// 4 raises to an integer power.
pub fn host_dec_binary(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "numbers.Decimal";
    expect_arity(name, &args, 9)?;
    let op = int_argument(name, &args[0])?;
    let left = decimal(name, &args, 1)?;
    let right = decimal(name, &args, 4)?;
    let (coefficient, exponent) = match op {
        0 | 1 => {
            let (left, right, exponent) = aligned(left, right)?;
            (if op == 0 { left + right } else { left - right }, exponent)
        }
        2 => {
            result_bits_fit(left.0.bits().saturating_add(right.0.bits()))?;
            (
                left.0 * right.0,
                exponent_sum(&[i128::from(left.1), i128::from(right.1)])?,
            )
        }
        3 => {
            let digits = positive_digits("Decimal.divide", int_argument(name, &args[7])?)?;
            divided(left, right, digits, int_argument(name, &args[8])?)?
        }
        4 => {
            // `right` is an integer written with exponent 0.
            let power =
                right.0.to_u32().filter(|_| right.1 == 0).ok_or_else(|| {
                    domain("`Decimal.pow` needs an exponent from 0 to 4294967295")
                })?;
            if left.0.bits() > 1 {
                result_bits_fit(left.0.bits().saturating_mul(u64::from(power)))?;
            }
            (
                left.0.pow(power),
                exponent_sum(&[i128::from(left.1) * i128::from(power)])?,
            )
        }
        _ => return Err(domain("`numbers.Decimal`: unknown operation")),
    };
    decimal_wire(coefficient, exponent)
}

/// `numbers::__aura_dec_compare(a.., b..)`: -1, 0, or 1.
pub fn host_dec_compare(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "numbers.Decimal";
    expect_arity(name, &args, 6)?;
    let left = decimal(name, &args, 0)?;
    let right = decimal(name, &args, 3)?;
    let order = match (left.0.sign(), right.0.sign()) {
        (left_sign, right_sign) if left_sign != right_sign => left_sign.cmp(&right_sign),
        (Sign::NoSign, _) => Ordering::Equal,
        (sign, _) => {
            // Compare by size first, which needs no scaling.
            let left_top = decimal_digits(&left.0)? as i128 + i128::from(left.1);
            let right_top = decimal_digits(&right.0)? as i128 + i128::from(right.1);
            let by_size = if sign == Sign::Minus {
                right_top.cmp(&left_top)
            } else {
                left_top.cmp(&right_top)
            };
            if by_size == Ordering::Equal {
                let (left, right, _) = aligned(left, right)?;
                left.cmp(&right)
            } else {
                by_size
            }
        }
    };
    Ok(int_value(order as i64))
}

/// `coefficient * 10^exponent` rounded to `places` digits after the point.
fn rounded_to_places(
    coefficient: BigInt,
    exponent: i64,
    places: i64,
    mode: i64,
) -> Result<(BigInt, i64)> {
    let target = places
        .checked_neg()
        .ok_or_else(|| overflow("`numbers.Decimal` exponent overflow"))?;
    if exponent >= target || coefficient.is_zero() {
        return Ok((coefficient, exponent));
    }
    let drop = (i128::from(target) - i128::from(exponent)) as u128;
    if drop > u128::from(decimal_digits(&coefficient)?) + 1 {
        // Every digit is dropped: the result is zero or one unit.
        let unit = rounded_quotient(&coefficient.signum(), &BigInt::from(4u8), mode, false)?;
        return Ok((unit, target));
    }
    let quotient = rounded_quotient(&coefficient, &power_of_ten(drop as u64)?, mode, false)?;
    Ok((quotient, target))
}

/// `numbers::__aura_dec_round(a.., places, rounding)`.
pub fn host_dec_round(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "Decimal.round";
    expect_arity(name, &args, 5)?;
    let (coefficient, exponent) = decimal(name, &args, 0)?;
    let places = int_argument(name, &args[3])?;
    let mode = int_argument(name, &args[4])?;
    let (coefficient, exponent) = rounded_to_places(coefficient, exponent, places, mode)?;
    decimal_wire(coefficient, exponent)
}

/// `numbers::__aura_dec_integer(a.., rounding)`: the decimal rounded to an
/// integer, as a `BigInt`.
pub fn host_dec_integer(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "numbers.Decimal";
    expect_arity(name, &args, 4)?;
    let (coefficient, exponent) = decimal(name, &args, 0)?;
    let mode = int_argument(name, &args[3])?;
    let (coefficient, exponent) = rounded_to_places(coefficient, exponent, 0, mode)?;
    Ok(wire(
        &(coefficient * power_of_ten(exponent.unsigned_abs())?),
        0,
    ))
}

/// `numbers::__aura_dec_normalize(small, digits, exponent)`.
pub fn host_dec_normalize(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "numbers.Decimal";
    expect_arity(name, &args, 3)?;
    let (coefficient, exponent) = decimal(name, &args, 0)?;
    decimal_wire(coefficient, exponent)
}

fn parsed_decimal(text: &str) -> Option<(BigInt, i64)> {
    let (sign, rest) = match text.as_bytes().first() {
        Some(b'-') => (Sign::Minus, &text[1..]),
        Some(b'+') => (Sign::Plus, &text[1..]),
        _ => (Sign::Plus, text),
    };
    let (number, exponent) = match rest.find(['e', 'E']) {
        Some(at) => {
            let written = &rest[at + 1..];
            let unsigned = written.strip_prefix(['+', '-']).unwrap_or(written);
            if !valid_digits(unsigned, 10) {
                return None;
            }
            (&rest[..at], written.parse::<i64>().ok()?)
        }
        None => (rest, 0),
    };
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let digits = [whole, fraction].concat();
    if !valid_digits(&digits, 10) {
        return None;
    }
    let magnitude = BigUint::parse_bytes(digits.as_bytes(), 10)?;
    let exponent = exponent.checked_sub(i64::try_from(fraction.len()).ok()?)?;
    Some((BigInt::from_biguint(sign, magnitude), exponent))
}

/// `numbers::__aura_dec_parse(text)`: the wire form, or an empty list.
pub fn host_dec_parse(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "Decimal.parse";
    expect_arity(name, &args, 1)?;
    match parsed_decimal(text_argument(name, &args[0])?) {
        Some((coefficient, exponent)) => decimal_wire(coefficient, exponent),
        None => Ok(no_value()),
    }
}

/// Plain notation: every digit written out, with no exponent.
fn plain(coefficient: &BigInt, exponent: i64) -> Result<String> {
    // The digits, the zeros or places the exponent adds, a sign, a point
    // and a leading zero.
    text_fits(
        digits_in_radix(coefficient.bits(), 10)
            .saturating_add(exponent.unsigned_abs())
            .saturating_add(3),
        "the text of a decimal",
    )?;
    let digit_bound = decimal_digit_bound(coefficient);
    let length = if exponent >= 0 {
        digit_bound.saturating_add(exponent as u64)
    } else {
        digit_bound.max(exponent.unsigned_abs()).saturating_add(2)
    };
    crate::builtin_limit::check(length.saturating_add(1))?;
    let digits = coefficient.magnitude().to_str_radix(10);
    let sign = if coefficient.is_negative() { "-" } else { "" };
    if exponent >= 0 {
        if exponent.unsigned_abs() > MAX_SCALE_DIGITS {
            return Err(too_large("the text of a decimal"));
        }
        return Ok(format!("{sign}{digits}{}", "0".repeat(exponent as usize)));
    }
    let places = exponent.unsigned_abs();
    if places > MAX_SCALE_DIGITS {
        return Err(too_large("the text of a decimal"));
    }
    let places = places as usize;
    Ok(if digits.len() > places {
        let (whole, fraction) = digits.split_at(digits.len() - places);
        format!("{sign}{whole}.{fraction}")
    } else {
        format!("{sign}0.{}{digits}", "0".repeat(places - digits.len()))
    })
}

/// The text of a normalized decimal: plain when its first digit lies
/// between the seventh place after the point and the twenty-first before
/// it, and `d.ddde30` otherwise, as a `float64` prints.
fn display(coefficient: &BigInt, exponent: i64) -> Result<String> {
    // The digits, a sign, a point and an exponent of up to 20 characters.
    text_fits(
        digits_in_radix(coefficient.bits(), 10).saturating_add(23),
        "the text of a decimal",
    )?;
    crate::builtin_limit::check(decimal_digit_bound(coefficient).saturating_add(22))?;
    let digits = coefficient.magnitude().to_str_radix(10);
    let top = digits.len() as i128 + i128::from(exponent) - 1;
    if (-7..21).contains(&top) {
        return plain(coefficient, exponent);
    }
    let sign = if coefficient.is_negative() { "-" } else { "" };
    let (first, rest) = digits.split_at(1);
    let point = if rest.is_empty() { "" } else { "." };
    Ok(format!("{sign}{first}{point}{rest}e{top}"))
}

/// `numbers::__aura_dec_format(a.., places)`: with `places` of 0 or more,
/// plain notation rounded half to even to exactly that many digits after
/// the point. With -1, the shortest text. With -2, plain notation.
pub fn host_dec_format(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "Decimal.to_fixed";
    expect_arity(name, &args, 4)?;
    let (coefficient, exponent) = decimal(name, &args, 0)?;
    let places = int_argument(name, &args[3])?;
    let text = match places {
        -1 => display(&coefficient, exponent)?,
        -2 => plain(&coefficient, exponent)?,
        places if places < 0 => {
            return Err(domain("`Decimal.to_fixed` needs 0 or more places"));
        }
        places => {
            let negative = coefficient.is_negative();
            let (rounded, rounded_exponent) = rounded_to_places(coefficient, exponent, places, 0)?;
            // Bring the coefficient to exactly `places` digits.
            let target = -places;
            let padded = if rounded_exponent > target {
                let gap = (i128::from(rounded_exponent) - i128::from(target)) as u128;
                rounded
                    * power_of_ten(u64::try_from(gap).map_err(|_| too_large("a power of ten"))?)?
            } else {
                rounded
            };
            let mut text = plain(&padded, target)?;
            if negative && padded.is_zero() {
                text.insert(0, '-');
            }
            text
        }
    };
    Ok(Value::string(text))
}

/// `numbers::__aura_dec_to_float(a..)`: the nearest `float64`.
pub fn host_dec_to_float(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "Decimal.to_float";
    expect_arity(name, &args, 3)?;
    let (coefficient, exponent) = decimal(name, &args, 0)?;
    // The standard parser rounds a decimal text correctly.
    crate::builtin_limit::check(decimal_digit_bound(&coefficient).saturating_add(22))?;
    let text = format!("{coefficient}e{exponent}");
    Ok(Value::Float(text.parse::<f64>().unwrap_or(f64::NAN)))
}

/// `numbers::__aura_dec_from_float(value)`: the exact value of the float,
/// or an empty list for NaN or an infinity.
pub fn host_dec_from_float(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "Decimal.from_float";
    expect_arity(name, &args, 1)?;
    match exact_float(float_argument(name, &args[0])?) {
        Some((coefficient, exponent)) => decimal_wire(coefficient, exponent),
        None => Ok(no_value()),
    }
}

/// `numbers::__aura_dec_sqrt(a.., digits)`: the square root to `digits`
/// significant digits, rounded half to even.
pub fn host_dec_sqrt(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "Decimal.sqrt";
    expect_arity(name, &args, 4)?;
    let (coefficient, exponent) = decimal(name, &args, 0)?;
    let digits = positive_digits(name, int_argument(name, &args[3])?)?;
    if coefficient.is_negative() {
        return Err(domain("`Decimal.sqrt` of a negative number"));
    }
    if coefficient.is_zero() {
        return decimal_wire(coefficient, 0);
    }
    // Scale to an even exponent and enough digits that the integer root
    // carries one digit more than asked for.
    let have = decimal_digits(&coefficient)? as i128;
    let mut shift = (2 * i128::from(digits) + 2 - have).max(0);
    if (i128::from(exponent) - shift) % 2 != 0 {
        shift += 1;
    }
    let scaled = coefficient * power_of_ten(shift as u64)?;
    let root = scaled.sqrt();
    let exact = &root * &root == scaled;
    let root_exponent = exponent_sum(&[(i128::from(exponent) - shift) / 2])?;
    let (rounded, rounded_exponent) = rounded_to_digits(root, root_exponent, digits, 0, !exact)?;
    decimal_wire(rounded, rounded_exponent)
}

type HostFunction = fn(Vec<Value>, Option<&[String]>) -> Result<Value>;

/// The host function behind `numbers::__aura_*`.
pub fn numbers_host_function(name: &str) -> Option<HostFunction> {
    Some(match name {
        "numbers::__aura_big_binary" => host_big_binary,
        "numbers::__aura_big_add_words" => host_big_add_words,
        "numbers::__aura_small_pow_mod" => host_small_pow_mod,
        "numbers::__aura_big_compare" => host_big_compare,
        "numbers::__aura_big_pow_mod" => host_big_pow_mod,
        "numbers::__aura_big_unary" => host_big_unary,
        "numbers::__aura_big_bits" => host_big_bits,
        "numbers::__aura_big_parse" => host_big_parse,
        "numbers::__aura_big_format" => host_big_format,
        "numbers::__aura_big_to_float" => host_big_to_float,
        "numbers::__aura_big_from_float" => host_big_from_float,
        "numbers::__aura_dec_binary" => host_dec_binary,
        "numbers::__aura_dec_compare" => host_dec_compare,
        "numbers::__aura_dec_round" => host_dec_round,
        "numbers::__aura_dec_integer" => host_dec_integer,
        "numbers::__aura_dec_normalize" => host_dec_normalize,
        "numbers::__aura_dec_parse" => host_dec_parse,
        "numbers::__aura_dec_format" => host_dec_format,
        "numbers::__aura_dec_to_float" => host_dec_to_float,
        "numbers::__aura_dec_from_float" => host_dec_from_float,
        "numbers::__aura_dec_sqrt" => host_dec_sqrt,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "numbers_host_tests.rs"]
mod tests;
