use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::sema::Type;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum IntegerRepresentation {
    Signed(i128),
    Unsigned(u128),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum IntegerKind {
    Int8,
    Int16,
    Int32,
    Int64,
    Int128,
    IntSize,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Uint128,
    UintSize,
}

impl IntegerKind {
    pub const fn runtime_type_name(self) -> &'static str {
        match self {
            Self::Int8 => "int8",
            Self::Int16 => "int16",
            Self::Int32 => "int32",
            Self::Int64 => "int64",
            Self::Int128 => "int128",
            Self::IntSize => "intsize",
            Self::Uint8 => "uint8",
            Self::Uint16 => "uint16",
            Self::Uint32 => "uint32",
            Self::Uint64 => "uint64",
            Self::Uint128 => "uint128",
            Self::UintSize => "uintsize",
        }
    }

    pub fn from_runtime_type_name(name: &str) -> Option<Self> {
        match name {
            "int8" => Some(Self::Int8),
            "int16" => Some(Self::Int16),
            "int32" => Some(Self::Int32),
            "int64" => Some(Self::Int64),
            "int128" => Some(Self::Int128),
            "intsize" => Some(Self::IntSize),
            "uint8" => Some(Self::Uint8),
            "uint16" => Some(Self::Uint16),
            "uint32" => Some(Self::Uint32),
            "uint64" => Some(Self::Uint64),
            "uint128" => Some(Self::Uint128),
            "uintsize" => Some(Self::UintSize),
            _ => None,
        }
    }

    pub const fn is_signed(self) -> bool {
        matches!(
            self,
            Self::Int8 | Self::Int16 | Self::Int32 | Self::Int64 | Self::Int128 | Self::IntSize
        )
    }

    pub const fn bit_width(self) -> u32 {
        match self {
            Self::Int8 | Self::Uint8 => 8,
            Self::Int16 | Self::Uint16 => 16,
            Self::Int32 | Self::Uint32 => 32,
            Self::Int64 | Self::Uint64 => 64,
            Self::Int128 | Self::Uint128 => 128,
            Self::IntSize | Self::UintSize => usize::BITS,
        }
    }

    pub const fn bounds(self) -> IntegerBounds {
        match self {
            Self::Int8 => IntegerBounds::Signed {
                min: i8::MIN as i128,
                max: i8::MAX as i128,
            },
            Self::Int16 => IntegerBounds::Signed {
                min: i16::MIN as i128,
                max: i16::MAX as i128,
            },
            Self::Int32 => IntegerBounds::Signed {
                min: i32::MIN as i128,
                max: i32::MAX as i128,
            },
            Self::Int64 => IntegerBounds::Signed {
                min: i64::MIN as i128,
                max: i64::MAX as i128,
            },
            Self::Int128 => IntegerBounds::Signed {
                min: i128::MIN,
                max: i128::MAX,
            },
            Self::IntSize => IntegerBounds::Signed {
                min: isize::MIN as i128,
                max: isize::MAX as i128,
            },
            Self::Uint8 => IntegerBounds::Unsigned {
                max: u8::MAX as u128,
            },
            Self::Uint16 => IntegerBounds::Unsigned {
                max: u16::MAX as u128,
            },
            Self::Uint32 => IntegerBounds::Unsigned {
                max: u32::MAX as u128,
            },
            Self::Uint64 => IntegerBounds::Unsigned {
                max: u64::MAX as u128,
            },
            Self::Uint128 => IntegerBounds::Unsigned { max: u128::MAX },
            Self::UintSize => IntegerBounds::Unsigned {
                max: usize::MAX as u128,
            },
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct IntegerValue {
    representation: IntegerRepresentation,
    runtime_kind: Option<IntegerKind>,
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

#[derive(Copy, Clone)]
enum WidthArithmetic {
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    SaturatingAdd,
    SaturatingSub,
    SaturatingMul,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IntegerShiftError {
    MismatchedKinds,
    InvalidCount { count: IntegerValue, width: u32 },
    Overflow,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IntegerPowerError {
    MismatchedKinds,
    NegativeExponent,
    Overflow,
}

#[derive(Copy, Clone)]
enum BitwiseOperation {
    And,
    Or,
    Xor,
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
        match self.representation {
            IntegerRepresentation::Signed(value) => write!(f, "{}", value),
            IntegerRepresentation::Unsigned(value) => write!(f, "{}", value),
        }
    }
}

impl IntegerValue {
    pub const fn zero() -> Self {
        Self::from_representation(IntegerRepresentation::Unsigned(0))
    }

    pub const fn from_literal(value: u128) -> Self {
        Self::from_representation(IntegerRepresentation::Unsigned(value))
    }

    pub fn from_signed(value: i128) -> Self {
        if value >= 0 {
            Self::from_literal(value as u128)
        } else {
            Self::from_representation(IntegerRepresentation::Signed(value))
        }
    }

    pub const fn from_representation(representation: IntegerRepresentation) -> Self {
        Self {
            representation,
            runtime_kind: None,
        }
    }

    pub fn from_typed_signed(value: i128, kind: IntegerKind) -> Option<Self> {
        if !kind.is_signed() {
            return None;
        }
        Self::from_signed(value).with_runtime_kind(kind)
    }

    pub fn from_typed_unsigned(value: u128, kind: IntegerKind) -> Option<Self> {
        if kind.is_signed() {
            return None;
        }
        Self::from_literal(value).with_runtime_kind(kind)
    }

    pub fn from_i32(value: i32) -> Self {
        Self::from_typed_signed(value as i128, IntegerKind::Int32)
            .expect("every i32 value fits the int32 runtime kind")
    }

    pub fn from_i64(value: i64) -> Self {
        Self::from_typed_signed(value as i128, IntegerKind::Int64)
            .expect("every i64 value fits the int64 runtime kind")
    }

    pub fn from_u64(value: u64) -> Self {
        Self::from_typed_unsigned(value as u128, IntegerKind::Uint64)
            .expect("every u64 value fits the uint64 runtime kind")
    }

    pub const fn representation(self) -> IntegerRepresentation {
        self.representation
    }

    pub const fn runtime_kind(self) -> Option<IntegerKind> {
        self.runtime_kind
    }

    pub const fn runtime_type_name(self) -> Option<&'static str> {
        match self.runtime_kind {
            Some(kind) => Some(kind.runtime_type_name()),
            None => None,
        }
    }

    pub fn with_runtime_kind(mut self, kind: IntegerKind) -> Option<Self> {
        if !self.fits_bounds(kind.bounds()) {
            return None;
        }
        self.runtime_kind = Some(kind);
        Some(self)
    }

    pub const fn without_runtime_kind(mut self) -> Self {
        self.runtime_kind = None;
        self
    }

    pub fn is_zero(&self) -> bool {
        matches!(
            self.representation,
            IntegerRepresentation::Signed(0) | IntegerRepresentation::Unsigned(0)
        )
    }

    pub fn to_f64(self) -> f64 {
        match self.representation {
            IntegerRepresentation::Signed(value) => value as f64,
            IntegerRepresentation::Unsigned(value) => value as f64,
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
        match self.representation {
            IntegerRepresentation::Signed(value) => Some(value),
            IntegerRepresentation::Unsigned(value) => i128::try_from(value).ok(),
        }
    }

    pub fn fits_bounds(self, bounds: IntegerBounds) -> bool {
        match self.representation {
            IntegerRepresentation::Signed(value) => match bounds {
                IntegerBounds::Signed { min, max } => value >= min && value <= max,
                IntegerBounds::Unsigned { max } => value >= 0 && (value as u128) <= max,
            },
            IntegerRepresentation::Unsigned(value) => {
                let max = match bounds {
                    IntegerBounds::Signed { max, .. } => max as u128,
                    IntegerBounds::Unsigned { max } => max,
                };
                value <= max
            }
        }
    }

    pub fn checked_neg(self) -> Option<Self> {
        let (sign, magnitude) = self.sign_magnitude();
        let result = match sign {
            IntegerSign::Zero => Some(Self::zero()),
            IntegerSign::Positive => {
                Self::from_sign_and_magnitude(IntegerSign::Negative, magnitude)
            }
            IntegerSign::Negative => {
                Self::from_sign_and_magnitude(IntegerSign::Positive, magnitude)
            }
        }?;
        result.with_optional_runtime_kind(self.runtime_kind)
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        let runtime_kind = self.common_runtime_kind(rhs);
        let (left_sign, left_mag) = self.sign_magnitude();
        let (right_sign, right_mag) = rhs.sign_magnitude();
        Self::combine_signed_magnitudes(left_sign, left_mag, right_sign, right_mag)?
            .with_optional_runtime_kind(runtime_kind)
    }

    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        let runtime_kind = self.common_runtime_kind(rhs);
        let (right_sign, right_mag) = rhs.sign_magnitude();
        let negated_right_sign = match right_sign {
            IntegerSign::Negative => IntegerSign::Positive,
            IntegerSign::Zero => IntegerSign::Zero,
            IntegerSign::Positive => IntegerSign::Negative,
        };
        let (left_sign, left_mag) = self.sign_magnitude();
        Self::combine_signed_magnitudes(left_sign, left_mag, negated_right_sign, right_mag)?
            .with_optional_runtime_kind(runtime_kind)
    }

    pub fn checked_mul(self, rhs: Self) -> Option<Self> {
        let runtime_kind = self.common_runtime_kind(rhs);
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
        Self::from_sign_and_magnitude(sign, magnitude)?.with_optional_runtime_kind(runtime_kind)
    }

    pub fn checked_pow(self, exponent: Self) -> Result<Self, IntegerPowerError> {
        let kind = self
            .common_runtime_kind(exponent)
            .ok_or(IntegerPowerError::MismatchedKinds)?;
        let mut remaining = exponent
            .as_nonnegative_u128()
            .ok_or(IntegerPowerError::NegativeExponent)?;
        let mut result = if kind.is_signed() {
            Self::from_typed_signed(1, kind)
        } else {
            Self::from_typed_unsigned(1, kind)
        }
        .ok_or(IntegerPowerError::Overflow)?;
        let mut factor = self;

        while remaining != 0 {
            if remaining & 1 == 1 {
                result = result
                    .checked_mul(factor)
                    .and_then(|value| value.with_runtime_kind(kind))
                    .ok_or(IntegerPowerError::Overflow)?;
            }
            remaining >>= 1;
            if remaining != 0 {
                factor = factor
                    .checked_mul(factor)
                    .and_then(|value| value.with_runtime_kind(kind))
                    .ok_or(IntegerPowerError::Overflow)?;
            }
        }
        Ok(result)
    }

    pub fn checked_div(self, rhs: Self) -> Option<Self> {
        if rhs.is_zero() {
            return None;
        }
        let runtime_kind = self.common_runtime_kind(rhs);
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
        Self::from_sign_and_magnitude(sign, magnitude)?.with_optional_runtime_kind(runtime_kind)
    }

    pub fn checked_rem(self, rhs: Self) -> Option<Self> {
        if rhs.is_zero() {
            return None;
        }
        let runtime_kind = self.common_runtime_kind(rhs);
        let (left_sign, left_mag) = self.sign_magnitude();
        let (_, right_mag) = rhs.sign_magnitude();
        let magnitude = left_mag % right_mag;
        Self::from_sign_and_magnitude(left_sign, magnitude)?
            .with_optional_runtime_kind(runtime_kind)
    }

    pub fn checked_floor_div(self, rhs: Self) -> Option<Self> {
        if rhs.is_zero() {
            return None;
        }
        let runtime_kind = self.common_runtime_kind(rhs);
        let (left_sign, left_mag) = self.sign_magnitude();
        let (right_sign, right_mag) = rhs.sign_magnitude();
        let remainder = left_mag % right_mag;
        let signs_differ = left_sign != IntegerSign::Zero && left_sign != right_sign;
        let magnitude = if remainder != 0 && signs_differ {
            (left_mag / right_mag).checked_add(1)?
        } else {
            left_mag / right_mag
        };
        let sign = if magnitude == 0 {
            IntegerSign::Zero
        } else if signs_differ {
            IntegerSign::Negative
        } else {
            IntegerSign::Positive
        };
        Self::from_sign_and_magnitude(sign, magnitude)?.with_optional_runtime_kind(runtime_kind)
    }

    pub fn checked_floor_rem(self, rhs: Self) -> Option<Self> {
        if rhs.is_zero() {
            return None;
        }
        let runtime_kind = self.common_runtime_kind(rhs);
        let (left_sign, left_mag) = self.sign_magnitude();
        let (right_sign, right_mag) = rhs.sign_magnitude();
        let truncating_remainder = left_mag % right_mag;
        if truncating_remainder == 0 {
            return Self::zero().with_optional_runtime_kind(runtime_kind);
        }
        let signs_differ = left_sign != IntegerSign::Zero && left_sign != right_sign;
        let magnitude = if signs_differ {
            right_mag - truncating_remainder
        } else {
            truncating_remainder
        };
        Self::from_sign_and_magnitude(right_sign, magnitude)?
            .with_optional_runtime_kind(runtime_kind)
    }

    pub fn wrapping_add(self, rhs: Self) -> Option<Self> {
        self.width_arithmetic(rhs, WidthArithmetic::WrappingAdd)
    }

    pub fn wrapping_sub(self, rhs: Self) -> Option<Self> {
        self.width_arithmetic(rhs, WidthArithmetic::WrappingSub)
    }

    pub fn wrapping_mul(self, rhs: Self) -> Option<Self> {
        self.width_arithmetic(rhs, WidthArithmetic::WrappingMul)
    }

    pub fn saturating_add(self, rhs: Self) -> Option<Self> {
        self.width_arithmetic(rhs, WidthArithmetic::SaturatingAdd)
    }

    pub fn saturating_sub(self, rhs: Self) -> Option<Self> {
        self.width_arithmetic(rhs, WidthArithmetic::SaturatingSub)
    }

    pub fn saturating_mul(self, rhs: Self) -> Option<Self> {
        self.width_arithmetic(rhs, WidthArithmetic::SaturatingMul)
    }

    pub fn checked_bitand(self, rhs: Self) -> Option<Self> {
        self.bitwise(rhs, BitwiseOperation::And)
    }

    pub fn checked_bitor(self, rhs: Self) -> Option<Self> {
        self.bitwise(rhs, BitwiseOperation::Or)
    }

    pub fn checked_bitxor(self, rhs: Self) -> Option<Self> {
        self.bitwise(rhs, BitwiseOperation::Xor)
    }

    pub fn bitnot(self) -> Option<Self> {
        let kind = self.runtime_kind?;
        macro_rules! signed {
            ($native:ty) => {{
                let value = <$native>::try_from(self.as_i128()?).ok()?;
                Self::from_typed_signed((!value) as i128, kind)
            }};
        }
        macro_rules! unsigned {
            ($native:ty) => {{
                let value = <$native>::try_from(self.as_nonnegative_u128()?).ok()?;
                Self::from_typed_unsigned((!value) as u128, kind)
            }};
        }
        match kind {
            IntegerKind::Int8 => signed!(i8),
            IntegerKind::Int16 => signed!(i16),
            IntegerKind::Int32 => signed!(i32),
            IntegerKind::Int64 => signed!(i64),
            IntegerKind::Int128 => signed!(i128),
            IntegerKind::IntSize => signed!(isize),
            IntegerKind::Uint8 => unsigned!(u8),
            IntegerKind::Uint16 => unsigned!(u16),
            IntegerKind::Uint32 => unsigned!(u32),
            IntegerKind::Uint64 => unsigned!(u64),
            IntegerKind::Uint128 => unsigned!(u128),
            IntegerKind::UintSize => unsigned!(usize),
        }
    }

    pub fn checked_shl(self, count: Self) -> Result<Self, IntegerShiftError> {
        let (kind, count) = self.valid_shift_count(count)?;
        let (sign, magnitude) = self.sign_magnitude();
        let allowed_magnitude = match kind.bounds() {
            IntegerBounds::Signed { min, max } => match sign {
                IntegerSign::Negative => min.unsigned_abs(),
                IntegerSign::Zero | IntegerSign::Positive => max as u128,
            },
            IntegerBounds::Unsigned { max } => max,
        };
        if magnitude > (allowed_magnitude >> count) {
            return Err(IntegerShiftError::Overflow);
        }
        let shifted = magnitude << count;
        Self::from_sign_and_magnitude(sign, shifted)
            .and_then(|value| value.with_runtime_kind(kind))
            .ok_or(IntegerShiftError::Overflow)
    }

    pub fn checked_shr(self, count: Self) -> Result<Self, IntegerShiftError> {
        let (kind, count) = self.valid_shift_count(count)?;
        macro_rules! signed {
            ($native:ty) => {{
                let value = <$native>::try_from(self.as_i128().expect("typed signed value"))
                    .expect("value fits its runtime kind");
                Self::from_typed_signed((value >> count) as i128, kind)
                    .expect("right shift remains in range")
            }};
        }
        macro_rules! unsigned {
            ($native:ty) => {{
                let value =
                    <$native>::try_from(self.as_nonnegative_u128().expect("typed unsigned value"))
                        .expect("value fits its runtime kind");
                Self::from_typed_unsigned((value >> count) as u128, kind)
                    .expect("right shift remains in range")
            }};
        }
        Ok(match kind {
            IntegerKind::Int8 => signed!(i8),
            IntegerKind::Int16 => signed!(i16),
            IntegerKind::Int32 => signed!(i32),
            IntegerKind::Int64 => signed!(i64),
            IntegerKind::Int128 => signed!(i128),
            IntegerKind::IntSize => signed!(isize),
            IntegerKind::Uint8 => unsigned!(u8),
            IntegerKind::Uint16 => unsigned!(u16),
            IntegerKind::Uint32 => unsigned!(u32),
            IntegerKind::Uint64 => unsigned!(u64),
            IntegerKind::Uint128 => unsigned!(u128),
            IntegerKind::UintSize => unsigned!(usize),
        })
    }

    pub fn wrapping_shl(self, count: Self) -> Result<Self, IntegerShiftError> {
        let (kind, count) = self.valid_shift_count(count)?;
        macro_rules! signed {
            ($native:ty) => {{
                let value = <$native>::try_from(self.as_i128().expect("typed signed value"))
                    .expect("value fits its runtime kind");
                Self::from_typed_signed(value.wrapping_shl(count) as i128, kind)
                    .expect("wrapping shift remains in range")
            }};
        }
        macro_rules! unsigned {
            ($native:ty) => {{
                let value =
                    <$native>::try_from(self.as_nonnegative_u128().expect("typed unsigned value"))
                        .expect("value fits its runtime kind");
                Self::from_typed_unsigned(value.wrapping_shl(count) as u128, kind)
                    .expect("wrapping shift remains in range")
            }};
        }
        Ok(match kind {
            IntegerKind::Int8 => signed!(i8),
            IntegerKind::Int16 => signed!(i16),
            IntegerKind::Int32 => signed!(i32),
            IntegerKind::Int64 => signed!(i64),
            IntegerKind::Int128 => signed!(i128),
            IntegerKind::IntSize => signed!(isize),
            IntegerKind::Uint8 => unsigned!(u8),
            IntegerKind::Uint16 => unsigned!(u16),
            IntegerKind::Uint32 => unsigned!(u32),
            IntegerKind::Uint64 => unsigned!(u64),
            IntegerKind::Uint128 => unsigned!(u128),
            IntegerKind::UintSize => unsigned!(usize),
        })
    }

    pub fn wrapping_shr(self, count: Self) -> Result<Self, IntegerShiftError> {
        self.checked_shr(count)
    }

    pub fn saturating_shl(self, count: Self) -> Result<Self, IntegerShiftError> {
        let kind = self
            .common_runtime_kind(count)
            .ok_or(IntegerShiftError::MismatchedKinds)?;
        match self.checked_shl(count) {
            Ok(value) => Ok(value),
            Err(IntegerShiftError::Overflow) => match kind.bounds() {
                IntegerBounds::Signed { min, max } => {
                    if self.sign_magnitude().0 == IntegerSign::Negative {
                        Self::from_typed_signed(min, kind)
                    } else {
                        Self::from_typed_signed(max, kind)
                    }
                    .ok_or(IntegerShiftError::Overflow)
                }
                IntegerBounds::Unsigned { max } => {
                    Self::from_typed_unsigned(max, kind).ok_or(IntegerShiftError::Overflow)
                }
            },
            Err(error) => Err(error),
        }
    }

    pub fn saturating_shr(self, count: Self) -> Result<Self, IntegerShiftError> {
        self.checked_shr(count)
    }

    fn bitwise(self, rhs: Self, operation: BitwiseOperation) -> Option<Self> {
        let kind = self.common_runtime_kind(rhs)?;
        macro_rules! apply {
            ($left:expr, $right:expr) => {{
                match operation {
                    BitwiseOperation::And => $left & $right,
                    BitwiseOperation::Or => $left | $right,
                    BitwiseOperation::Xor => $left ^ $right,
                }
            }};
        }
        macro_rules! signed {
            ($native:ty) => {{
                let left = <$native>::try_from(self.as_i128()?).ok()?;
                let right = <$native>::try_from(rhs.as_i128()?).ok()?;
                Self::from_typed_signed(apply!(left, right) as i128, kind)
            }};
        }
        macro_rules! unsigned {
            ($native:ty) => {{
                let left = <$native>::try_from(self.as_nonnegative_u128()?).ok()?;
                let right = <$native>::try_from(rhs.as_nonnegative_u128()?).ok()?;
                Self::from_typed_unsigned(apply!(left, right) as u128, kind)
            }};
        }
        match kind {
            IntegerKind::Int8 => signed!(i8),
            IntegerKind::Int16 => signed!(i16),
            IntegerKind::Int32 => signed!(i32),
            IntegerKind::Int64 => signed!(i64),
            IntegerKind::Int128 => signed!(i128),
            IntegerKind::IntSize => signed!(isize),
            IntegerKind::Uint8 => unsigned!(u8),
            IntegerKind::Uint16 => unsigned!(u16),
            IntegerKind::Uint32 => unsigned!(u32),
            IntegerKind::Uint64 => unsigned!(u64),
            IntegerKind::Uint128 => unsigned!(u128),
            IntegerKind::UintSize => unsigned!(usize),
        }
    }

    fn valid_shift_count(self, count: Self) -> Result<(IntegerKind, u32), IntegerShiftError> {
        let kind = self
            .common_runtime_kind(count)
            .ok_or(IntegerShiftError::MismatchedKinds)?;
        let width = kind.bit_width();
        let valid = count
            .as_nonnegative_u128()
            .filter(|value| *value < width as u128)
            .and_then(|value| u32::try_from(value).ok());
        valid
            .map(|count| (kind, count))
            .ok_or(IntegerShiftError::InvalidCount { count, width })
    }

    fn width_arithmetic(self, rhs: Self, operation: WidthArithmetic) -> Option<Self> {
        let kind = self.common_runtime_kind(rhs)?;
        macro_rules! signed {
            ($native:ty) => {{
                let left = <$native>::try_from(self.as_i128()?).ok()?;
                let right = <$native>::try_from(rhs.as_i128()?).ok()?;
                let result = match operation {
                    WidthArithmetic::WrappingAdd => left.wrapping_add(right),
                    WidthArithmetic::WrappingSub => left.wrapping_sub(right),
                    WidthArithmetic::WrappingMul => left.wrapping_mul(right),
                    WidthArithmetic::SaturatingAdd => left.saturating_add(right),
                    WidthArithmetic::SaturatingSub => left.saturating_sub(right),
                    WidthArithmetic::SaturatingMul => left.saturating_mul(right),
                };
                Self::from_typed_signed(result as i128, kind)
            }};
        }
        macro_rules! unsigned {
            ($native:ty) => {{
                let left = <$native>::try_from(self.as_nonnegative_u128()?).ok()?;
                let right = <$native>::try_from(rhs.as_nonnegative_u128()?).ok()?;
                let result = match operation {
                    WidthArithmetic::WrappingAdd => left.wrapping_add(right),
                    WidthArithmetic::WrappingSub => left.wrapping_sub(right),
                    WidthArithmetic::WrappingMul => left.wrapping_mul(right),
                    WidthArithmetic::SaturatingAdd => left.saturating_add(right),
                    WidthArithmetic::SaturatingSub => left.saturating_sub(right),
                    WidthArithmetic::SaturatingMul => left.saturating_mul(right),
                };
                Self::from_typed_unsigned(result as u128, kind)
            }};
        }
        match kind {
            IntegerKind::Int8 => signed!(i8),
            IntegerKind::Int16 => signed!(i16),
            IntegerKind::Int32 => signed!(i32),
            IntegerKind::Int64 => signed!(i64),
            IntegerKind::Int128 => signed!(i128),
            IntegerKind::IntSize => signed!(isize),
            IntegerKind::Uint8 => unsigned!(u8),
            IntegerKind::Uint16 => unsigned!(u16),
            IntegerKind::Uint32 => unsigned!(u32),
            IntegerKind::Uint64 => unsigned!(u64),
            IntegerKind::Uint128 => unsigned!(u128),
            IntegerKind::UintSize => unsigned!(usize),
        }
    }

    fn as_nonnegative_u128(self) -> Option<u128> {
        match self.representation {
            IntegerRepresentation::Signed(value) => u128::try_from(value).ok(),
            IntegerRepresentation::Unsigned(value) => Some(value),
        }
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
        match self.representation {
            IntegerRepresentation::Signed(value) if value < 0 => {
                (IntegerSign::Negative, value.unsigned_abs())
            }
            IntegerRepresentation::Signed(0) | IntegerRepresentation::Unsigned(0) => {
                (IntegerSign::Zero, 0)
            }
            IntegerRepresentation::Signed(value) => (IntegerSign::Positive, value as u128),
            IntegerRepresentation::Unsigned(value) => (IntegerSign::Positive, value),
        }
    }

    fn from_sign_and_magnitude(sign: IntegerSign, magnitude: u128) -> Option<Self> {
        match sign {
            IntegerSign::Zero => Some(Self::zero()),
            IntegerSign::Positive => Some(Self::from_literal(magnitude)),
            IntegerSign::Negative => {
                if magnitude == (1u128 << 127) {
                    Some(Self::from_representation(IntegerRepresentation::Signed(
                        i128::MIN,
                    )))
                } else if magnitude < (1u128 << 127) {
                    Some(Self::from_representation(IntegerRepresentation::Signed(
                        -(magnitude as i128),
                    )))
                } else {
                    None
                }
            }
        }
    }

    fn common_runtime_kind(self, rhs: Self) -> Option<IntegerKind> {
        (self.runtime_kind == rhs.runtime_kind)
            .then_some(self.runtime_kind)
            .flatten()
    }

    fn with_optional_runtime_kind(self, runtime_kind: Option<IntegerKind>) -> Option<Self> {
        match runtime_kind {
            Some(kind) => self
                .with_runtime_kind(kind)
                .or_else(|| Some(self.without_runtime_kind())),
            None => Some(self),
        }
    }
}

pub fn integer_type_bounds(ty: &Type) -> Option<IntegerBounds> {
    match ty {
        Type::Unit => None,
        Type::Module(_) => None,
        Type::TypeParam(_) => None,
        Type::Tuple(_) => None,
        Type::Function { .. } | Type::Closure { .. } => None,
        Type::Named(_, args) if !args.is_empty() => None,
        Type::Named(name, _) => IntegerKind::from_runtime_type_name(name).map(IntegerKind::bounds),
    }
}

pub fn minimal_signed_type_for_negative_literal(value: u128) -> Option<Type> {
    let negative = IntegerValue::from_literal(value).checked_neg()?;
    let int64 = Type::named("int64");
    let int64_bounds = integer_type_bounds(&int64).expect("int64 bounds should be defined");
    if negative.fits_bounds(int64_bounds) {
        return Some(int64);
    }
    None
}

#[cfg(test)]
#[path = "integer_tests.rs"]
mod tests;
