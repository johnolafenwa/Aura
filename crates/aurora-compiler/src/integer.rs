use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::sema::Type;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum IntegerValue {
    Signed(i128),
    Unsigned(u128),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IntegerBounds {
    Signed { min: i128, max: i128 },
    Unsigned { max: u128 },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum IntegerSign {
    Negative,
    Zero,
    Positive,
}

impl PartialEq for IntegerValue {
    fn eq(&self, other: &Self) -> bool {
        Ord::cmp(self, other) == Ordering::Equal
    }
}

impl Eq for IntegerValue {}

impl PartialOrd for IntegerValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(Ord::cmp(self, other))
    }
}

impl Ord for IntegerValue {
    fn cmp(&self, other: &Self) -> Ordering {
        let (left_sign, left_mag) = self.sign_magnitude();
        let (right_sign, right_mag) = other.sign_magnitude();
        match (left_sign, right_sign) {
            (IntegerSign::Negative, IntegerSign::Negative) => right_mag.cmp(&left_mag),
            (IntegerSign::Negative, _) => Ordering::Less,
            (_, IntegerSign::Negative) => Ordering::Greater,
            (IntegerSign::Zero, IntegerSign::Zero) => Ordering::Equal,
            (IntegerSign::Zero, IntegerSign::Positive) => Ordering::Less,
            (IntegerSign::Positive, IntegerSign::Zero) => Ordering::Greater,
            (IntegerSign::Positive, IntegerSign::Positive) => left_mag.cmp(&right_mag),
        }
    }
}

impl fmt::Display for IntegerValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signed(value) => write!(f, "{}", value),
            Self::Unsigned(value) => write!(f, "{}", value),
        }
    }
}

impl IntegerValue {
    pub const fn zero() -> Self {
        Self::Unsigned(0)
    }

    pub const fn from_literal(value: u128) -> Self {
        Self::Unsigned(value)
    }

    pub fn from_signed(value: i128) -> Self {
        if value >= 0 {
            Self::Unsigned(value as u128)
        } else {
            Self::Signed(value)
        }
    }

    pub fn is_zero(&self) -> bool {
        matches!(self, Self::Signed(0) | Self::Unsigned(0))
    }

    pub fn to_f64(self) -> f64 {
        match self {
            Self::Signed(value) => value as f64,
            Self::Unsigned(value) => value as f64,
        }
    }

    pub fn to_exact_f64(self) -> Option<f64> {
        let (sign, magnitude) = self.sign_magnitude();
        let float = Self::exact_unsigned_f64(magnitude)?;
        Some(match sign {
            IntegerSign::Negative => -float,
            IntegerSign::Zero | IntegerSign::Positive => float,
        })
    }

    pub fn to_exact_f32(self) -> Option<f32> {
        let (sign, magnitude) = self.sign_magnitude();
        let float = Self::exact_unsigned_f32(magnitude)?;
        Some(match sign {
            IntegerSign::Negative => -float,
            IntegerSign::Zero | IntegerSign::Positive => float,
        })
    }

    fn exact_unsigned_f64(value: u128) -> Option<f64> {
        if value == 0 {
            return Some(0.0);
        }
        let float = value as f64;
        let bits = float.to_bits();
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        debug_assert_ne!(exponent_bits, 0);
        let exponent = exponent_bits - 1023;
        let significand = (bits & ((1_u64 << 52) - 1)) | (1_u64 << 52);
        let reconstructed = if exponent >= 52 {
            let shift = (exponent - 52) as u32;
            let significand = significand as u128;
            if significand > (u128::MAX >> shift) {
                return None;
            }
            significand << shift
        } else {
            debug_assert!(exponent >= 0);
            let shift = (52 - exponent) as u32;
            let mask = (1_u64 << shift) - 1;
            debug_assert_eq!(significand & mask, 0);
            (significand >> shift) as u128
        };
        (reconstructed == value).then_some(float)
    }

    fn exact_unsigned_f32(value: u128) -> Option<f32> {
        if value == 0 {
            return Some(0.0);
        }
        let float = value as f32;
        let bits = float.to_bits();
        let exponent_bits = ((bits >> 23) & 0xff) as i32;
        if exponent_bits == 0xff {
            return None;
        }
        debug_assert_ne!(exponent_bits, 0);
        let exponent = exponent_bits - 127;
        let significand = (bits & ((1_u32 << 23) - 1)) | (1_u32 << 23);
        let reconstructed = if exponent >= 23 {
            let shift = (exponent - 23) as u32;
            let significand = significand as u128;
            debug_assert!(shift <= 104);
            significand << shift
        } else {
            debug_assert!(exponent >= 0);
            let shift = (23 - exponent) as u32;
            let mask = (1_u32 << shift) - 1;
            debug_assert_eq!(significand & mask, 0);
            (significand >> shift) as u128
        };
        (reconstructed == value).then_some(float)
    }

    pub fn as_i128(self) -> Option<i128> {
        match self {
            Self::Signed(value) => Some(value),
            Self::Unsigned(value) => i128::try_from(value).ok(),
        }
    }

    pub fn fits_bounds(self, bounds: IntegerBounds) -> bool {
        match bounds {
            IntegerBounds::Signed { min, max } => match self {
                Self::Signed(value) => value >= min && value <= max,
                Self::Unsigned(value) => value <= max as u128,
            },
            IntegerBounds::Unsigned { max } => match self {
                Self::Signed(value) => value >= 0 && (value as u128) <= max,
                Self::Unsigned(value) => value <= max,
            },
        }
    }

    pub fn checked_neg(self) -> Option<Self> {
        let (sign, magnitude) = self.sign_magnitude();
        match sign {
            IntegerSign::Zero => Some(Self::zero()),
            IntegerSign::Positive => {
                Self::from_sign_and_magnitude(IntegerSign::Negative, magnitude)
            }
            IntegerSign::Negative => {
                Self::from_sign_and_magnitude(IntegerSign::Positive, magnitude)
            }
        }
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        let (left_sign, left_mag) = self.sign_magnitude();
        let (right_sign, right_mag) = rhs.sign_magnitude();
        Self::combine_signed_magnitudes(left_sign, left_mag, right_sign, right_mag)
    }

    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        let (right_sign, right_mag) = rhs.sign_magnitude();
        let negated_right_sign = match right_sign {
            IntegerSign::Negative => IntegerSign::Positive,
            IntegerSign::Zero => IntegerSign::Zero,
            IntegerSign::Positive => IntegerSign::Negative,
        };
        let (left_sign, left_mag) = self.sign_magnitude();
        Self::combine_signed_magnitudes(left_sign, left_mag, negated_right_sign, right_mag)
    }

    pub fn checked_mul(self, rhs: Self) -> Option<Self> {
        let (left_sign, left_mag) = self.sign_magnitude();
        let (right_sign, right_mag) = rhs.sign_magnitude();
        let magnitude = left_mag.checked_mul(right_mag)?;
        let sign = match (left_sign, right_sign) {
            (IntegerSign::Zero, _) | (_, IntegerSign::Zero) => IntegerSign::Zero,
            (IntegerSign::Positive, IntegerSign::Positive)
            | (IntegerSign::Negative, IntegerSign::Negative) => IntegerSign::Positive,
            (IntegerSign::Positive, IntegerSign::Negative)
            | (IntegerSign::Negative, IntegerSign::Positive) => IntegerSign::Negative,
        };
        Self::from_sign_and_magnitude(sign, magnitude)
    }

    pub fn checked_div(self, rhs: Self) -> Option<Self> {
        if rhs.is_zero() {
            return None;
        }
        let (left_sign, left_mag) = self.sign_magnitude();
        let (right_sign, right_mag) = rhs.sign_magnitude();
        let magnitude = left_mag / right_mag;
        let sign = if magnitude == 0 {
            IntegerSign::Zero
        } else if left_sign == right_sign {
            IntegerSign::Positive
        } else {
            IntegerSign::Negative
        };
        Self::from_sign_and_magnitude(sign, magnitude)
    }

    pub fn checked_rem(self, rhs: Self) -> Option<Self> {
        if rhs.is_zero() {
            return None;
        }
        let (left_sign, left_mag) = self.sign_magnitude();
        let (_, right_mag) = rhs.sign_magnitude();
        let magnitude = left_mag % right_mag;
        Self::from_sign_and_magnitude(left_sign, magnitude)
    }

    fn combine_signed_magnitudes(
        left_sign: IntegerSign,
        left_mag: u128,
        right_sign: IntegerSign,
        right_mag: u128,
    ) -> Option<Self> {
        match (left_sign, right_sign) {
            (IntegerSign::Zero, _) => Self::from_sign_and_magnitude(right_sign, right_mag),
            (_, IntegerSign::Zero) => Self::from_sign_and_magnitude(left_sign, left_mag),
            (IntegerSign::Positive, IntegerSign::Positive)
            | (IntegerSign::Negative, IntegerSign::Negative) => {
                let magnitude = left_mag.checked_add(right_mag)?;
                Self::from_sign_and_magnitude(left_sign, magnitude)
            }
            _ => match left_mag.cmp(&right_mag) {
                Ordering::Greater => Self::from_sign_and_magnitude(left_sign, left_mag - right_mag),
                Ordering::Less => Self::from_sign_and_magnitude(right_sign, right_mag - left_mag),
                Ordering::Equal => Some(Self::zero()),
            },
        }
    }

    fn sign_magnitude(self) -> (IntegerSign, u128) {
        match self {
            Self::Signed(value) if value < 0 => (IntegerSign::Negative, value.unsigned_abs()),
            Self::Signed(0) | Self::Unsigned(0) => (IntegerSign::Zero, 0),
            Self::Signed(value) => (IntegerSign::Positive, value as u128),
            Self::Unsigned(value) => (IntegerSign::Positive, value),
        }
    }

    fn from_sign_and_magnitude(sign: IntegerSign, magnitude: u128) -> Option<Self> {
        match sign {
            IntegerSign::Zero => Some(Self::zero()),
            IntegerSign::Positive => Some(Self::Unsigned(magnitude)),
            IntegerSign::Negative => {
                if magnitude == (1u128 << 127) {
                    Some(Self::Signed(i128::MIN))
                } else if magnitude < (1u128 << 127) {
                    Some(Self::Signed(-(magnitude as i128)))
                } else {
                    None
                }
            }
        }
    }
}

pub fn integer_type_bounds(ty: &Type) -> Option<IntegerBounds> {
    match ty {
        Type::Unit => None,
        Type::Module(_) => None,
        Type::TypeParam(_) => None,
        Type::Named(_, args) if !args.is_empty() => None,
        Type::Named(name, _) => match name.as_str() {
            "int8" => Some(IntegerBounds::Signed {
                min: i8::MIN as i128,
                max: i8::MAX as i128,
            }),
            "int16" => Some(IntegerBounds::Signed {
                min: i16::MIN as i128,
                max: i16::MAX as i128,
            }),
            "int32" => Some(IntegerBounds::Signed {
                min: i32::MIN as i128,
                max: i32::MAX as i128,
            }),
            "int64" => Some(IntegerBounds::Signed {
                min: i64::MIN as i128,
                max: i64::MAX as i128,
            }),
            "int128" => Some(IntegerBounds::Signed {
                min: i128::MIN,
                max: i128::MAX,
            }),
            "intsize" => Some(IntegerBounds::Signed {
                min: isize::MIN as i128,
                max: isize::MAX as i128,
            }),
            "uint8" => Some(IntegerBounds::Unsigned {
                max: u8::MAX as u128,
            }),
            "uint16" => Some(IntegerBounds::Unsigned {
                max: u16::MAX as u128,
            }),
            "uint32" => Some(IntegerBounds::Unsigned {
                max: u32::MAX as u128,
            }),
            "uint64" => Some(IntegerBounds::Unsigned {
                max: u64::MAX as u128,
            }),
            "uint128" => Some(IntegerBounds::Unsigned { max: u128::MAX }),
            "uintsize" => Some(IntegerBounds::Unsigned {
                max: usize::MAX as u128,
            }),
            _ => None,
        },
    }
}

pub fn minimal_signed_type_for_negative_literal(value: u128) -> Option<Type> {
    let negative = IntegerValue::from_literal(value).checked_neg()?;
    let int32 = Type::named("int32");
    let int32_bounds = integer_type_bounds(&int32).expect("int32 bounds should be defined");
    if negative.fits_bounds(int32_bounds) {
        return Some(int32);
    }

    let int64 = Type::named("int64");
    let int64_bounds = integer_type_bounds(&int64).expect("int64 bounds should be defined");
    if negative.fits_bounds(int64_bounds) {
        return Some(int64);
    }

    Some(Type::named("int128"))
}

#[cfg(test)]
#[path = "integer_tests.rs"]
mod tests;
