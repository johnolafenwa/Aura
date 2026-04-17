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
        IntegerValue::from_literal(3).checked_mul(IntegerValue::zero()),
        Some(IntegerValue::zero())
    );
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
