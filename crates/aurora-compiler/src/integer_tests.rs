use super::{
    integer_type_bounds, minimal_signed_type_for_negative_literal, IntegerBounds, IntegerSign,
    IntegerValue,
};
use crate::sema::Type;
use std::cmp::Ordering;

#[test]
fn integer_value_helpers_cover_division_remainder_comparisons_and_bounds() {
    let negative = IntegerValue::from_signed(-9);
    let positive = IntegerValue::from_signed(4);
    let zero = IntegerValue::zero();
    let signed_positive = IntegerValue::from_signed(5);
    let explicit_signed_positive = IntegerValue::Signed(5);

    assert!(zero.is_zero());
    assert!(!positive.is_zero());
    assert_eq!(negative.to_f64(), -9.0);
    assert_eq!(positive.to_f64(), 4.0);
    assert_eq!(negative.to_exact_f64(), Some(-9.0));
    assert_eq!(positive.to_exact_f64(), Some(4.0));
    assert_eq!(zero.to_exact_f64(), Some(0.0));
    assert_eq!(
        IntegerValue::from_literal(1_u128 << 53).to_exact_f64(),
        Some((1_u128 << 53) as f64)
    );
    assert_eq!(
        IntegerValue::from_literal((1_u128 << 53) + 1).to_exact_f64(),
        None
    );
    assert_eq!(IntegerValue::from_literal(u128::MAX).to_exact_f64(), None);
    assert_eq!(negative.to_exact_f32(), Some(-9.0));
    assert_eq!(positive.to_exact_f32(), Some(4.0));
    assert_eq!(zero.to_exact_f32(), Some(0.0));
    assert_eq!(
        IntegerValue::from_literal(1_u128 << 24).to_exact_f32(),
        Some((1_u128 << 24) as f32)
    );
    assert_eq!(
        IntegerValue::from_literal((1_u128 << 24) + 1).to_exact_f32(),
        None
    );
    assert_eq!(IntegerValue::from_literal(u128::MAX).to_exact_f32(), None);
    assert_eq!(
        IntegerValue::from_literal(i128::MAX as u128).as_i128(),
        Some(i128::MAX)
    );
    assert_eq!(
        IntegerValue::from_literal((i128::MAX as u128) + 1).as_i128(),
        None
    );

    assert_eq!(
        negative.checked_div(positive),
        Some(IntegerValue::from_signed(-2))
    );
    assert_eq!(
        negative.checked_rem(positive),
        Some(IntegerValue::from_signed(-1))
    );
    assert_eq!(positive.checked_div(zero), None);
    assert_eq!(positive.checked_rem(zero), None);
    assert_eq!(
        signed_positive.checked_div(IntegerValue::from_signed(-2)),
        Some(IntegerValue::from_signed(-2))
    );
    assert_eq!(
        explicit_signed_positive.checked_rem(IntegerValue::from_signed(2)),
        Some(IntegerValue::from_signed(1))
    );
    assert_eq!(zero.cmp(&positive), Ordering::Less);
    assert_eq!(positive.cmp(&negative), Ordering::Greater);
    assert_eq!(
        negative.cmp(&IntegerValue::from_signed(-12)),
        Ordering::Greater
    );
    assert_eq!(
        signed_positive.cmp(&IntegerValue::zero()),
        Ordering::Greater
    );

    assert_eq!(
        minimal_signed_type_for_negative_literal(7),
        Some(Type::named("int32"))
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i32::MAX as u128) + 2),
        Some(Type::named("int64"))
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i64::MAX as u128) + 2),
        Some(Type::named("int128"))
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i64::MAX as u128) + 10_000),
        Some(Type::named("int128"))
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((1u128 << 127) + 1),
        None
    );

    assert_eq!(integer_type_bounds(&Type::Unit), None);
    assert_eq!(integer_type_bounds(&Type::Module("pkg".to_string())), None);
    assert_eq!(integer_type_bounds(&Type::TypeParam("T".to_string())), None);
    assert_eq!(
        integer_type_bounds(&Type::Named("Vec".to_string(), vec![Type::named("int32")])),
        None
    );
    assert_eq!(
        integer_type_bounds(&Type::named("uintsize")),
        Some(IntegerBounds::Unsigned {
            max: usize::MAX as u128
        })
    );
    assert_eq!(
        integer_type_bounds(&Type::named("intsize")),
        Some(IntegerBounds::Signed {
            min: isize::MIN as i128,
            max: isize::MAX as i128,
        })
    );
    assert_eq!(
        integer_type_bounds(&Type::named("uint128")),
        Some(IntegerBounds::Unsigned { max: u128::MAX })
    );

    assert_eq!(
        IntegerValue::zero().checked_neg(),
        Some(IntegerValue::zero())
    );
    assert_eq!(
        IntegerValue::from_signed(3).checked_neg(),
        Some(IntegerValue::from_signed(-3))
    );
    assert_eq!(
        IntegerValue::from_signed(-3).checked_neg(),
        Some(IntegerValue::from_signed(3))
    );
    assert_eq!(
        IntegerValue::from_signed(4).checked_add(IntegerValue::from_signed(-4)),
        Some(IntegerValue::zero())
    );
    assert_eq!(
        IntegerValue::from_signed(-4).checked_add(IntegerValue::from_signed(-5)),
        Some(IntegerValue::from_signed(-9))
    );
    assert_eq!(
        IntegerValue::from_signed(9).checked_sub(IntegerValue::from_signed(4)),
        Some(IntegerValue::from_signed(5))
    );
    assert_eq!(
        IntegerValue::from_signed(-3).checked_sub(IntegerValue::from_signed(-7)),
        Some(IntegerValue::from_signed(4))
    );
    assert_eq!(
        IntegerValue::from_literal(3).checked_mul(IntegerValue::zero()),
        Some(IntegerValue::zero())
    );
    assert_eq!(
        IntegerValue::from_literal(u128::MAX).checked_add(IntegerValue::from_literal(1)),
        None
    );
    assert_eq!(
        IntegerValue::from_literal(u128::MAX).checked_mul(IntegerValue::from_literal(2)),
        None
    );
    assert!(IntegerValue::Signed(3).fits_bounds(IntegerBounds::Unsigned { max: 5 }));
    assert!(!IntegerValue::Signed(-1).fits_bounds(IntegerBounds::Unsigned { max: 5 }));
    assert_eq!(
        IntegerValue::from_sign_and_magnitude(IntegerSign::Zero, 9),
        Some(IntegerValue::zero())
    );
    assert_eq!(
        IntegerValue::from_sign_and_magnitude(IntegerSign::Positive, 9),
        Some(IntegerValue::from_literal(9))
    );
    assert_eq!(
        IntegerValue::from_sign_and_magnitude(IntegerSign::Negative, 9),
        Some(IntegerValue::from_signed(-9))
    );
    assert_eq!(
        IntegerValue::from_sign_and_magnitude(IntegerSign::Negative, 1u128 << 127),
        Some(IntegerValue::Signed(i128::MIN))
    );
    assert_eq!(
        IntegerValue::from_sign_and_magnitude(IntegerSign::Negative, (1u128 << 127) + 1),
        None
    );
}
