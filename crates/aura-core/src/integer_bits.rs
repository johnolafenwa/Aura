//! Operations on the fixed-width two's-complement representation of an integer.
use crate::integer::{IntegerBounds, IntegerKind, IntegerRepresentation, IntegerValue};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum BitMethod {
    RotateLeft, RotateRight, CountOnes, LeadingZeros, TrailingZeros, SwapBytes,
}

impl BitMethod {
    pub const ALL: &'static [Self] = &[Self::RotateLeft, Self::RotateRight, Self::CountOnes,
        Self::LeadingZeros, Self::TrailingZeros, Self::SwapBytes];

    pub fn of(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|method| method.name() == name)
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::RotateLeft => "rotate_left", Self::RotateRight => "rotate_right",
            Self::CountOnes => "count_ones", Self::LeadingZeros => "leading_zeros",
            Self::TrailingZeros => "trailing_zeros", Self::SwapBytes => "swap_bytes",
        }
    }
    pub const fn signature(self) -> &'static str {
        match self {
            Self::RotateLeft => "rotate_left(count: int64) -> Self",
            Self::RotateRight => "rotate_right(count: int64) -> Self",
            Self::CountOnes => "count_ones() -> int64", Self::LeadingZeros => "leading_zeros() -> int64",
            Self::TrailingZeros => "trailing_zeros() -> int64", Self::SwapBytes => "swap_bytes() -> Self",
        }
    }
    pub const fn counts(self) -> bool {
        matches!(self, Self::CountOnes | Self::LeadingZeros | Self::TrailingZeros)
    }

    pub fn apply(self, value: IntegerValue, kind: IntegerKind, count: i64) -> Result<IntegerValue, &'static str> {
        let width = kind.bit_width();
        let padding = 128 - width;
        let mask = u128::MAX >> padding;
        let bits = match value.representation() {
            IntegerRepresentation::Signed(v) => v as u128,
            IntegerRepresentation::Unsigned(v) => v,
        } & mask;
        let count = u64::try_from(count).map_err(|_| "rotation count must be nonnegative")? as u32 % width;
        let result = match self {
            Self::RotateLeft => ((bits << count) | (bits >> ((width - count) % width))) & mask,
            Self::RotateRight => ((bits >> count) | (bits << ((width - count) % width))) & mask,
            Self::SwapBytes => bits.swap_bytes() >> padding,
            Self::CountOnes => return Ok(IntegerValue::from_i64(i64::from(bits.count_ones()))),
            Self::LeadingZeros => return Ok(IntegerValue::from_i64(i64::from(bits.leading_zeros() - padding))),
            Self::TrailingZeros => return Ok(IntegerValue::from_i64(i64::from(bits.trailing_zeros().min(width)))),
        };
        Ok(match kind.bounds() {
            IntegerBounds::Signed { .. } => IntegerValue::from_typed_signed(((result << padding) as i128) >> padding, kind),
            IntegerBounds::Unsigned { .. } => IntegerValue::from_typed_unsigned(result, kind),
        }.expect("masked result fits its declared width"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_rotations_and_zero_counts_use_the_receiver_width() {
        use BitMethod::*;
        let signed = IntegerValue::from_i64(-2);
        let apply = |method: BitMethod, value, count| method.apply(value, IntegerKind::Int8, count).unwrap().as_i128().unwrap();
        assert_eq!(apply(RotateLeft, signed, 9), -3);
        assert_eq!(apply(RotateRight, signed, 8), -2);
        assert_eq!(apply(SwapBytes, signed, 0), -2);
        assert_eq!(apply(CountOnes, signed, 0), 7);
        for op in [LeadingZeros, TrailingZeros] {
            assert_eq!(apply(op, IntegerValue::from_i64(0), 0), 8);
        }
        assert!(RotateLeft.apply(signed, IntegerKind::Int8, -1).is_err());
        for op in BitMethod::ALL {
            assert_eq!(BitMethod::of(op.name()), Some(*op));
            assert!(op.signature().starts_with(op.name()));
        }
    }
}
