use super::*;

fn host(name: &str, args: Vec<Value>) -> Result<Value> {
    host_builtin_function(name).unwrap_or_else(|| panic!("`{name}` is a builtin"))(args, None)
}

fn float(name: &str, args: &[f64]) -> f64 {
    let args = args.iter().map(|value| Value::Float(*value)).collect();
    match host(name, args).unwrap_or_else(|error| panic!("{name}: {}", error.message)) {
        Value::Float(value) => value,
        other => panic!("`{name}` returned {other:?}"),
    }
}

fn float_error(name: &str, args: &[f64]) -> String {
    let args = args.iter().map(|value| Value::Float(*value)).collect();
    host(name, args).expect_err(name).code
}

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn integer(name: &str, args: &[i64]) -> Result<Value> {
    host(name, args.iter().map(|value| int(*value)).collect())
}

fn near(value: f64, expected: f64) {
    assert!(
        (value - expected).abs() <= 1e-12 * expected.abs().max(1.0),
        "{value} is not near {expected}"
    );
}

const UNARY: [&str; 15] = [
    "sqrt", "cbrt", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh",
    "expm1", "log1p", "degrees", "radians",
];

#[test]
fn unary_functions_return_the_binary64_value() {
    use std::f64::consts::PI;
    for (name, input, expected) in [
        ("sqrt", 2.0, std::f64::consts::SQRT_2),
        ("cbrt", -27.0, -3.0),
        ("asin", 1.0, PI / 2.0),
        ("acos", -1.0, PI),
        ("atan", 1.0, PI / 4.0),
        ("sinh", 1.0, 1.1752011936438014),
        ("cosh", 1.0, 1.5430806348152437),
        ("tanh", 0.5, 0.46211715726000974),
        ("asinh", 1.0, 0.881373587019543),
        ("acosh", 2.0, 1.3169578969248166),
        ("atanh", 0.5, 0.5493061443340548),
        ("expm1", 1e-6, 1.0000005000001665e-6),
        ("log1p", 1e-6, 9.999995000003334e-7),
        ("degrees", PI, 180.0),
        ("radians", 180.0, PI),
    ] {
        near(float(&format!("math::{name}"), &[input]), expected);
    }
    assert_eq!(float("math::sqrt", &[-0.0]).to_bits(), (-0.0f64).to_bits());
    assert_eq!(float("math::sqrt", &[f64::INFINITY]), f64::INFINITY);
    assert_eq!(float("math::tanh", &[f64::NEG_INFINITY]), -1.0);
    assert_eq!(
        float("math::atan", &[f64::INFINITY]),
        std::f64::consts::FRAC_PI_2
    );
    assert_eq!(float("math::expm1", &[f64::NEG_INFINITY]), -1.0);
}

#[test]
fn unary_functions_pass_nan_through_and_reject_values_outside_the_domain() {
    for name in UNARY {
        let name = format!("math::{name}");
        assert!(float(&name, &[f64::NAN]).is_nan(), "{name}");
    }
    for (name, input) in [
        ("sqrt", -1.0),
        ("sqrt", f64::NEG_INFINITY),
        ("asin", 1.5),
        ("asin", -1.5),
        ("acos", 2.0),
        ("acos", f64::INFINITY),
        ("acosh", 0.5),
        ("atanh", 1.0),
        ("atanh", -1.0),
        ("atanh", 3.0),
        ("log1p", -1.0),
        ("log1p", -2.0),
    ] {
        assert_eq!(
            float_error(&format!("math::{name}"), &[input]),
            "AU4001",
            "{name}({input})"
        );
    }
    for (name, input) in [
        ("sinh", 1000.0),
        ("cosh", 1000.0),
        ("expm1", 1000.0),
        ("degrees", f64::MAX),
    ] {
        assert_eq!(
            float_error(&format!("math::{name}"), &[input]),
            "AU4002",
            "{name}({input})"
        );
    }
}

#[test]
fn binary_functions_follow_ieee_754() {
    use std::f64::consts::PI;
    near(float("math::atan2", &[1.0, -1.0]), 3.0 * PI / 4.0);
    near(float("math::atan2", &[-0.0, -1.0]), -PI);
    assert_eq!(float("math::atan2", &[0.0, 0.0]), 0.0);
    near(float("math::hypot", &[3.0, 4.0]), 5.0);
    assert_eq!(
        float("math::hypot", &[f64::INFINITY, f64::NAN]),
        f64::INFINITY
    );
    near(
        float("math::hypot", &[1e200, 1e200]),
        1e200 * std::f64::consts::SQRT_2,
    );
    assert_eq!(float_error("math::hypot", &[f64::MAX, f64::MAX]), "AU4002");
    assert_eq!(float("math::copysign", &[2.5, -0.0]), -2.5);
    assert_eq!(float("math::copysign", &[-2.5, 1.0]), 2.5);
    assert!(float("math::copysign", &[f64::NAN, -1.0]).is_nan());
    assert!(float("math::atan2", &[f64::NAN, 1.0]).is_nan());
}

#[test]
fn predicates_classify_values() {
    for (name, nan, infinite, finite) in [
        ("math::is_nan", true, false, false),
        ("math::is_inf", false, true, false),
        ("math::is_finite", false, false, true),
    ] {
        for (input, expected) in [
            (f64::NAN, nan),
            (f64::NEG_INFINITY, infinite),
            (1.5, finite),
        ] {
            assert_eq!(
                host(name, vec![Value::Float(input)]).expect(name),
                Value::Bool(expected),
                "{name}({input})"
            );
        }
    }
}

#[test]
fn integer_functions_are_exact_and_report_overflow() {
    for (name, args, expected) in [
        ("math::gcd", vec![84, -36], 12),
        ("math::gcd", vec![0, 0], 0),
        ("math::gcd", vec![0, -5], 5),
        ("math::gcd", vec![i64::MIN, 6], 2),
        ("math::lcm", vec![4, 6], 12),
        ("math::lcm", vec![-4, 6], 12),
        ("math::lcm", vec![0, 6], 0),
        ("math::isqrt", vec![0], 0),
        ("math::isqrt", vec![99], 9),
        ("math::isqrt", vec![100], 10),
        ("math::isqrt", vec![i64::MAX], 3_037_000_499),
        ("math::factorial", vec![0], 1),
        ("math::factorial", vec![20], 2_432_902_008_176_640_000),
        ("math::comb", vec![52, 5], 2_598_960),
        ("math::comb", vec![5, 9], 0),
        ("math::comb", vec![66, 33], 7_219_428_434_016_265_740),
        ("math::comb", vec![0, 0], 1),
        ("math::perm", vec![10, 3], 720),
        ("math::perm", vec![3, 10], 0),
        ("math::perm", vec![20, 20], 2_432_902_008_176_640_000),
    ] {
        assert_eq!(
            integer(name, &args).expect(name),
            int(expected),
            "{name}{args:?}"
        );
    }
    for (name, args, code) in [
        ("math::gcd", vec![i64::MIN, 0], "AU4002"),
        ("math::gcd", vec![i64::MIN, i64::MIN], "AU4002"),
        ("math::lcm", vec![i64::MAX, i64::MAX - 1], "AU4002"),
        ("math::isqrt", vec![-1], "AU4001"),
        ("math::factorial", vec![-1], "AU4001"),
        ("math::factorial", vec![21], "AU4002"),
        ("math::comb", vec![-1, 0], "AU4001"),
        ("math::comb", vec![3, -1], "AU4001"),
        ("math::comb", vec![68, 34], "AU4002"),
        ("math::perm", vec![-1, 0], "AU4001"),
        ("math::perm", vec![3, -1], "AU4001"),
        ("math::perm", vec![21, 21], "AU4002"),
    ] {
        assert_eq!(
            integer(name, &args).expect_err(name).code,
            code,
            "{name}{args:?}"
        );
    }
}

#[test]
fn arguments_of_the_wrong_type_or_count_are_rejected() {
    for name in UNARY {
        let name = format!("math::{name}");
        assert!(host(&name, vec![int(1)]).is_err(), "{name}");
        assert!(host(&name, Vec::new()).is_err(), "{name}");
    }
    for name in ["math::atan2", "math::hypot", "math::copysign"] {
        assert!(host(name, vec![Value::Float(1.0)]).is_err(), "{name}");
        assert!(
            host(name, vec![Value::Float(1.0), int(1)]).is_err(),
            "{name}"
        );
    }
    for name in ["math::is_nan", "math::is_inf", "math::is_finite"] {
        assert!(host(name, vec![Value::Bool(true)]).is_err(), "{name}");
    }
    for name in ["math::gcd", "math::lcm", "math::comb", "math::perm"] {
        assert!(host(name, vec![int(1)]).is_err(), "{name}");
        assert!(
            host(name, vec![int(1), Value::Float(1.0)]).is_err(),
            "{name}"
        );
    }
    for name in ["math::isqrt", "math::factorial"] {
        assert!(host(name, vec![Value::Float(1.0)]).is_err(), "{name}");
        assert!(host(name, vec![int(1), int(1)]).is_err(), "{name}");
    }
    // An integer outside `int64` is not an `int64` argument.
    let wide = Value::Int(IntegerValue::from_u64(u64::MAX));
    for (name, args) in [
        ("math::isqrt", vec![wide.clone()]),
        ("math::gcd", vec![int(4), wide]),
    ] {
        let error = host(name, args).expect_err(name);
        assert_eq!(error.code, "AU4001", "{name}");
        assert!(
            error.message.contains("expects `int64`"),
            "{}",
            error.message
        );
    }
}

fn encoding_result(value: Value) -> (String, Vec<Value>) {
    match value {
        Value::EnumVariant(value) => {
            let crate::runtime_value::VariantPayloads::Values(payloads) = value.payloads else {
                panic!("a host result holds plain values");
            };
            (value.variant_name.to_string(), payloads)
        }
        other => panic!("expected a Result, found {other:?}"),
    }
}

#[test]
fn bytes_encode_and_decode_report_typed_errors() {
    let text = |value: &str| Value::string(value);
    let (variant, payload) =
        encoding_result(host("bytes::encode", vec![text("hé"), text("latin-1")]).expect("encode"));
    assert_eq!(variant, "Ok");
    let encoded = payload[0].clone();
    let (variant, payload) =
        encoding_result(host("bytes::decode", vec![encoded, text("LATIN_1")]).expect("decode"));
    assert_eq!(variant, "Ok");
    assert_eq!(payload[0], text("hé"));
    for (name, args) in [
        ("bytes::encode", vec![text("hé"), text("ascii")]),
        ("bytes::encode", vec![text("hé"), text("klingon")]),
        ("bytes::decode", vec![payload_bytes(&[0xff]), text("utf-8")]),
        (
            "bytes::decode",
            vec![payload_bytes(&[0x61]), text("klingon")],
        ),
    ] {
        let (variant, _) = encoding_result(host(name, args).expect(name));
        assert_eq!(variant, "Err", "{name}");
    }
    assert!(host("bytes::encode", vec![text("x")]).is_err());
    assert!(host("bytes::decode", vec![text("x"), text("ascii")]).is_err());
    assert!(host("bytes::encode", vec![int(1), text("ascii")]).is_err());
}

fn payload_bytes(bytes: &[u8]) -> Value {
    // Latin-1 maps each character below U+0100 to the byte of its number.
    let text = bytes
        .iter()
        .map(|byte| char::from(*byte))
        .collect::<String>();
    host(
        "bytes::encode",
        vec![Value::string(&text), Value::string("latin-1")],
    )
    .map(|value| encoding_result(value).1[0].clone())
    .expect("latin-1 encodes")
}
