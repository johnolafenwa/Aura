
use super::{
    integer_type_bounds, minimal_signed_type_for_negative_literal, IntegerBounds, IntegerValue,
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
        Type::named("int32")
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i32::MAX as u128) + 2),
        Type::named("int64")
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i64::MAX as u128) + 2),
        Type::named("int128")
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i64::MAX as u128) + 10_000),
        Type::named("int128")
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
}
