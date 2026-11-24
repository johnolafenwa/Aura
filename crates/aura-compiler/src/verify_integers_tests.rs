use super::*;

/// Verifies `source` with the solver on this machine. Without one the test
/// is skipped, except under CI, which installs a pinned Z3.
fn verify(source: &str) -> Option<Report> {
    let options = Options::default();
    if smt::SolverCommand::find(options.solver.as_deref()).is_err() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must install an SMT solver for the verifier tests"
        );
        eprintln!("skipping: no SMT solver on PATH");
        return None;
    }
    let program = crate::check_source(source).expect("the program checks");
    let module = crate::mir::lower_for_verification(&program);
    Some(verify_module(&program, &module, &options).expect("the verifier runs"))
}

/// Each obligation that was not proved, as `line kind status`.
fn open_checks(report: &Report) -> Vec<String> {
    report
        .obligations
        .iter()
        .filter(|obligation| obligation.status != Status::Proved)
        .map(|obligation| {
            let status = match &obligation.status {
                Status::Proved => "proved",
                Status::Failed { .. } => "failed",
                Status::Unknown => "unknown",
                Status::Unsupported(_) => "unsupported",
            };
            format!(
                "{} {} {status}",
                obligation.span.line,
                obligation.kind.name()
            )
        })
        .collect()
}

/// Verifies a program that must prove completely, and runs it to show the
/// interpreter agrees that no check fails.
fn assert_proved(source: &str) {
    let Some(report) = verify(source) else {
        return;
    };
    assert_eq!(open_checks(&report), Vec::<String>::new(), "{report:#?}");
    assert!(report.is_proved());
    assert!(!report.obligations.is_empty());
    crate::run_source(source).expect("a proved program runs without a failed check");
}

fn assert_open(source: &str, expected: &[&str]) {
    if let Some(report) = verify(source) {
        assert_eq!(open_checks(&report), expected, "{report:#?}");
    }
}

// ----- An oracle for the differential tests ----------------------------

/// The modeled integer types: name, least value, greatest value.
const TYPES: [(&str, i128, i128); 8] = [
    ("int8", i8::MIN as i128, i8::MAX as i128),
    ("int16", i16::MIN as i128, i16::MAX as i128),
    ("int32", i32::MIN as i128, i32::MAX as i128),
    ("int64", i64::MIN as i128, i64::MAX as i128),
    ("uint8", 0, u8::MAX as i128),
    ("uint16", 0, u16::MAX as i128),
    ("uint32", 0, u32::MAX as i128),
    ("uint64", 0, u64::MAX as i128),
];

fn width(lo: i128, hi: i128) -> i128 {
    (hi - lo + 1).trailing_zeros() as i128
}

/// Values at the ends of a type's range, around zero, powers of two, and
/// an alternating bit pattern.
fn edges(lo: i128, hi: i128) -> Vec<i128> {
    let top_bit = 1i128 << (width(lo, hi) - 2);
    let pattern = 0x5555_5555_5555_5555 & hi;
    let mut values = vec![0, 1, 2, top_bit, pattern, hi - 1, hi];
    if lo < 0 {
        values.extend([-1, -2, -pattern, lo + 1, lo]);
    }
    values
}

/// What the run time gives for `~a`, computed apart from the model.
fn oracle_not(a: i128, lo: i128, hi: i128) -> i128 {
    if lo < 0 {
        !a
    } else {
        // Flip the low bits only.
        a ^ hi
    }
}

/// What the run time gives for `a << count`, or `None` where it stops:
/// a count outside `0..width`, or a result outside the type.
fn oracle_shl(a: i128, count: i128, lo: i128, hi: i128) -> Option<i128> {
    if !(0..width(lo, hi)).contains(&count) {
        return None;
    }
    let mut value = a;
    for _ in 0..count {
        value += value;
    }
    (lo..=hi).contains(&value).then_some(value)
}

/// What the run time gives for `a >> count`: the sign is kept.
fn oracle_shr(a: i128, count: i128, lo: i128, hi: i128) -> Option<i128> {
    (0..width(lo, hi)).contains(&count).then(|| a >> count)
}

/// An Aura literal; a negative one is parenthesized so it can follow an
/// operator.
fn au(value: i128) -> String {
    if value < 0 {
        format!("({value})")
    } else {
        value.to_string()
    }
}

// ----- Bit operations ----------------------------------------------------

#[test]
fn a_mask_a_shift_and_a_scale_prove() {
    assert_proved(
        r#"
def low(x: int64) -> int64:
    ensures 0 <= result and result <= 255
    return x & 255

def half(x: int64) -> int64:
    requires x >= 0
    ensures result <= x
    return x >> 1

def kb(n: int64) -> int64:
    requires 0 <= n and n < 1000
    ensures result == n * 1024
    return n << 10

def main():
    print(low(1000))
    print(half(9))
    print(kb(3))
"#,
    );
}

#[test]
fn wrong_claims_about_a_mask_and_a_shift_fail() {
    // `x & 255` is 255 for `x = 255`, `0 >> 1` is not below 0, and a
    // shift by 10 of a large value loses bits.
    assert_open(
        r#"
def low(x: int64) -> int64:
    ensures result <= 254
    return x & 255

def half(x: int64) -> int64:
    requires x >= 0
    ensures result < x
    return x >> 1

def kb(n: int64) -> int64:
    return n << 10

def main():
    print(low(1000))
    print(half(9))
    print(kb(3))
"#,
        &["3 ensures failed", "8 ensures failed", "12 overflow failed"],
    );
}

#[test]
fn a_shift_count_must_be_in_range() {
    // The run time stops on a negative count and on one that is not below
    // the width, for `>>` too.
    assert_open(
        r#"
def right(n: int64, c: int64) -> int64:
    return n >> c

def left(n: uint8, c: uint8) -> uint8:
    requires n == 0
    return n << c

def wide(x: int32) -> int32:
    return x >> 32

def main():
    print(right(8, 1))
    print(left(0 as uint8, 1 as uint8))
    print(wide(1))
"#,
        &[
            "3 overflow failed",
            "7 overflow failed",
            "10 overflow failed",
        ],
    );
    assert_proved(
        r#"
def right(n: int64, c: int64) -> int64:
    requires 0 <= c and c < 64
    requires n >= 0
    ensures result <= n
    ensures result >= 0
    return n >> c

def left(n: uint8, c: uint8) -> uint8:
    requires c < 8
    requires n == 0
    ensures result == 0
    return n << c

def bit(k: int64) -> int64:
    requires 0 <= k and k < 63
    ensures result > 0
    return 1 << k

def same(x: int32) -> int32:
    ensures result == x
    return (x << 0) >> 0

def main():
    print(right(8, 1))
    print(left(0 as uint8, 1 as uint8))
    print(bit(5))
    print(same(4))
"#,
    );
}

#[test]
fn a_right_shift_keeps_the_sign() {
    assert_proved(
        r#"
def sign(x: int32) -> int32:
    ensures result == 0 or result == -1
    ensures (x < 0) == (result == -1)
    return x >> 31

def floor_half(x: int64) -> int64:
    ensures result == x // 2
    return x >> 1

def main():
    print(sign(-5))
    print(floor_half(-3))
"#,
    );
    // It does not round toward zero.
    assert_open(
        r#"
def half(x: int64) -> int64:
    ensures result + result >= x
    return x >> 1

def main():
    print(half(4))
"#,
        &["3 ensures failed"],
    );
}

#[test]
fn bit_operations_with_a_constant_are_exact() {
    assert_proved(
        r#"
def field(x: int64) -> int64:
    ensures result == (x // 256) % 256 * 256
    return x & 65280

def set_low(x: int64) -> int64:
    ensures result % 16 == 15
    ensures result >= x
    return x | 15

def flip(x: uint8) -> uint8:
    ensures result == 255 - x
    return x ^ 255

def invert(x: uint8) -> uint8:
    ensures result == 255 - x
    return ~x

def negate(x: int64) -> int64:
    requires x > -9223372036854775807
    ensures result == -x
    return (~x) + 1

def align(x: int64) -> int64:
    requires 0 <= x and x < 1000000
    ensures result % 8 == 0
    ensures x <= result and result < x + 8
    return (x + 7) & -8

def even(x: int64) -> bool:
    ensures result == (x % 2 == 0)
    return 1 & x == 0

def all_bits(x: int64) -> int64:
    ensures result == x
    return (x & -1) | 0

def main():
    print(field(4660))
    print(set_low(32))
    print(flip(3 as uint8))
    print(invert(3 as uint8))
    print(negate(5))
    print(align(13))
    print(even(4))
    print(all_bits(7))
    print(~5)
    print(12 ^ 10)
"#,
    );
    assert_open(
        r#"
def set_low(x: int64) -> int64:
    ensures result > x
    return x | 15

def invert(x: int8) -> int8:
    ensures result + x == 0
    return ~x

def main():
    print(set_low(32))
    print(invert(3 as int8))
"#,
        &["3 ensures failed", "7 ensures failed"],
    );
}

#[test]
fn bit_operations_of_two_unknowns_have_their_bounds() {
    assert_proved(
        r#"
def both(a: int64, b: int64) -> int64:
    requires a >= 0 and b >= 0
    ensures 0 <= result and result <= a and result <= b
    return a & b

def either(a: int64, b: int64) -> int64:
    requires a >= 0 and b >= 0
    ensures result >= a and result >= b
    return a | b

def differ(a: uint32, b: uint32) -> uint32:
    requires a <= 1000 and b <= 1000
    ensures result <= 2000
    return a ^ b

def negative(a: int64, b: int64) -> int64:
    requires a < 0
    ensures result < 0
    return a | b

def masked(a: int64, b: int64) -> int64:
    requires b >= 0
    ensures 0 <= result and result <= b
    return a & b

def same(a: int64) -> int64:
    ensures result == a
    return (a & a) | (a ^ a)

def swapped(a: uint8, b: uint8) -> bool:
    ensures result
    return a & b == b & a and a | b == b | a and a ^ b == b ^ a

def main():
    print(both(12, 10))
    print(either(12, 10))
    print(differ(12 as uint32, 10 as uint32))
    print(negative(-4, 3))
    print(masked(-4, 7))
    print(same(9))
    print(swapped(3 as uint8, 5 as uint8))
"#,
    );
}

#[test]
fn a_claim_the_bounds_do_not_give_is_not_modeled() {
    // `12 ^ 10` is 6, so the first claim is wrong, and the second is true
    // but needs the bits. Neither is reported as failed with made-up
    // values, and neither is proved.
    assert_open(
        r#"
def differ(a: int64, b: int64) -> int64:
    requires a >= 0 and b >= 0
    ensures result >= a
    return a ^ b

def power(k: int64) -> int64:
    requires 0 <= k and k < 63
    ensures result & (result - 1) == 0
    return 1 << k

def main():
    print(differ(12, 10))
    print(power(3))
"#,
        &["4 ensures unsupported", "9 ensures unsupported"],
    );
    let Some(report) = verify(
        r#"
def differ(a: int64, b: int64) -> int64:
    ensures result == 0
    return a ^ b

def main():
    print(differ(12, 12))
"#,
    ) else {
        return;
    };
    let reasons: Vec<&str> = report
        .obligations
        .iter()
        .filter_map(|obligation| match &obligation.status {
            Status::Unsupported(reason) => Some(reason.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        reasons,
        ["it depends on `^` of two values that are not constants at line 4, which the verifier does not model"]
    );
}

#[test]
fn a_check_beside_an_unknown_bit_operation_still_fails() {
    // The check reads `a` only, so the made-up `a & b` is not its cause.
    assert_open(
        r#"
def both(a: int64, b: int64) -> int64:
    r = a & b
    assert a > 0
    return r

def main():
    print(both(12, 10))
"#,
        &["4 assert failed"],
    );
}

#[test]
fn contracts_read_bit_operations() {
    assert_proved(
        r#"
def low(x: int64) -> int64:
    ensures result == x & 255
    return x & 255

def use_low(x: int64) -> int64:
    ensures 0 <= result and result <= 255
    return low(x)

def scaled(x: uint32, k: uint32) -> uint32:
    requires k < 32
    requires x << k <= 1000
    ensures result == x << k
    return x << k

def use_scaled(x: uint32) -> uint32:
    requires x <= 10
    ensures result == x * 4
    return scaled(x, 2)

def halved(x: int64, k: int64) -> int64:
    requires 0 <= k and k < 64
    ensures result == x >> k
    return x >> k

def use_halved(x: int64) -> int64:
    requires x >= 0
    ensures result <= x
    return halved(x, 3)

def inverted(x: uint8) -> uint8:
    ensures result == (~x)
    return ~x

def twice(x: uint8) -> uint8:
    ensures result == x
    return inverted(inverted(x))

def either(a: int64, b: int64) -> int64:
    ensures result == a | b
    return a | b

def use_either(a: int64, b: int64) -> int64:
    requires a >= 0 and b >= 0
    ensures result >= a
    return either(b, a)

def exponent(k: int64) -> int64:
    requires 1 << k == 8
    ensures result == 3
    return k

def constants(x: int64) -> int64:
    requires 12 & 10 == 8 and (~(-9)) == 8
    ensures result == x
    return x

def clear(xs: list[int64], m: int64) -> bool:
    requires all([xs[i] & m == 0 for i in range(len(xs))])
    requires len(xs) > 2
    ensures result
    return xs[1] & m == 0

def main():
    print(use_low(1000))
    print(use_scaled(3 as uint32))
    print(use_halved(64))
    print(twice(7 as uint8))
    print(use_either(1, 2))
    print(exponent(3))
    print(constants(1))
    print(clear([8, 16, 32], 7))
"#,
    );
}

#[test]
fn a_shift_in_a_contract_has_the_run_time_checks() {
    // A caller must show the shift in the callee's `requires` loses no
    // bits and has a count in range.
    assert_open(
        r#"
def scaled(x: uint32, k: uint32) -> uint32:
    requires k < 32
    requires x << k <= 1000
    ensures result == x << k
    return x << k

def lost(x: uint32) -> uint32:
    return scaled(x, 2)

def far(x: uint32) -> uint32:
    requires x == 0
    return scaled(x, 31) + scaled(x, 32)

def main():
    print(lost(3 as uint32))
    print(far(0 as uint32))
"#,
        &[
            "9 requires failed",
            "13 requires failed",
            "13 requires failed",
        ],
    );
}

#[test]
fn a_set_operation_in_a_contract_is_not_read_as_bits() {
    // `&` of two sets is not an integer operation; the clause is skipped,
    // so nothing follows from it.
    assert_open(
        r#"
def common(a: set[int64], b: set[int64], n: int64) -> int64:
    requires len(a & b) > 3
    assert n > 0
    return 1

def main():
    print(common({1}, {1}, 1))
"#,
        &["4 assert failed", "8 requires unsupported"],
    );
}

/// One function per value of each type. It pins its parameter to the
/// value and asserts what each operation with a literal gives, as the
/// oracle computes it. The verifier must prove every assert through the
/// arithmetic encoding, and the interpreter must agree when it runs them.
#[test]
fn operations_with_a_literal_match_the_interpreter() {
    for (name, lo, hi) in TYPES {
        let values = edges(lo, hi);
        let counts = [0, 1, width(lo, hi) / 2, width(lo, hi) - 1];
        let mut source = String::new();
        let mut main = String::from("def main():\n");
        for (index, &a) in values.iter().enumerate() {
            source.push_str(&format!(
                "def check_{index}(a: {name}) -> {name}:\n    requires a == {}\n",
                au(a)
            ));
            for &m in &values {
                source.push_str(&format!(
                    "    assert a & {m} == {} and a | {m} == {} and a ^ {m} == {}\n",
                    au(a & m),
                    au(a | m),
                    au(a ^ m),
                    m = au(m),
                ));
                // A literal on the left takes its type from the right.
                if (0..3).contains(&m) {
                    source.push_str(&format!(
                        "    assert {m} & a == {} and {m} | a == {} and {m} ^ a == {}\n",
                        au(a & m),
                        au(a | m),
                        au(a ^ m),
                    ));
                }
            }
            source.push_str(&format!(
                "    assert (~a) == {}\n",
                au(oracle_not(a, lo, hi))
            ));
            for count in counts {
                let right = oracle_shr(a, count, lo, hi).expect("the count is in range");
                source.push_str(&format!("    assert a >> {count} == {}\n", au(right)));
                if let Some(left) = oracle_shl(a, count, lo, hi) {
                    source.push_str(&format!("    assert a << {count} == {}\n", au(left)));
                }
            }
            source.push_str("    return a\n\n");
            main.push_str(&format!("    check_{index}({})\n", au(a)));
        }
        source.push_str(&main);
        assert_proved(&source);
    }
}

/// The same for a shift by a count that is not a literal, which reads the
/// table of powers of two.
#[test]
fn shifts_by_a_variable_match_the_interpreter() {
    for (name, lo, hi) in TYPES {
        let counts = [0, 1, width(lo, hi) / 2, width(lo, hi) - 1];
        let mut source = String::new();
        let mut main = String::from("def main():\n");
        let mut index = 0;
        for a in edges(lo, hi) {
            for count in counts {
                source.push_str(&format!(
                    "def check_{index}(a: {name}, c: {name}) -> {name}:\n    requires a == {} and c == {count}\n",
                    au(a)
                ));
                let right = oracle_shr(a, count, lo, hi).expect("the count is in range");
                source.push_str(&format!("    assert a >> c == {}\n", au(right)));
                if let Some(left) = oracle_shl(a, count, lo, hi) {
                    source.push_str(&format!("    assert a << c == {}\n", au(left)));
                }
                source.push_str("    return a\n\n");
                main.push_str(&format!("    check_{index}({}, {count})\n", au(a)));
                index += 1;
            }
        }
        source.push_str(&main);
        assert_proved(&source);
    }
}

/// Each shift the oracle says stops the program: the verifier reports its
/// check as failed, and the interpreter stops.
#[test]
fn shifts_that_stop_the_program_are_reported() {
    for (name, lo, hi) in TYPES {
        let bits = width(lo, hi);
        // Value, count, and whether the shift is `<<`.
        let mut cases = vec![
            (1, bits, true),
            (1, bits, false),
            (0, bits + 1, false),
            (hi, 1, true),
            (hi / 2 + 1, 1, true),
            (1, bits - 1, lo < 0),
            (3, bits - 1, true),
        ];
        if lo < 0 {
            cases.extend([
                (1, -1, true),
                (1, -1, false),
                (lo, 1, true),
                (-2, bits - 1, true),
            ]);
        }
        let mut source = String::new();
        let mut expected = Vec::new();
        for (index, &(a, count, left)) in cases.iter().enumerate() {
            let symbol = if left { "<<" } else { ">>" };
            let stops = if left {
                oracle_shl(a, count, lo, hi).is_none()
            } else {
                oracle_shr(a, count, lo, hi).is_none()
            };
            let function = format!(
                "def variable_{index}(a: {name}, c: {name}) -> {name}:\n    requires a == {a} and c == {count}\n    return a {symbol} c\n\ndef literal_{index}(a: {name}) -> {name}:\n    requires a == {a}\n    return a {symbol} {count}\n\n",
                a = au(a),
                count = au(count),
            );
            if stops {
                expected.push(format!("{} overflow failed", index * 8 + 3));
                expected.push(format!("{} overflow failed", index * 8 + 7));
                for call in [
                    format!("variable_{index}({}, {})", au(a), au(count)),
                    format!("literal_{index}({})", au(a)),
                ] {
                    let run = format!("{function}def main():\n    print({call})\n");
                    assert!(crate::run_source(&run).is_err(), "{run}");
                }
            }
            source.push_str(&function);
        }
        source.push_str("def main():\n    print(1)\n");
        let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
        assert_open(&source, &expected);
    }
}

/// The bounds stated for `a & b` of two unknowns must allow what the run
/// time computes. With both pinned, a claim that the three results differ
/// from the oracle's would be proved if a bound left the true value out.
#[test]
fn bounds_of_unknown_bit_operations_allow_the_interpreter_results() {
    for (name, lo, hi) in TYPES {
        let pattern = 0x5555_5555_5555_5555 & hi;
        let mut values = vec![0, 1, pattern, hi];
        if lo < 0 {
            values.extend([-1, -pattern, lo]);
        }
        let mut source = String::new();
        let mut run = String::from("def main():\n");
        let mut expected = Vec::new();
        let mut index = 0;
        for &a in &values {
            for &b in &values {
                if a == b {
                    continue;
                }
                let results = format!(
                    "a & b == {} and a | b == {} and a ^ b == {}",
                    au(a & b),
                    au(a | b),
                    au(a ^ b)
                );
                source.push_str(&format!(
                    "def wrong_{index}(a: {name}, b: {name}) -> {name}:\n    requires a == {} and b == {}\n    assert not ({results})\n    return a\n\ndef right_{index}(a: {name}, b: {name}) -> {name}:\n    assert {results}\n    return a\n\n",
                    au(a),
                    au(b),
                ));
                expected.push(format!("{} assert unsupported", index * 9 + 3));
                expected.push(format!("{} assert unsupported", index * 9 + 7));
                run.push_str(&format!("    right_{index}({}, {})\n", au(a), au(b)));
                index += 1;
            }
        }
        source.push_str(&run);
        crate::run_source(&source).expect("the oracle agrees with the interpreter");
        let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
        assert_open(&source, &expected);
    }
}
