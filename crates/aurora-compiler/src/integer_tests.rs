use super::{
    integer_type_bounds, minimal_signed_type_for_negative_literal, IntegerBounds, IntegerKind,
    IntegerRepresentation, IntegerSign, IntegerValue,
};
use crate::sema::Type;
use std::cmp::Ordering;

#[test]
fn runtime_integer_kinds_report_every_language_integer_name() {
    let cases = [
        (IntegerKind::Int8, "int8"),
        (IntegerKind::Int16, "int16"),
        (IntegerKind::Int32, "int32"),
        (IntegerKind::Int64, "int64"),
        (IntegerKind::Int128, "int128"),
        (IntegerKind::IntSize, "intsize"),
        (IntegerKind::Uint8, "uint8"),
        (IntegerKind::Uint16, "uint16"),
        (IntegerKind::Uint32, "uint32"),
        (IntegerKind::Uint64, "uint64"),
        (IntegerKind::Uint128, "uint128"),
        (IntegerKind::UintSize, "uintsize"),
    ];

    for (kind, name) in cases {
        assert_eq!(kind.runtime_type_name(), name);
        assert_eq!(IntegerKind::from_runtime_type_name(name), Some(kind));
    }
    assert_eq!(IntegerKind::from_runtime_type_name("float64"), None);
    assert_eq!(IntegerValue::from_i32(7).runtime_type_name(), Some("int32"));
    assert_eq!(IntegerValue::from_i64(7).runtime_type_name(), Some("int64"));
    assert_eq!(
        IntegerValue::from_u64(7).runtime_type_name(),
        Some("uint64")
    );
    assert_eq!(IntegerValue::from_literal(7).runtime_type_name(), None);
}

#[test]
fn integer_floor_division_and_modulo_follow_the_divisor_sign() {
    for (left, right, quotient, remainder) in [
        (7, 3, 2, 1),
        (-7, 3, -3, 2),
        (7, -3, -3, -2),
        (-7, -3, 2, -1),
    ] {
        let left = IntegerValue::from_signed(left);
        let right = IntegerValue::from_signed(right);
        assert_eq!(
            left.checked_floor_div(right),
            Some(IntegerValue::from_signed(quotient))
        );
        assert_eq!(
            left.checked_floor_rem(right),
            Some(IntegerValue::from_signed(remainder))
        );
    }

    let zero = IntegerValue::zero();
    let seven = IntegerValue::from_signed(7);
    assert_eq!(
        IntegerValue::from_signed(1).checked_div(IntegerValue::from_signed(2)),
        Some(zero),
        "truncating division must canonicalize a zero quotient without a sign"
    );
    assert_eq!(seven.checked_floor_div(zero), None);
    assert_eq!(seven.checked_floor_rem(zero), None);

    let minimum = IntegerValue::from_i64(i64::MIN);
    let negative_one = IntegerValue::from_i64(-1);
    assert_eq!(
        minimum.checked_floor_div(negative_one),
        Some(IntegerValue::from_literal((i64::MAX as u128) + 1))
    );
    assert_eq!(
        minimum.checked_floor_rem(negative_one),
        Some(IntegerValue::from_i64(0))
    );
}

#[test]
fn typed_integer_constructors_enforce_each_declared_width() {
    let signed_cases = [
        (IntegerKind::Int8, i8::MIN as i128, i8::MAX as i128),
        (IntegerKind::Int16, i16::MIN as i128, i16::MAX as i128),
        (IntegerKind::Int32, i32::MIN as i128, i32::MAX as i128),
        (IntegerKind::Int64, i64::MIN as i128, i64::MAX as i128),
        (IntegerKind::Int128, i128::MIN, i128::MAX),
        (IntegerKind::IntSize, isize::MIN as i128, isize::MAX as i128),
    ];
    for (kind, min, max) in signed_cases {
        assert_eq!(
            IntegerValue::from_typed_signed(min, kind)
                .unwrap()
                .runtime_kind(),
            Some(kind)
        );
        assert_eq!(
            IntegerValue::from_typed_signed(max, kind)
                .unwrap()
                .runtime_kind(),
            Some(kind)
        );
        if min > i128::MIN {
            assert_eq!(IntegerValue::from_typed_signed(min - 1, kind), None);
        }
        if max < i128::MAX {
            assert_eq!(IntegerValue::from_typed_signed(max + 1, kind), None);
        }
    }

    let unsigned_cases = [
        (IntegerKind::Uint8, u8::MAX as u128),
        (IntegerKind::Uint16, u16::MAX as u128),
        (IntegerKind::Uint32, u32::MAX as u128),
        (IntegerKind::Uint64, u64::MAX as u128),
        (IntegerKind::Uint128, u128::MAX),
        (IntegerKind::UintSize, usize::MAX as u128),
    ];
    for (kind, max) in unsigned_cases {
        assert_eq!(
            IntegerValue::from_typed_unsigned(max, kind)
                .unwrap()
                .runtime_kind(),
            Some(kind)
        );
        if max < u128::MAX {
            assert_eq!(IntegerValue::from_typed_unsigned(max + 1, kind), None);
        }
    }
    assert_eq!(IntegerValue::from_typed_signed(1, IntegerKind::Uint8), None);
    assert_eq!(
        IntegerValue::from_typed_unsigned(1, IntegerKind::Int8),
        None
    );
}

#[test]
fn typed_integer_metadata_survives_copy_containers_and_serde() {
    let original = IntegerValue::from_i32(23);
    let copied = Some(original);
    let values = [copied.unwrap(), original];

    assert_eq!(values[0].runtime_kind(), Some(IntegerKind::Int32));
    assert_eq!(
        values[1].representation(),
        IntegerRepresentation::Unsigned(23)
    );
    assert_eq!(IntegerValue::from_i32(23), IntegerValue::from_i64(23));
    assert_eq!(
        IntegerValue::from_u64(23).cmp(&IntegerValue::from_i32(23)),
        Ordering::Equal
    );

    let encoded = serde_json::to_string(&values[0]).expect("typed integer should serialize");
    let decoded: IntegerValue =
        serde_json::from_str(&encoded).expect("typed integer should deserialize");
    assert_eq!(decoded.runtime_kind(), Some(IntegerKind::Int32));
    assert_eq!(
        decoded.representation(),
        IntegerRepresentation::Unsigned(23)
    );
}

#[test]
fn checked_arithmetic_preserves_common_width_and_exposes_out_of_width_results_for_validation() {
    let one_i32 = IntegerValue::from_i32(1);
    let two_i32 = IntegerValue::from_i32(2);
    let six_i32 = IntegerValue::from_i32(6);

    for result in [
        one_i32.checked_neg().unwrap(),
        one_i32.checked_add(two_i32).unwrap(),
        six_i32.checked_sub(two_i32).unwrap(),
        two_i32.checked_mul(two_i32).unwrap(),
        six_i32.checked_div(two_i32).unwrap(),
        six_i32.checked_rem(IntegerValue::from_i32(4)).unwrap(),
    ] {
        assert_eq!(result.runtime_kind(), Some(IntegerKind::Int32));
    }

    for (result, expected) in [
        (
            IntegerValue::from_i32(i32::MAX)
                .checked_add(one_i32)
                .unwrap(),
            IntegerValue::from_literal(i32::MAX as u128 + 1),
        ),
        (
            IntegerValue::from_i32(i32::MIN)
                .checked_sub(one_i32)
                .unwrap(),
            IntegerValue::from_signed(i32::MIN as i128 - 1),
        ),
        (
            IntegerValue::from_i32(46_341)
                .checked_mul(IntegerValue::from_i32(46_341))
                .unwrap(),
            IntegerValue::from_literal(2_147_488_281),
        ),
        (
            IntegerValue::from_i32(i32::MIN).checked_neg().unwrap(),
            IntegerValue::from_literal(2_147_483_648),
        ),
        (
            IntegerValue::from_i32(i32::MIN)
                .checked_div(IntegerValue::from_i32(-1))
                .unwrap(),
            IntegerValue::from_literal(2_147_483_648),
        ),
        (
            IntegerValue::from_u64(0)
                .checked_sub(IntegerValue::from_u64(1))
                .unwrap(),
            IntegerValue::from_signed(-1),
        ),
        (
            IntegerValue::from_u64(u64::MAX)
                .checked_add(IntegerValue::from_u64(1))
                .unwrap(),
            IntegerValue::from_literal(u64::MAX as u128 + 1),
        ),
    ] {
        assert_eq!(result, expected);
        assert_eq!(result.runtime_kind(), None);
    }

    let mixed = IntegerValue::from_i32(1)
        .checked_add(IntegerValue::from_i64(2))
        .expect("legacy numeric arithmetic remains available across distinct tags");
    assert_eq!(mixed, IntegerValue::from_literal(3));
    assert_eq!(mixed.runtime_kind(), None);
}

#[test]
fn wrapping_integer_arithmetic_preserves_declared_width_and_wraps_at_both_bounds() {
    let one_i32 = IntegerValue::from_i32(1);
    let two_i32 = IntegerValue::from_i32(2);
    assert_eq!(
        IntegerValue::from_i32(i32::MAX).wrapping_add(one_i32),
        Some(IntegerValue::from_i32(i32::MIN))
    );
    assert_eq!(
        IntegerValue::from_i32(i32::MIN).wrapping_sub(one_i32),
        Some(IntegerValue::from_i32(i32::MAX))
    );
    assert_eq!(
        IntegerValue::from_i32(i32::MAX).wrapping_mul(two_i32),
        Some(IntegerValue::from_i32(-2))
    );
    assert_eq!(
        IntegerValue::from_i64(i64::MAX).wrapping_add(IntegerValue::from_i64(1)),
        Some(IntegerValue::from_i64(i64::MIN))
    );
    assert_eq!(
        IntegerValue::from_typed_unsigned(u8::MAX as u128, IntegerKind::Uint8)
            .unwrap()
            .wrapping_add(IntegerValue::from_typed_unsigned(1, IntegerKind::Uint8).unwrap()),
        Some(IntegerValue::from_typed_unsigned(0, IntegerKind::Uint8).unwrap())
    );
    assert_eq!(
        IntegerValue::from_typed_signed(i128::MAX, IntegerKind::Int128)
            .unwrap()
            .wrapping_add(IntegerValue::from_typed_signed(1, IntegerKind::Int128).unwrap()),
        Some(IntegerValue::from_typed_signed(i128::MIN, IntegerKind::Int128).unwrap())
    );

    assert_eq!(
        one_i32.wrapping_add(IntegerValue::from_i64(1)),
        None,
        "mixed runtime widths must not silently choose a wrapping modulus"
    );
    assert_eq!(
        IntegerValue::from_literal(1).wrapping_add(IntegerValue::from_literal(2)),
        None,
        "untyped literal values have no wrapping width"
    );
}

#[test]
fn saturating_integer_arithmetic_clamps_to_the_declared_bounds() {
    let one_i32 = IntegerValue::from_i32(1);
    let two_i32 = IntegerValue::from_i32(2);
    assert_eq!(
        IntegerValue::from_i32(i32::MAX).saturating_add(one_i32),
        Some(IntegerValue::from_i32(i32::MAX))
    );
    assert_eq!(
        IntegerValue::from_i32(i32::MIN).saturating_sub(one_i32),
        Some(IntegerValue::from_i32(i32::MIN))
    );
    assert_eq!(
        IntegerValue::from_i32(i32::MAX).saturating_mul(two_i32),
        Some(IntegerValue::from_i32(i32::MAX))
    );
    assert_eq!(
        IntegerValue::from_i64(i64::MIN).saturating_mul(IntegerValue::from_i64(-1)),
        Some(IntegerValue::from_i64(i64::MAX))
    );
    assert_eq!(
        IntegerValue::from_typed_unsigned(0, IntegerKind::Uint8)
            .unwrap()
            .saturating_sub(IntegerValue::from_typed_unsigned(1, IntegerKind::Uint8).unwrap()),
        Some(IntegerValue::from_typed_unsigned(0, IntegerKind::Uint8).unwrap())
    );
    assert_eq!(
        IntegerValue::from_typed_unsigned(u128::MAX, IntegerKind::Uint128)
            .unwrap()
            .saturating_add(IntegerValue::from_typed_unsigned(1, IntegerKind::Uint128).unwrap()),
        Some(IntegerValue::from_typed_unsigned(u128::MAX, IntegerKind::Uint128).unwrap())
    );

    assert_eq!(
        one_i32.saturating_mul(IntegerValue::from_i64(1)),
        None,
        "mixed runtime widths must not silently choose saturation bounds"
    );
    assert_eq!(
        IntegerValue::from_literal(1).saturating_add(IntegerValue::from_literal(2)),
        None
    );
}

#[test]
fn d3_negative_literal_default_is_int64_and_does_not_widen_implicitly() {
    assert_eq!(
        minimal_signed_type_for_negative_literal(7),
        Some(Type::named("int64"))
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i64::MAX as u128) + 1),
        Some(Type::named("int64"))
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i64::MAX as u128) + 2),
        None
    );
}

#[test]
fn integer_value_helpers_cover_division_remainder_comparisons_and_bounds() {
    let negative = IntegerValue::from_signed(-9);
    let positive = IntegerValue::from_signed(4);
    let zero = IntegerValue::zero();
    let signed_positive = IntegerValue::from_signed(5);
    let explicit_signed_positive =
        IntegerValue::from_representation(IntegerRepresentation::Signed(5));

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
        Some(Type::named("int64"))
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i32::MAX as u128) + 2),
        Some(Type::named("int64"))
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i64::MAX as u128) + 2),
        None
    );
    assert_eq!(
        minimal_signed_type_for_negative_literal((i64::MAX as u128) + 10_000),
        None
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
    assert!(
        IntegerValue::from_representation(IntegerRepresentation::Signed(3))
            .fits_bounds(IntegerBounds::Unsigned { max: 5 })
    );
    assert!(
        !IntegerValue::from_representation(IntegerRepresentation::Signed(-1))
            .fits_bounds(IntegerBounds::Unsigned { max: 5 })
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
        Some(IntegerValue::from_signed(i128::MIN))
    );
    assert_eq!(
        IntegerValue::from_sign_and_magnitude(IntegerSign::Negative, (1u128 << 127) + 1),
        None
    );
}
