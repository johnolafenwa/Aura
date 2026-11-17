use super::*;

fn int(value: i64) -> Value {
    int_value(value)
}

fn empty() -> Value {
    no_value()
}

fn text(value: &str) -> Value {
    Value::string(value)
}

/// The `small` and `digits` arguments that carry `value`.
fn big(value: &str) -> [Value; 2] {
    let value: BigInt = value.parse().expect("test integer");
    match value.to_i64() {
        Some(small) => [int(small), empty()],
        None => [int(0), wire(&value, 0)],
    }
}

/// The three arguments that carry `coefficient * 10^exponent`.
fn dec(coefficient: &str, exponent: i64) -> [Value; 3] {
    let [small, digits] = big(coefficient);
    [small, digits, int(exponent)]
}

fn args<const N: usize>(parts: [&[Value]; N]) -> Vec<Value> {
    parts.iter().flat_map(|part| part.iter().cloned()).collect()
}

fn list_words(value: &Value) -> Vec<u64> {
    words("test", value).expect("a word list").into_owned()
}

/// The integer and exponent a wire list holds.
fn unwire(value: &Value) -> (BigInt, i64) {
    let words = list_words(value);
    assert!(words.len() > HEADER, "wire list too short: {words:?}");
    let mut number = BigInt::from_biguint(Sign::Plus, magnitude(&words[HEADER..]));
    if words[1] == 1 {
        number = -number;
    }
    assert_eq!(
        words[0],
        u64::from(number.to_i64().is_none()),
        "the size tag of {number}"
    );
    let exponent = i64::try_from(words[3]).expect("exponent");
    (number, if words[2] == 1 { -exponent } else { exponent })
}

fn big_text(value: Result<Value>) -> String {
    let (number, exponent) = unwire(&value.expect("a result"));
    assert_eq!(exponent, 0);
    number.to_string()
}

fn dec_text(value: Result<Value>) -> String {
    let (number, exponent) = unwire(&value.expect("a result"));
    format!("{number}e{exponent}")
}

fn code(value: Result<Value>) -> String {
    value.expect_err("an error").code
}

fn binary(op: i64, left: &str, right: &str) -> Result<Value> {
    host_big_binary(args([&[int(op)], &big(left), &big(right)]), None)
}

const HUGE: &str = "123456789012345678901234567890123456789";

#[test]
fn big_binary_computes_every_operation() {
    assert_eq!(
        big_text(binary(0, "9223372036854775807", "1")),
        "9223372036854775808"
    );
    assert_eq!(
        big_text(binary(0, "-9223372036854775807", "-1")),
        "-9223372036854775808"
    );
    assert_eq!(big_text(binary(1, HUGE, HUGE)), "0");
    assert_eq!(big_text(binary(2, HUGE, "-1000")), format!("-{HUGE}000"));
    assert_eq!(big_text(binary(3, "-7", "2")), "-4");
    assert_eq!(big_text(binary(4, "-7", "2")), "1");
    assert_eq!(big_text(binary(3, "7", "-2")), "-4");
    assert_eq!(big_text(binary(4, "7", "-2")), "-1");
    assert_eq!(big_text(binary(5, "-48", "18")), "6");
    assert_eq!(
        big_text(binary(6, "2", "100")),
        (BigInt::one() << 100u32).to_string()
    );
    assert_eq!(big_text(binary(6, "0", "0")), "1");
    assert_eq!(big_text(binary(6, "-1", "4294967295")), "-1");
    assert_eq!(
        big_text(binary(7, "3", "70")),
        (BigInt::from(3) << 70u32).to_string()
    );
    assert_eq!(big_text(binary(7, "0", "1000000000000")), "0");
    assert_eq!(big_text(binary(8, "-9", "1")), "-5");
    assert_eq!(big_text(binary(8, HUGE, "1000")), "0");
    assert_eq!(big_text(binary(8, "-5", "1000")), "-1");
    assert_eq!(big_text(binary(9, "-2", "7")), "6");
    assert_eq!(big_text(binary(10, "8", "1")), "9");
    assert_eq!(big_text(binary(11, "-1", "5")), "-6");
    assert_eq!(big_text(binary(12, "4", "-6")), "12");
}

#[test]
fn big_binary_reports_bad_operands() {
    assert_eq!(code(binary(3, "1", "0")), "AU4001");
    assert_eq!(code(binary(4, HUGE, "0")), "AU4001");
    assert_eq!(code(binary(6, "2", "-1")), "AU4001");
    assert_eq!(code(binary(6, "2", HUGE)), "AU4001");
    assert_eq!(code(binary(6, "3", "4294967295")), "AU4005");
    assert_eq!(code(binary(7, "1", "-1")), "AU4001");
    assert_eq!(code(binary(8, "1", "-1")), "AU4001");
    assert_eq!(code(binary(7, "1", "99999999999")), "AU4005");
    assert_eq!(code(binary(99, "1", "1")), "AU4001");
    assert_eq!(code(host_big_binary(vec![int(0)], None)), "AU4001");
    assert_eq!(
        code(host_big_binary(
            vec![text("x"), int(0), empty(), int(0), empty()],
            None
        )),
        "AU4001"
    );
    assert_eq!(
        code(host_big_binary(
            vec![int(0), int(0), int(0), int(0), empty()],
            None
        )),
        "AU4001"
    );
    // A digits list shorter than a wire form.
    let short = word_list(vec![1, 0]);
    assert_eq!(
        code(host_big_binary(
            vec![int(0), int(0), short, int(0), empty()],
            None
        )),
        "AU4001"
    );
}

#[test]
fn word_lists_are_read_from_either_list_form() {
    let general = Value::Vec(VecValue::new(
        Type::named("uint64"),
        ListElements::Values(
            [1u64, 0, 0, 0, 5, 1]
                .iter()
                .map(|word| Value::Int(IntegerValue::from_u64(*word)))
                .collect::<Vec<_>>()
                .into(),
        ),
    ));
    let read = list_words(&general);
    assert_eq!(read, vec![1, 0, 0, 0, 5, 1]);
    let mixed = Value::Vec(VecValue::new(
        Type::named("str"),
        ListElements::Values(vec![text("x")].into()),
    ));
    assert!(words("test", &mixed).is_err());
    assert!(words("test", &int(1)).is_err());
}

#[test]
fn big_compare_orders_values() {
    let compare = |left: &str, right: &str| {
        host_big_compare(args([&big(left), &big(right)]), None).expect("an order")
    };
    assert_eq!(compare("1", HUGE), int(-1));
    assert_eq!(compare(HUGE, HUGE), int(0));
    assert_eq!(compare(HUGE, "-1"), int(1));
    assert_eq!(code(host_big_compare(Vec::new(), None)), "AU4001");
}

#[test]
fn big_pow_mod_follows_the_modulus() {
    let pow_mod = |base: &str, exponent: &str, modulus: &str| {
        host_big_pow_mod(args([&big(base), &big(exponent), &big(modulus)]), None)
    };
    assert_eq!(big_text(pow_mod("7", "560", "561")), "1");
    assert_eq!(big_text(pow_mod("2", "10", "-1000")), "-976");
    assert_eq!(code(pow_mod("2", "-1", "5")), "AU4001");
    assert_eq!(code(pow_mod("2", "1", "0")), "AU4001");
    assert_eq!(code(host_big_pow_mod(Vec::new(), None)), "AU4001");
}

#[test]
fn big_unary_takes_roots_and_factorials() {
    let unary = |op: i64, value: &str| host_big_unary(args([&[int(op)], &big(value)]), None);
    assert_eq!(big_text(unary(0, "99")), "9");
    assert_eq!(big_text(unary(0, "0")), "0");
    assert_eq!(code(unary(0, "-1")), "AU4001");
    assert_eq!(big_text(unary(1, "0")), "1");
    assert_eq!(big_text(unary(1, "20")), "2432902008176640000");
    assert_eq!(
        big_text(unary(1, "30")),
        "265252859812191058636308480000000"
    );
    let direct: BigInt = (1..=200u32).map(BigInt::from).product();
    assert_eq!(big_text(unary(1, "200")), direct.to_string());
    assert_eq!(code(unary(1, "-1")), "AU4001");
    assert_eq!(code(unary(1, "9999999999")), "AU4005");
    assert_eq!(code(unary(2, "1")), "AU4001");
    assert_eq!(code(host_big_unary(Vec::new(), None)), "AU4001");
}

#[test]
fn big_bits_counts_the_magnitude() {
    let bits = |value: &str| host_big_bits(args([&big(value)]), None).expect("bits");
    assert_eq!(bits("0"), int(0));
    assert_eq!(bits("-255"), int(8));
    assert_eq!(bits(HUGE), int(127));
    assert_eq!(code(host_big_bits(Vec::new(), None)), "AU4001");
}

#[test]
fn big_parse_reads_signs_and_radixes() {
    let parse = |value: &str, radix: i64| host_big_parse(vec![text(value), int(radix)], None);
    assert_eq!(big_text(parse("+42", 10)), "42");
    assert_eq!(big_text(parse("-ff", 16)), "-255");
    assert_eq!(big_text(parse("Zz", 36)), "1295");
    assert_eq!(big_text(parse(HUGE, 10)), HUGE);
    for bad in ["", "-", "+", "1_000", "12a", " 1", "1 ", "--1", "é"] {
        assert!(
            list_words(&parse(bad, 10).expect("a list")).is_empty(),
            "{bad:?}"
        );
    }
    assert!(list_words(&parse("2", 2).expect("a list")).is_empty());
    assert_eq!(code(parse("1", 1)), "AU4001");
    assert_eq!(code(parse("1", 37)), "AU4001");
    assert_eq!(code(host_big_parse(vec![int(1), int(10)], None)), "AU4001");
    assert_eq!(code(host_big_parse(Vec::new(), None)), "AU4001");
}

#[test]
fn big_format_writes_any_radix() {
    let format =
        |value: &str, radix: i64| host_big_format(args([&big(value), &[int(radix)]]), None);
    assert_eq!(format(HUGE, 10).expect("text"), text(HUGE));
    assert_eq!(format("-255", 16).expect("text"), text("-ff"));
    assert_eq!(format("5", 2).expect("text"), text("101"));
    assert_eq!(code(format("5", 0)), "AU4001");
    assert_eq!(code(host_big_format(Vec::new(), None)), "AU4001");
}

#[test]
fn big_float_conversions_round_and_truncate() {
    let to_float = |value: &str| host_big_to_float(args([&big(value)]), None).expect("a float");
    assert_eq!(to_float("-3"), Value::Float(-3.0));
    assert_eq!(to_float(HUGE), Value::Float(1.2345678901234568e38));
    let enormous = format!("1{}", "0".repeat(400));
    assert_eq!(to_float(&enormous), Value::Float(f64::INFINITY));
    assert_eq!(code(host_big_to_float(Vec::new(), None)), "AU4001");

    let from_float = |value: f64| host_big_from_float(vec![Value::Float(value)], None);
    assert_eq!(big_text(from_float(-2.9)), "-2");
    assert_eq!(big_text(from_float(0.5)), "0");
    assert_eq!(
        big_text(from_float(1e30)),
        "1000000000000000019884624838656"
    );
    assert_eq!(big_text(from_float(5e-324)), "0");
    assert!(list_words(&from_float(f64::NAN).expect("a list")).is_empty());
    assert!(list_words(&from_float(f64::NEG_INFINITY).expect("a list")).is_empty());
    assert_eq!(code(host_big_from_float(vec![int(1)], None)), "AU4001");
    assert_eq!(code(host_big_from_float(Vec::new(), None)), "AU4001");
}

fn dec_binary(
    op: i64,
    left: (&str, i64),
    right: (&str, i64),
    digits: i64,
    mode: i64,
) -> Result<Value> {
    host_dec_binary(
        args([
            &[int(op)],
            &dec(left.0, left.1),
            &dec(right.0, right.1),
            &[int(digits), int(mode)],
        ]),
        None,
    )
}

#[test]
fn decimal_arithmetic_is_exact_and_normalized() {
    assert_eq!(dec_text(dec_binary(0, ("1", -1), ("2", -1), 0, 0)), "3e-1");
    assert_eq!(dec_text(dec_binary(0, ("5", -1), ("5", -1), 0, 0)), "1e0");
    assert_eq!(
        dec_text(dec_binary(0, ("1", 30), ("1", -30), 0, 0)),
        format!("1{}1e-30", "0".repeat(59))
    );
    assert_eq!(dec_text(dec_binary(0, ("0", 0), ("7", 5), 0, 0)), "7e5");
    assert_eq!(dec_text(dec_binary(0, ("7", 5), ("0", 0), 0, 0)), "7e5");
    assert_eq!(dec_text(dec_binary(1, ("1", 0), ("1", -3), 0, 0)), "999e-3");
    assert_eq!(dec_text(dec_binary(1, ("15", -1), ("15", -1), 0, 0)), "0e0");
    assert_eq!(dec_text(dec_binary(2, ("15", -1), ("2", 0), 0, 0)), "3e0");
    assert_eq!(
        dec_text(dec_binary(2, (HUGE, -10), ("-1", 4), 0, 0)),
        format!("-{HUGE}e-6")
    );
    assert_eq!(
        dec_text(dec_binary(4, ("11", -1), ("10", 0), 0, 0)),
        "25937424601e-10"
    );
    assert_eq!(dec_text(dec_binary(4, ("0", 0), ("0", 0), 0, 0)), "1e0");
    assert_eq!(code(dec_binary(4, ("2", 0), ("-1", 0), 0, 0)), "AU4001");
    assert_eq!(code(dec_binary(4, ("2", 0), ("5", -1), 0, 0)), "AU4001");
    assert_eq!(
        code(dec_binary(4, ("3", 0), ("4294967295", 0), 0, 0)),
        "AU4005"
    );
    assert_eq!(code(dec_binary(9, ("2", 0), ("1", 0), 0, 0)), "AU4001");
    assert_eq!(
        code(dec_binary(0, ("1", 500_000_000), ("1", 0), 0, 0)),
        "AU4005"
    );
    assert_eq!(
        code(dec_binary(0, ("1", i64::MAX), ("1", i64::MIN), 0, 0)),
        "AU4005"
    );
    assert_eq!(
        code(dec_binary(2, ("1", i64::MAX), ("1", i64::MAX), 0, 0)),
        "AU4002"
    );
    assert_eq!(
        code(dec_binary(2, ("1", i64::MAX), ("1", 1), 0, 0)),
        "AU4002"
    );
    assert_eq!(code(host_dec_binary(Vec::new(), None)), "AU4001");
}

#[test]
fn decimal_division_rounds_to_the_requested_digits() {
    assert_eq!(
        dec_text(dec_binary(3, ("1", 0), ("3", 0), 5, 0)),
        "33333e-5"
    );
    assert_eq!(
        dec_text(dec_binary(3, ("2", 0), ("3", 0), 5, 0)),
        "66667e-5"
    );
    assert_eq!(
        dec_text(dec_binary(3, ("-2", 0), ("3", 0), 5, 3)),
        "-66666e-5"
    );
    assert_eq!(dec_text(dec_binary(3, ("10", 0), ("4", 0), 28, 0)), "25e-1");
    assert_eq!(dec_text(dec_binary(3, ("1", 0), ("8", 0), 2, 0)), "12e-2");
    assert_eq!(dec_text(dec_binary(3, ("1", 0), ("8", 0), 2, 1)), "13e-2");
    assert_eq!(dec_text(dec_binary(3, ("3", 0), ("8", 0), 2, 0)), "38e-2");
    assert_eq!(dec_text(dec_binary(3, ("999", 0), ("1", 0), 2, 0)), "1e3");
    assert_eq!(
        dec_text(dec_binary(3, ("1", 100), ("7", -100), 3, 0)),
        "143e197"
    );
    assert_eq!(dec_text(dec_binary(3, ("0", 0), ("7", 0), 3, 0)), "0e0");
    assert_eq!(dec_text(dec_binary(3, (HUGE, 0), ("3", 0), 1, 0)), "4e37");
    assert_eq!(code(dec_binary(3, ("1", 0), ("0", 0), 5, 0)), "AU4001");
    assert_eq!(code(dec_binary(3, ("1", 0), ("3", 0), 0, 0)), "AU4001");
    assert_eq!(
        code(dec_binary(3, ("1", 0), ("3", 0), 1_000_001, 0)),
        "AU4001"
    );
    assert_eq!(code(dec_binary(3, ("1", 0), ("3", 0), 5, 7)), "AU4001");
}

#[test]
fn every_rounding_mode_picks_its_side() {
    let round = |value: (&str, i64), places: i64, mode: i64| {
        dec_text(host_dec_round(
            args([&dec(value.0, value.1), &[int(places), int(mode)]]),
            None,
        ))
    };
    // Rows: half even, half up, half down, down, up, floor, ceiling.
    let cases: [(&str, [&str; 7]); 6] = [
        ("25", ["2e0", "3e0", "2e0", "2e0", "3e0", "2e0", "3e0"]),
        ("35", ["4e0", "4e0", "3e0", "3e0", "4e0", "3e0", "4e0"]),
        (
            "-25",
            ["-2e0", "-3e0", "-2e0", "-2e0", "-3e0", "-3e0", "-2e0"],
        ),
        ("26", ["3e0", "3e0", "3e0", "2e0", "3e0", "2e0", "3e0"]),
        (
            "-21",
            ["-2e0", "-2e0", "-2e0", "-2e0", "-3e0", "-3e0", "-2e0"],
        ),
        ("20", ["2e0", "2e0", "2e0", "2e0", "2e0", "2e0", "2e0"]),
    ];
    for (coefficient, expected) in cases {
        for (mode, want) in expected.iter().enumerate() {
            assert_eq!(
                round((coefficient, -1), 0, mode as i64),
                *want,
                "{coefficient}e-1 mode {mode}"
            );
        }
    }
    assert_eq!(round(("123456", -3), 2, 0), "12346e-2");
    assert_eq!(round(("123456", -3), -2, 0), "1e2");
    assert_eq!(round(("123456", -3), 5, 0), "123456e-3");
    assert_eq!(round(("1", -40), 2, 0), "0e0");
    assert_eq!(round(("1", -40), 2, 4), "1e-2");
    assert_eq!(round(("-1", -40), 2, 5), "-1e-2");
    assert_eq!(round(("-1", -40), 2, 6), "0e0");
    assert_eq!(round(("0", 0), 2, 0), "0e0");
    assert_eq!(round(("5", -1), 0, 0), "0e0");
    assert_eq!(round(("5", -3), 0, 1), "0e0");
    assert_eq!(
        code(host_dec_round(
            args([&dec("1", -5), &[int(2), int(9)]]),
            None
        )),
        "AU4001"
    );
    assert_eq!(
        code(host_dec_round(
            args([&dec("1", -5), &[int(i64::MIN), int(0)]]),
            None
        )),
        "AU4002"
    );
    assert_eq!(code(host_dec_round(Vec::new(), None)), "AU4001");
}

#[test]
fn decimal_compare_orders_values() {
    let compare = |left: (&str, i64), right: (&str, i64)| {
        host_dec_compare(args([&dec(left.0, left.1), &dec(right.0, right.1)]), None)
            .expect("an order")
    };
    assert_eq!(compare(("1", 0), ("1", 0)), int(0));
    assert_eq!(compare(("0", 0), ("0", 0)), int(0));
    assert_eq!(compare(("-1", 0), ("1", -5)), int(-1));
    assert_eq!(compare(("0", 0), ("-1", 9)), int(1));
    assert_eq!(compare(("15", -1), ("2", 0)), int(-1));
    assert_eq!(compare(("25", -1), ("2", 0)), int(1));
    assert_eq!(compare(("1", 9_000_000_000), ("9", 0)), int(1));
    assert_eq!(compare(("-1", 9_000_000_000), ("-9", 0)), int(-1));
    assert_eq!(compare(("-15", -1), ("-2", 0)), int(1));
    assert_eq!(compare((HUGE, -38), ("1235", -3)), int(-1));
    assert_eq!(code(host_dec_compare(Vec::new(), None)), "AU4001");
}

#[test]
fn decimal_to_integer_and_normal_form() {
    let integer = |value: (&str, i64), mode: i64| {
        big_text(host_dec_integer(
            args([&dec(value.0, value.1), &[int(mode)]]),
            None,
        ))
    };
    assert_eq!(integer(("-25", -1), 5), "-3");
    assert_eq!(integer(("-25", -1), 3), "-2");
    assert_eq!(integer(("25", -1), 6), "3");
    assert_eq!(integer(("12", 3), 0), "12000");
    assert_eq!(code(host_dec_integer(Vec::new(), None)), "AU4001");

    let normal = |value: (&str, i64)| host_dec_normalize(args([&dec(value.0, value.1)]), None);
    assert_eq!(dec_text(normal(("1200", -2))), "12e0");
    assert_eq!(dec_text(normal(("0", 77))), "0e0");
    assert_eq!(dec_text(normal(("1024", 0))), "1024e0");
    assert_eq!(code(normal(("10", i64::MAX))), "AU4002");
    assert_eq!(code(normal(("1", i64::MAX))), "AU4002");
    assert_eq!(code(host_dec_normalize(Vec::new(), None)), "AU4001");
}

#[test]
fn decimal_parse_reads_points_and_exponents() {
    let parse = |value: &str| host_dec_parse(vec![text(value)], None);
    assert_eq!(dec_text(parse("1.50")), "15e-1");
    assert_eq!(dec_text(parse("-.5")), "-5e-1");
    assert_eq!(dec_text(parse("+5.")), "5e0");
    assert_eq!(dec_text(parse("1e3")), "1e3");
    assert_eq!(dec_text(parse("12.5E-3")), "125e-4");
    assert_eq!(dec_text(parse("1e+2")), "1e2");
    assert_eq!(dec_text(parse("000")), "0e0");
    for bad in [
        "",
        ".",
        "-",
        "e5",
        "1e",
        "1e+",
        "1e5.0",
        "1.2.3",
        "1,5",
        " 1",
        "NaN",
        "inf",
        "1e99999999999999999999",
        "0x10",
        "1_0",
    ] {
        assert!(
            list_words(&parse(bad).expect("a list")).is_empty(),
            "{bad:?}"
        );
    }
    assert_eq!(code(host_dec_parse(vec![int(1)], None)), "AU4001");
    assert_eq!(code(host_dec_parse(Vec::new(), None)), "AU4001");
}

#[test]
fn decimal_text_forms() {
    let format = |value: (&str, i64), places: i64| {
        host_dec_format(args([&dec(value.0, value.1), &[int(places)]]), None)
    };
    let shown = |value: (&str, i64), places: i64| format(value, places).expect("text");
    assert_eq!(shown(("15", -1), -1), text("1.5"));
    assert_eq!(shown(("-15", -3), -1), text("-0.015"));
    assert_eq!(shown(("12", 3), -1), text("12000"));
    assert_eq!(shown(("0", 0), -1), text("0"));
    assert_eq!(shown(("1", 21), -1), text("1e21"));
    assert_eq!(shown(("1", 20), -1), text("100000000000000000000"));
    assert_eq!(shown(("-123", 30), -1), text("-1.23e32"));
    assert_eq!(shown(("1", -7), -1), text("0.0000001"));
    assert_eq!(shown(("12", -9), -1), text("1.2e-8"));
    assert_eq!(shown(("12", -9), -2), text("0.000000012"));
    assert_eq!(shown(("1", 21), -2), text("1000000000000000000000"));
    assert_eq!(shown(("12345", -1), 3), text("1234.500"));
    assert_eq!(shown(("12345", -1), 0), text("1234"));
    assert_eq!(shown(("12355", -1), 0), text("1236"));
    assert_eq!(shown(("12", 2), 2), text("1200.00"));
    assert_eq!(shown(("-5", -3), 2), text("-0.00"));
    assert_eq!(shown(("0", 0), 2), text("0.00"));
    assert_eq!(shown(("5", -1), 0), text("0"));
    assert_eq!(code(format(("1", 0), -3)), "AU4001");
    assert_eq!(code(format(("1", 500_000_000), -2)), "AU4005");
    assert_eq!(code(format(("1", -500_000_000), -2)), "AU4005");
    assert_eq!(code(format(("1", 500_000_000), 2)), "AU4005");
    assert_eq!(code(host_dec_format(Vec::new(), None)), "AU4001");
}

#[test]
fn decimal_float_conversions() {
    let to_float = |value: (&str, i64)| {
        host_dec_to_float(args([&dec(value.0, value.1)]), None).expect("a float")
    };
    assert_eq!(to_float(("314159", -5)), Value::Float(3.14159));
    assert_eq!(to_float(("-1", 400)), Value::Float(f64::NEG_INFINITY));
    assert_eq!(to_float(("1", -400)), Value::Float(0.0));
    assert_eq!(code(host_dec_to_float(Vec::new(), None)), "AU4001");

    let from_float = |value: f64| host_dec_from_float(vec![Value::Float(value)], None);
    assert_eq!(dec_text(from_float(0.5)), "5e-1");
    assert_eq!(dec_text(from_float(-3.0)), "-3e0");
    assert_eq!(dec_text(from_float(1e22)), "1e22");
    assert_eq!(
        dec_text(from_float(0.1)),
        "1000000000000000055511151231257827021181583404541015625e-55"
    );
    assert_eq!(dec_text(from_float(5e-324)).len(), "e-1074".len() + 751);
    assert!(list_words(&from_float(f64::INFINITY).expect("a list")).is_empty());
    assert!(list_words(&from_float(f64::NAN).expect("a list")).is_empty());
    assert_eq!(code(host_dec_from_float(Vec::new(), None)), "AU4001");
}

#[test]
fn decimal_square_roots() {
    let sqrt = |value: (&str, i64), digits: i64| {
        host_dec_sqrt(args([&dec(value.0, value.1), &[int(digits)]]), None)
    };
    assert_eq!(dec_text(sqrt(("4", 0), 10)), "2e0");
    assert_eq!(dec_text(sqrt(("2", 0), 10)), "1414213562e-9");
    assert_eq!(dec_text(sqrt(("2", -1), 5)), "44721e-5");
    assert_eq!(dec_text(sqrt(("25", -2), 5)), "5e-1");
    assert_eq!(dec_text(sqrt(("0", 0), 5)), "0e0");
    assert_eq!(dec_text(sqrt(("1", 101), 3)), "316e48");
    assert_eq!(dec_text(sqrt((HUGE, 0), 4)), "1111e16");
    assert_eq!(code(sqrt(("-1", 0), 5)), "AU4001");
    assert_eq!(code(sqrt(("1", 0), 0)), "AU4001");
    assert_eq!(code(host_dec_sqrt(Vec::new(), None)), "AU4001");
}

#[test]
fn every_host_function_has_a_name() {
    for name in [
        "big_binary",
        "big_add_words",
        "small_pow_mod",
        "big_compare",
        "big_pow_mod",
        "big_unary",
        "big_bits",
        "big_parse",
        "big_format",
        "big_to_float",
        "big_from_float",
        "dec_binary",
        "dec_compare",
        "dec_round",
        "dec_integer",
        "dec_normalize",
        "dec_parse",
        "dec_format",
        "dec_to_float",
        "dec_from_float",
        "dec_sqrt",
    ] {
        let function = numbers_host_function(&format!("numbers::__aura_{name}"))
            .unwrap_or_else(|| panic!("{name} is registered"));
        // No arguments is never a valid call.
        assert_eq!(code(function(Vec::new(), None)), "AU4001", "{name}");
    }
    assert!(numbers_host_function("numbers::__aura_missing").is_none());
}

#[test]
fn digit_counts_and_powers() {
    assert_eq!(decimal_digits(&BigInt::zero()), 1);
    assert_eq!(decimal_digits(&BigInt::from(-999)), 3);
    assert_eq!(decimal_digits(&BigInt::from(u64::MAX)), 20);
    assert_eq!(decimal_digits(&HUGE.parse().expect("an integer")), 39);
    assert_eq!(
        code(power_of_ten(MAX_SCALE_DIGITS + 1).map(|_| int(0))),
        "AU4005"
    );
    assert_eq!(factorial(1), BigInt::one());
    assert!(exact_float(f64::NAN).is_none());
    let (coefficient, exponent) = exact_float(-0.75).expect("a finite float");
    assert_eq!(exponent, -53);
    assert_eq!(dec_text(decimal_wire(coefficient, exponent)), "-75e-2");
}

#[test]
fn small_modular_powers_use_floor_remainders_at_both_word_limits() {
    for base in [i64::MIN, -19, -1, 0, 1, 17, i64::MAX] {
        for exponent in [0, 1, 2, 31, 65537, i64::MAX] {
            for modulus in [i64::MIN, -23, -1, 1, 13, i64::MAX] {
                let expected =
                    BigInt::from(base).modpow(&BigInt::from(exponent), &BigInt::from(modulus));
                assert_eq!(
                    small_pow_mod(base, exponent, modulus).unwrap(),
                    expected.to_i64().unwrap()
                );
            }
        }
    }
    assert!(small_pow_mod(1, -1, 3).is_err());
    assert!(small_pow_mod(1, 4, 0).is_err());
    assert_eq!(
        factorial(1000),
        (1..=1000_u32).map(BigInt::from).product::<BigInt>()
    );
}

#[test]
fn adding_magnitudes_propagates_carries_and_cancels_opposite_signs() {
    for (a, b) in [
        (
            "340282366920938463463374607431768211455",
            "18446744073709551617",
        ),
        (
            "-340282366920938463463374607431768211455",
            "-18446744073709551617",
        ),
        (
            "340282366920938463463374607431768211455",
            "-340282366920938463463374607431768211455",
        ),
        (
            "18446744073709551617",
            "-340282366920938463463374607431768211455",
        ),
    ] {
        let a: BigInt = a.parse().unwrap();
        let b: BigInt = b.parse().unwrap();
        assert_eq!(
            unwire(&add_big_words(&wire(&a, 0), &wire(&b, 0)).unwrap()).0,
            a + b
        );
    }
    assert!(add_big_words(&empty(), &empty()).is_err());
}
