use super::{ArrayCompareOp, ArrayMaskOp, ArrayPairOp, ArrayUnaryOp};
use crate::array_views::ArrayAxisSelection::Slice;
use crate::integer::IntegerValue;
use crate::runtime_value::{ArrayDType, ArrayStorage, ArrayValue, Value};

fn floats(shape: &[usize], values: &[f64]) -> ArrayValue {
    ArrayValue::new(shape.into(), ArrayStorage::Float64(values.to_vec().into())).unwrap()
}

fn singles(shape: &[usize], values: &[f32]) -> ArrayValue {
    ArrayValue::new(shape.into(), ArrayStorage::Float32(values.to_vec().into())).unwrap()
}

fn ints(shape: &[usize], values: &[i64]) -> ArrayValue {
    ArrayValue::new(shape.into(), ArrayStorage::Int64(values.to_vec().into())).unwrap()
}

fn shorts(shape: &[usize], values: &[i32]) -> ArrayValue {
    ArrayValue::new(shape.into(), ArrayStorage::Int32(values.to_vec().into())).unwrap()
}

fn mask(shape: &[usize], values: &[bool]) -> ArrayValue {
    ArrayValue::new(shape.into(), ArrayStorage::Bool(values.to_vec().into())).unwrap()
}

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn short(value: i32) -> Value {
    Value::Int(IntegerValue::from_i32(value))
}

fn reversed(array: &ArrayValue) -> ArrayValue {
    array
        .select(&[Slice {
            start: None,
            end: None,
            step: -1,
        }])
        .unwrap()
}

fn message<T: std::fmt::Debug>(result: Result<T, crate::diag::Diagnostic>) -> String {
    let error = result.unwrap_err();
    format!("{} {}", error.code, error.message)
}

#[test]
fn operation_codes_and_names_round_trip() {
    for operation in ArrayUnaryOp::ALL {
        assert_eq!(ArrayUnaryOp::from_code(*operation as i64), Some(*operation));
        assert_eq!(ArrayUnaryOp::from_name(operation.name()), Some(*operation));
    }
    for operation in ArrayPairOp::ALL {
        assert_eq!(ArrayPairOp::from_code(*operation as i64), Some(*operation));
        assert_eq!(ArrayPairOp::from_name(operation.name()), Some(*operation));
    }
    for operation in ArrayCompareOp::ALL {
        assert_eq!(
            ArrayCompareOp::from_code(*operation as i64),
            Some(*operation)
        );
        assert_eq!(
            ArrayCompareOp::from_name(operation.name()),
            Some(*operation)
        );
    }
    for operation in ArrayMaskOp::ALL {
        assert_eq!(ArrayMaskOp::from_code(*operation as i64), Some(*operation));
        assert_eq!(ArrayMaskOp::from_name(operation.name()), Some(*operation));
    }
    assert_eq!(ArrayUnaryOp::from_code(99), None);
    assert_eq!(ArrayPairOp::from_code(-1), None);
    assert_eq!(ArrayCompareOp::from_name("same"), None);
    assert_eq!(ArrayMaskOp::from_code(3), None);
}

#[test]
fn abs_covers_every_numeric_dtype_and_reports_the_integer_minimum() {
    assert_eq!(
        ints(&[3], &[-2, 0, 5]).unary(ArrayUnaryOp::Abs).unwrap(),
        ints(&[3], &[2, 0, 5])
    );
    assert_eq!(
        shorts(&[2], &[-7, 7]).unary(ArrayUnaryOp::Abs).unwrap(),
        shorts(&[2], &[7, 7])
    );
    assert_eq!(
        floats(&[2], &[-1.5, 2.0]).unary(ArrayUnaryOp::Abs).unwrap(),
        floats(&[2], &[1.5, 2.0])
    );
    assert_eq!(
        singles(&[2], &[-1.5, 2.0])
            .unary(ArrayUnaryOp::Abs)
            .unwrap(),
        singles(&[2], &[1.5, 2.0])
    );
    assert_eq!(
        message(ints(&[2], &[1, i64::MIN]).unary(ArrayUnaryOp::Abs)),
        "AU4002 array abs overflowed at flat index 1"
    );
    assert_eq!(
        message(shorts(&[1], &[i32::MIN]).unary(ArrayUnaryOp::Abs)),
        "AU4002 array abs overflowed at flat index 0"
    );
    assert_eq!(
        message(mask(&[1], &[true]).unary(ArrayUnaryOp::Abs)),
        "AU4007 array abs requires numeric elements, found bool"
    );
}

#[test]
fn math_members_follow_the_scalar_functions() {
    let values = floats(&[2, 2], &[1.0, 4.0, 0.25, 9.0]);
    assert_eq!(
        values.unary(ArrayUnaryOp::Sqrt).unwrap(),
        floats(&[2, 2], &[1.0, 2.0, 0.5, 3.0])
    );
    let ArrayStorage::Float64(roots) = floats(&[1], &[-1.0])
        .unary(ArrayUnaryOp::Sqrt)
        .unwrap()
        .base
    else {
        panic!("sqrt keeps float64")
    };
    assert!(roots[0].is_nan());
    for (operation, function) in [
        (ArrayUnaryOp::Exp, f64::exp as fn(f64) -> f64),
        (ArrayUnaryOp::Log, f64::ln),
        (ArrayUnaryOp::Log2, f64::log2),
        (ArrayUnaryOp::Log10, f64::log10),
        (ArrayUnaryOp::Sin, f64::sin),
        (ArrayUnaryOp::Cos, f64::cos),
        (ArrayUnaryOp::Tan, f64::tan),
    ] {
        let expected: Vec<f64> = [1.0, 4.0, 0.25, 9.0].map(function).to_vec();
        assert_eq!(
            values.unary(operation).unwrap(),
            floats(&[2, 2], &expected),
            "{}",
            operation.name()
        );
        // A view is read in its own order.
        let turned = reversed(&floats(&[4], &[9.0, 0.25, 4.0, 1.0]));
        assert_eq!(turned.unary(operation).unwrap(), floats(&[4], &expected));
    }
    assert_eq!(
        message(floats(&[3], &[1.0, 2.0, -1.0]).unary(ArrayUnaryOp::Log)),
        "AU4001 `math.log` domain error for `-1` at flat index 2"
    );
    assert_eq!(
        message(floats(&[1], &[f64::INFINITY]).unary(ArrayUnaryOp::Sin)),
        "AU4001 `math.sin` domain error for `inf` at flat index 0"
    );
    assert_eq!(
        message(floats(&[2], &[0.0, 1e6]).unary(ArrayUnaryOp::Exp)),
        "AU4002 `math.exp` overflow at flat index 1"
    );
    for operation in [ArrayUnaryOp::Sqrt, ArrayUnaryOp::Exp, ArrayUnaryOp::Floor] {
        assert_eq!(
            message(singles(&[1], &[1.0]).unary(operation)),
            format!(
                "AU4007 array {} requires float64 elements, found float32",
                operation.name()
            )
        );
    }
}

#[test]
fn rounding_members_give_int64_and_report_values_out_of_range() {
    let values = floats(&[4], &[1.5, -1.5, 2.0, -0.25]);
    assert_eq!(
        values.unary(ArrayUnaryOp::Floor).unwrap(),
        ints(&[4], &[1, -2, 2, -1])
    );
    assert_eq!(
        values.unary(ArrayUnaryOp::Ceil).unwrap(),
        ints(&[4], &[2, -1, 2, 0])
    );
    assert_eq!(
        values.unary(ArrayUnaryOp::Trunc).unwrap(),
        ints(&[4], &[1, -1, 2, 0])
    );
    assert_eq!(
        message(floats(&[2], &[0.0, 1e300]).unary(ArrayUnaryOp::Floor)),
        "AU4002 `math.floor` result cannot be represented as `int64` at flat index 1"
    );
    assert_eq!(
        message(floats(&[1], &[f64::NAN]).unary(ArrayUnaryOp::Trunc)),
        "AU4002 `math.trunc` result cannot be represented as `int64` at flat index 0"
    );
}

#[test]
fn to_float_and_logical_not_require_their_dtypes() {
    assert_eq!(
        ints(&[2], &[3, -4]).unary(ArrayUnaryOp::ToFloat).unwrap(),
        floats(&[2], &[3.0, -4.0])
    );
    assert_eq!(
        shorts(&[2], &[3, -4]).unary(ArrayUnaryOp::ToFloat).unwrap(),
        floats(&[2], &[3.0, -4.0])
    );
    assert_eq!(
        ints(&[1], &[i64::MAX])
            .unary(ArrayUnaryOp::ToFloat)
            .unwrap(),
        floats(&[1], &[9_223_372_036_854_775_808.0])
    );
    assert_eq!(
        message(floats(&[1], &[1.0]).unary(ArrayUnaryOp::ToFloat)),
        "AU4007 array to_float requires integer elements, found float64"
    );
    assert_eq!(
        mask(&[2], &[true, false])
            .unary(ArrayUnaryOp::LogicalNot)
            .unwrap(),
        mask(&[2], &[false, true])
    );
    assert_eq!(
        message(ints(&[1], &[1]).unary(ArrayUnaryOp::LogicalNot)),
        "AU4007 array logical_not requires bool elements, found int64"
    );
}

#[test]
fn minimum_and_maximum_broadcast_and_order_floats_as_min_and_max_do() {
    let left = ints(&[2, 2], &[1, 8, 5, 2]);
    assert_eq!(
        left.pairwise(&Value::Array(ints(&[2], &[4, 4])), ArrayPairOp::Minimum)
            .unwrap(),
        ints(&[2, 2], &[1, 4, 4, 2])
    );
    assert_eq!(
        left.pairwise(&int(4), ArrayPairOp::Maximum).unwrap(),
        ints(&[2, 2], &[4, 8, 5, 4])
    );
    // A column operand repeats along each row, and a row along each column.
    assert_eq!(
        ints(&[1, 2], &[3, 6])
            .pairwise(&Value::Array(ints(&[2, 1], &[4, 5])), ArrayPairOp::Maximum)
            .unwrap(),
        ints(&[2, 2], &[4, 6, 5, 6])
    );
    assert_eq!(
        shorts(&[2], &[1, 9])
            .pairwise(&short(5), ArrayPairOp::Minimum)
            .unwrap(),
        shorts(&[2], &[1, 5])
    );
    assert_eq!(
        shorts(&[2], &[1, 9])
            .pairwise(&short(5), ArrayPairOp::Maximum)
            .unwrap(),
        shorts(&[2], &[5, 9])
    );
    assert_eq!(
        singles(&[2], &[1.0, 9.0])
            .pairwise(&Value::Float(5.0), ArrayPairOp::Minimum)
            .unwrap(),
        singles(&[2], &[1.0, 5.0])
    );
    assert_eq!(
        singles(&[2], &[1.0, 9.0])
            .pairwise(&Value::Float(5.0), ArrayPairOp::Maximum)
            .unwrap(),
        singles(&[2], &[5.0, 9.0])
    );
    // A strided operand is read where it lies.
    let turned = reversed(&floats(&[3], &[3.0, 2.0, 1.0]));
    assert_eq!(
        turned
            .pairwise(
                &Value::Array(reversed(&floats(&[3], &[0.0, 5.0, 2.0]))),
                ArrayPairOp::Minimum
            )
            .unwrap(),
        floats(&[3], &[1.0, 2.0, 0.0])
    );
    let bits = |array: ArrayValue| match array.base {
        ArrayStorage::Float64(values) => values.iter().map(|value| value.to_bits()).collect(),
        _ => Vec::new(),
    };
    let signed = floats(&[4], &[0.0, -0.0, f64::NAN, 1.0]);
    let other = Value::Array(floats(&[4], &[-0.0, 0.0, 1.0, f64::NAN]));
    let smaller = bits(signed.pairwise(&other, ArrayPairOp::Minimum).unwrap());
    let larger = bits(signed.pairwise(&other, ArrayPairOp::Maximum).unwrap());
    assert_eq!(&smaller[..2], [(-0.0_f64).to_bits(); 2]);
    assert_eq!(&larger[..2], [0.0_f64.to_bits(); 2]);
    for values in [&smaller, &larger] {
        assert!(f64::from_bits(values[2]).is_nan() && f64::from_bits(values[3]).is_nan());
    }
    assert_eq!(
        message(mask(&[1], &[true]).pairwise(&Value::Bool(true), ArrayPairOp::Minimum)),
        "AU4007 array minimum requires numeric elements, found bool"
    );
    assert_eq!(
        message(left.pairwise(&Value::Array(ints(&[3], &[1, 2, 3])), ArrayPairOp::Minimum)),
        "AU4007 array minimum requires the same dtype and shapes that broadcast, found [2, 2] int64 and [3] int64"
    );
    assert_eq!(
        message(left.pairwise(&Value::Array(floats(&[2], &[1.0, 2.0])), ArrayPairOp::Maximum)),
        "AU4007 array maximum requires the same dtype and shapes that broadcast, found [2, 2] int64 and [2] float64"
    );
    assert!(left
        .pairwise(&Value::Float(1.0), ArrayPairOp::Minimum)
        .is_err());
}

#[test]
fn pow_follows_math_pow_and_reports_the_failing_element() {
    assert_eq!(
        floats(&[3], &[2.0, 3.0, 4.0])
            .pairwise(&Value::Float(2.0), ArrayPairOp::Pow)
            .unwrap(),
        floats(&[3], &[4.0, 9.0, 16.0])
    );
    assert_eq!(
        floats(&[2], &[2.0, 3.0])
            .pairwise(&Value::Array(floats(&[2], &[3.0, 0.0])), ArrayPairOp::Pow)
            .unwrap(),
        floats(&[2], &[8.0, 1.0])
    );
    let failure =
        message(floats(&[2], &[1.0, -8.0]).pairwise(&Value::Float(0.5), ArrayPairOp::Pow));
    assert!(failure.ends_with(" at flat index 1"), "{failure}");
    assert_eq!(
        message(ints(&[1], &[2]).pairwise(&int(2), ArrayPairOp::Pow)),
        "AU4007 array pow requires float64 elements, found int64"
    );
}

#[test]
fn comparisons_give_masks_for_every_numeric_dtype() {
    let left = ints(&[4], &[1, 2, 3, 4]);
    let right = Value::Array(ints(&[4], &[4, 2, 1, 9]));
    for (operation, expected) in [
        (ArrayCompareOp::Less, [true, false, false, true]),
        (ArrayCompareOp::LessEqual, [true, true, false, true]),
        (ArrayCompareOp::Greater, [false, false, true, false]),
        (ArrayCompareOp::GreaterEqual, [false, true, true, false]),
        (ArrayCompareOp::Equal, [false, true, false, false]),
        (ArrayCompareOp::NotEqual, [true, false, true, true]),
    ] {
        assert_eq!(
            left.compare(&right, operation).unwrap(),
            mask(&[4], &expected),
            "{}",
            operation.name()
        );
    }
    assert_eq!(
        left.compare(&int(2), ArrayCompareOp::Greater).unwrap(),
        mask(&[4], &[false, false, true, true])
    );
    assert_eq!(
        shorts(&[2], &[1, 5])
            .compare(&short(3), ArrayCompareOp::Less)
            .unwrap(),
        mask(&[2], &[true, false])
    );
    assert_eq!(
        singles(&[2], &[1.0, 5.0])
            .compare(&Value::Float(3.0), ArrayCompareOp::Less)
            .unwrap(),
        mask(&[2], &[true, false])
    );
    // A NaN compares false, except through `not_equal`.
    let with_nan = floats(&[2], &[f64::NAN, 1.0]);
    assert_eq!(
        with_nan
            .compare(&Value::Array(with_nan.clone()), ArrayCompareOp::Equal)
            .unwrap(),
        mask(&[2], &[false, true])
    );
    assert_eq!(
        with_nan
            .compare(&Value::Float(1.0), ArrayCompareOp::NotEqual)
            .unwrap(),
        mask(&[2], &[true, false])
    );
    // A matrix against a row, and one element on the left of a run.
    assert_eq!(
        floats(&[2, 2], &[1.0, 5.0, 3.0, 0.0])
            .compare(
                &Value::Array(floats(&[2], &[2.0, 2.0])),
                ArrayCompareOp::Less
            )
            .unwrap(),
        mask(&[2, 2], &[true, false, false, true])
    );
    assert_eq!(
        floats(&[1], &[2.0])
            .compare(
                &Value::Array(floats(&[3], &[1.0, 2.0, 3.0])),
                ArrayCompareOp::Less
            )
            .unwrap(),
        mask(&[3], &[false, false, true])
    );
    assert_eq!(
        message(mask(&[1], &[true]).compare(&Value::Bool(true), ArrayCompareOp::Equal)),
        "AU4007 array equal requires numeric elements, found bool"
    );
    assert!(left
        .compare(&Value::Float(1.0), ArrayCompareOp::Less)
        .is_err());
    assert!(left
        .compare(&Value::Array(ints(&[3], &[1, 2, 3])), ArrayCompareOp::Less)
        .is_err());
}

#[test]
fn masks_combine_reduce_and_choose() {
    let left = mask(&[4], &[true, true, false, false]);
    let right = Value::Array(mask(&[4], &[true, false, true, false]));
    assert_eq!(
        left.pairwise(&right, ArrayPairOp::LogicalAnd).unwrap(),
        mask(&[4], &[true, false, false, false])
    );
    assert_eq!(
        left.pairwise(&right, ArrayPairOp::LogicalOr).unwrap(),
        mask(&[4], &[true, true, true, false])
    );
    assert_eq!(
        left.pairwise(&Value::Bool(true), ArrayPairOp::LogicalAnd)
            .unwrap(),
        left
    );
    assert_eq!(
        message(ints(&[1], &[1]).pairwise(&int(1), ArrayPairOp::LogicalOr)),
        "AU4007 array logical_or requires bool elements, found int64"
    );
    assert_eq!(
        left.mask_reduce(ArrayMaskOp::Any).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        left.mask_reduce(ArrayMaskOp::All).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(left.mask_reduce(ArrayMaskOp::Count).unwrap(), int(2));
    let none = mask(&[2], &[false, false]);
    assert_eq!(
        none.mask_reduce(ArrayMaskOp::Any).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        reversed(&mask(&[2], &[true, true]))
            .mask_reduce(ArrayMaskOp::All)
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        message(ints(&[1], &[1]).mask_reduce(ArrayMaskOp::Any)),
        "AU4007 array any requires bool elements, found int64"
    );

    let values = Value::Array(floats(&[4], &[1.0, 2.0, 3.0, 4.0]));
    assert_eq!(
        left.choose(&values, &Value::Float(0.0), ArrayDType::Float64)
            .unwrap(),
        floats(&[4], &[1.0, 2.0, 0.0, 0.0])
    );
    assert_eq!(
        left.choose(&Value::Float(-1.0), &values, ArrayDType::Float64)
            .unwrap(),
        floats(&[4], &[-1.0, -1.0, 3.0, 4.0])
    );
    assert_eq!(
        left.choose(&int(1), &int(0), ArrayDType::Int64).unwrap(),
        ints(&[4], &[1, 1, 0, 0])
    );
    assert_eq!(
        left.choose(&short(1), &short(0), ArrayDType::Int32)
            .unwrap(),
        shorts(&[4], &[1, 1, 0, 0])
    );
    assert_eq!(
        left.choose(&Value::Float(1.0), &Value::Float(0.0), ArrayDType::Float32)
            .unwrap(),
        singles(&[4], &[1.0, 1.0, 0.0, 0.0])
    );
    assert_eq!(
        left.choose(&Value::Bool(false), &right, ArrayDType::Bool)
            .unwrap(),
        mask(&[4], &[false, false, true, false])
    );
    // A mask of one row chooses along each row of a matrix.
    assert_eq!(
        mask(&[2], &[true, false])
            .choose(
                &Value::Array(ints(&[2, 2], &[1, 2, 3, 4])),
                &Value::Array(ints(&[2, 1], &[7, 8])),
                ArrayDType::Int64
            )
            .unwrap(),
        ints(&[2, 2], &[1, 7, 3, 8])
    );
    assert_eq!(
        message(ints(&[1], &[1]).choose(&int(1), &int(0), ArrayDType::Int64)),
        "AU4007 array choose requires bool elements, found int64"
    );
    assert_eq!(
        message(left.choose(&values, &int(0), ArrayDType::Int64)),
        "AU4007 array choose requires int64 values whose shapes broadcast with the mask, found [4] float64 for a mask of [4]"
    );
    assert!(left
        .choose(
            &Value::Array(ints(&[3], &[1, 2, 3])),
            &int(0),
            ArrayDType::Int64
        )
        .is_err());
    assert!(left
        .choose(&Value::Float(1.0), &int(0), ArrayDType::Int64)
        .is_err());
    assert!(left
        .choose(&int(1), &Value::Float(1.0), ArrayDType::Int64)
        .is_err());
}

#[test]
fn casts_convert_each_element_as_the_scalar_cast_does() {
    let small = shorts(&[2], &[3, -4]);
    assert_eq!(small.cast(ArrayDType::Int64).unwrap(), ints(&[2], &[3, -4]));
    assert_eq!(
        small.cast(ArrayDType::Float64).unwrap(),
        floats(&[2], &[3.0, -4.0])
    );
    assert_eq!(
        small.cast(ArrayDType::Float32).unwrap(),
        singles(&[2], &[3.0, -4.0])
    );
    let wide = ints(&[2], &[3, -4]);
    assert_eq!(wide.cast(ArrayDType::Int32).unwrap(), small);
    assert_eq!(
        wide.cast(ArrayDType::Float64).unwrap(),
        floats(&[2], &[3.0, -4.0])
    );
    assert_eq!(
        wide.cast(ArrayDType::Float32).unwrap(),
        singles(&[2], &[3.0, -4.0])
    );
    let single = singles(&[2], &[3.75, -4.5]);
    assert_eq!(single.cast(ArrayDType::Int32).unwrap(), small);
    assert_eq!(single.cast(ArrayDType::Int64).unwrap(), wide);
    assert_eq!(
        single.cast(ArrayDType::Float64).unwrap(),
        floats(&[2], &[3.75, -4.5])
    );
    let double = floats(&[2], &[3.75, -4.5]);
    assert_eq!(double.cast(ArrayDType::Int32).unwrap(), small);
    assert_eq!(double.cast(ArrayDType::Int64).unwrap(), wide);
    assert_eq!(double.cast(ArrayDType::Float32).unwrap(), single);
    // A cast to the Array's own dtype shares the buffer.
    let same = double.cast(ArrayDType::Float64).unwrap();
    assert!(same.shares_buffer_with(&double));
    // A view converts in its own order.
    assert_eq!(
        reversed(&double).cast(ArrayDType::Int64).unwrap(),
        ints(&[2], &[-4, 3])
    );

    assert_eq!(
        message(ints(&[2], &[1, 1 << 40]).cast(ArrayDType::Int32)),
        "AU4002 integer value `1099511627776` does not fit in `int32` at flat index 1"
    );
    assert_eq!(
        message(ints(&[1], &[(1 << 53) + 1]).cast(ArrayDType::Float64)),
        "AU4002 integer value `9007199254740993` cannot be represented exactly as `float64` at flat index 0"
    );
    assert_eq!(
        message(ints(&[1], &[(1 << 24) + 1]).cast(ArrayDType::Float32)),
        "AU4002 integer value `16777217` cannot be represented exactly as `float32` at flat index 0"
    );
    assert_eq!(
        message(shorts(&[1], &[(1 << 24) + 1]).cast(ArrayDType::Float32)),
        "AU4002 integer value `16777217` cannot be represented exactly as `float32` at flat index 0"
    );
    assert_eq!(
        message(floats(&[2], &[0.0, f64::NAN]).cast(ArrayDType::Int64)),
        "AU2999 cannot cast non-finite float to `int64` at flat index 1"
    );
    assert!(message(floats(&[1], &[1e10]).cast(ArrayDType::Int32)).ends_with(" at flat index 0"));
    assert!(message(floats(&[1], &[1e19]).cast(ArrayDType::Int64)).ends_with(" at flat index 0"));
    assert!(
        message(singles(&[1], &[f32::INFINITY]).cast(ArrayDType::Int32))
            .ends_with(" at flat index 0")
    );
    assert!(message(singles(&[1], &[1e30]).cast(ArrayDType::Int64)).ends_with(" at flat index 0"));
    assert_eq!(
        message(mask(&[1], &[true]).cast(ArrayDType::Int64)),
        "AU4007 array casts are only supported between numeric dtypes, found bool and int64"
    );
    assert_eq!(
        message(wide.cast(ArrayDType::Bool)),
        "AU4007 array casts are only supported between numeric dtypes, found int64 and bool"
    );
}

#[test]
fn masks_are_arrays_without_arithmetic() {
    use crate::runtime_value::{ArrayBinaryOp, ArrayReduction, IntegerArithmeticMode};
    let mut flags = ArrayValue::zeros(ArrayDType::Bool, Box::new([2, 2])).unwrap();
    assert_eq!(flags.dtype().runtime_type_name(), "bool");
    assert!(!flags.dtype().is_numeric());
    assert_eq!(ArrayDType::from_code(4), Some(ArrayDType::Bool));
    flags.set(&[0, 1], Value::Bool(true)).unwrap();
    assert_eq!(flags.get(&[0, 1]).unwrap(), Value::Bool(true));
    assert!(flags.set(&[0, 0], int(1)).is_err());
    assert_eq!(
        flags.render(),
        "Array[bool](shape=[2, 2], values=[false, true, false, false])"
    );
    assert!(!flags.is_partial_view());
    let mut filled = flags.clone();
    filled.fill(Value::Bool(true)).unwrap();
    assert_eq!(filled, mask(&[2, 2], &[true; 4]));
    let mut own = mask(&[2], &[false, false]);
    own.fill(Value::Bool(true)).unwrap();
    assert_eq!(own, mask(&[2], &[true, true]));
    assert!(own.fill(int(1)).is_err());
    assert_eq!(
        ArrayValue::from_values(
            &aura_core::types::Type::named("bool"),
            Box::new([2]),
            vec![Value::Bool(true), Value::Bool(false)]
        )
        .unwrap(),
        mask(&[2], &[true, false])
    );
    let whole = Slice {
        start: None,
        end: None,
        step: 1,
    };
    let mut target = mask(&[3], &[false; 3]);
    target
        .assign(
            &[Slice {
                start: Some(1),
                end: None,
                step: 1,
            }],
            &Value::Array(mask(&[2], &[true, true])),
        )
        .unwrap();
    assert_eq!(target, mask(&[3], &[false, true, true]));
    target.assign(&[whole], &Value::Bool(false)).unwrap();
    assert_eq!(target, mask(&[3], &[false; 3]));

    let checked = IntegerArithmeticMode::Checked;
    let expected = "AU4007 array arithmetic requires numeric elements, found bool";
    assert_eq!(
        message(flags.binary(&flags, ArrayBinaryOp::Add, checked)),
        expected
    );
    assert_eq!(
        message(flags.scalar_binary(&Value::Bool(true), false, ArrayBinaryOp::Add, checked)),
        expected
    );
    assert_eq!(
        message(reversed(&own).scalar_binary(
            &Value::Bool(true),
            false,
            ArrayBinaryOp::Add,
            checked
        )),
        expected
    );
    let mut temporary = own.clone();
    assert!(temporary
        .binary_reusing(&Value::Bool(true), true, ArrayBinaryOp::Add, checked)
        .is_none_or(|result| result.is_err()));
    assert_eq!(
        message(flags.reduce(ArrayReduction::Sum)),
        "AU4007 array sum requires numeric elements, found bool"
    );
    assert_eq!(
        message(flags.reduce_axis(ArrayReduction::Max, 0)),
        "AU4007 array max requires numeric elements, found bool"
    );
    assert_eq!(
        message(own.dot(&own)),
        "AU4007 array dot requires numeric elements, found bool"
    );
    assert_eq!(
        message(flags.matmul(&flags)),
        "AU4007 array matmul requires numeric elements, found bool"
    );
}

#[test]
fn matmul_runs_one_product_per_batch_position() {
    let stack = ints(&[2, 2, 3], &(0..12).collect::<Vec<_>>());
    // One matrix serves every batch position.
    let shared = ints(&[3, 2], &(0..6).collect::<Vec<_>>());
    assert_eq!(
        stack.matmul(&shared).unwrap(),
        ints(&[2, 2, 2], &[10, 13, 28, 40, 46, 67, 64, 94])
    );
    let paired = ints(&[2, 3, 2], &(0..12).collect::<Vec<_>>());
    assert_eq!(
        stack.matmul(&paired).unwrap(),
        ints(&[2, 2, 2], &[10, 13, 28, 40, 172, 193, 244, 274])
    );
    // A vector is one column on the right and one row on the left.
    assert_eq!(
        stack.matmul(&ints(&[3], &[1, 2, 3])).unwrap(),
        ints(&[2, 2], &[8, 26, 44, 62])
    );
    assert_eq!(
        ints(&[2], &[1, 2]).matmul(&stack).unwrap(),
        ints(&[2, 3], &[6, 9, 12, 24, 27, 30])
    );
    // Batch dimensions stretch against each other.
    let rows = ints(&[3, 1, 1, 2], &(0..6).collect::<Vec<_>>());
    let squares = ints(&[1, 2, 2, 2], &(0..8).collect::<Vec<_>>());
    assert_eq!(
        rows.matmul(&squares).unwrap(),
        ints(&[3, 2, 1, 2], &[2, 3, 6, 7, 6, 11, 26, 31, 10, 19, 46, 55])
    );
    // Float kernels take the same path, and a view supplies its elements
    // in logical order.
    let floats_stack = floats(&[2, 2, 2], &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    let identity = floats(&[2, 2], &[1.0, 0.0, 0.0, 1.0]);
    assert_eq!(floats_stack.matmul(&identity).unwrap(), floats_stack);
    assert_eq!(
        reversed(&floats_stack).matmul(&identity).unwrap(),
        floats(&[2, 2, 2], &[5.0, 6.0, 7.0, 8.0, 1.0, 2.0, 3.0, 4.0])
    );
    assert_eq!(
        floats_stack.matmul(&floats(&[2], &[1.0, 1.0])).unwrap(),
        floats(&[2, 2], &[3.0, 7.0, 11.0, 15.0])
    );
    let single_stack = singles(&[2, 1, 2], &[1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        single_stack.matmul(&singles(&[2, 1], &[2.0, 3.0])).unwrap(),
        singles(&[2, 1, 1], &[8.0, 18.0])
    );
    assert_eq!(
        shorts(&[2, 1, 2], &[1, 2, 3, 4])
            .matmul(&shorts(&[2, 1], &[2, 3]))
            .unwrap(),
        shorts(&[2, 1, 1], &[8, 18])
    );
    // An empty batch gives an empty result of the product's shape.
    let empty = ints(&[0, 2, 3], &[]);
    assert_eq!(empty.matmul(&shared).unwrap(), ints(&[0, 2, 2], &[]));
}

#[test]
fn matmul_reports_batch_mismatches_and_overflow_positions() {
    let stack = ints(&[2, 2, 3], &(0..12).collect::<Vec<_>>());
    for other in [
        ints(&[3, 3, 2], &[0; 18]),
        ints(&[2, 2, 2], &[0; 8]),
        ints(&[2], &[0; 2]),
    ] {
        let error = stack.matmul(&other).unwrap_err();
        assert_eq!(error.code, "AU4007");
        assert!(
            error
                .message
                .starts_with("array matmul requires shapes [.., n, k] and [.., k, m]"),
            "{}",
            error.message
        );
    }
    let vector = ints(&[3], &[1, 2, 3]);
    assert_eq!(vector.matmul(&vector).unwrap_err().code, "AU4007");
    // The failing element is counted in the whole result.
    let large = ints(&[2, 1, 1], &[1, i64::MAX]);
    let error = large.matmul(&ints(&[1, 2], &[1, 2])).unwrap_err();
    assert_eq!(error.code, "AU4002");
    assert_eq!(error.message, "array matmul overflowed at flat index 3");
}
