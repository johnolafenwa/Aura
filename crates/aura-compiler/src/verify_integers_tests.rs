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

/// One statement on pinned inputs: the parameters, the `requires` that
/// pins them, the statement, and the arguments of a call. Each `$` stands
/// for the item's number, which keeps the names of the items that share a
/// function apart.
struct Item {
    params: String,
    pinned: String,
    statement: String,
    args: String,
}

/// Adds functions to `source` that each pin the inputs of up to 16 items
/// and run their statements. Gives the calls of those functions and the
/// last line of each statement.
fn groups(source: &mut String, prefix: &str, items: &[Item]) -> (String, Vec<usize>) {
    let mut calls = String::new();
    let mut lines = Vec::new();
    let mut line = source.lines().count() + 1;
    for (group, chunk) in items.chunks(16).enumerate() {
        let numbered = |part: fn(&Item) -> &str| -> Vec<String> {
            chunk
                .iter()
                .enumerate()
                .map(|(at, item)| part(item).replace('$', &at.to_string()))
                .collect()
        };
        source.push_str(&format!(
            "def {prefix}_{group}({}) -> int64:\n",
            numbered(|item| &item.params).join(", ")
        ));
        for pinned in numbered(|item| &item.pinned) {
            source.push_str(&format!("    requires {pinned}\n"));
        }
        line += 1 + chunk.len();
        for statement in numbered(|item| &item.statement) {
            source.push_str(&format!("    {statement}\n"));
            line += statement.lines().count();
            lines.push(line - 1);
        }
        source.push_str("    return 0\n\n");
        line += 2;
        calls.push_str(&format!(
            "    {prefix}_{group}({})\n",
            numbered(|item| &item.args).join(", ")
        ));
    }
    (calls, lines)
}

/// One operation on pinned inputs, written like an `Item`, and what the
/// oracle says the run time gives: `None` where it stops the program.
struct Case {
    params: String,
    pinned: String,
    expr: String,
    args: String,
    expected: Option<i128>,
}

/// Checks each case against the verifier and the interpreter. A case with
/// a value must prove `assert expr == value`, and the assert must hold
/// when run. A case that stops must have exactly one failed check, on the
/// operation, and must stop in the interpreter.
fn check_cases(cases: &[Case]) {
    let item = |case: &Case, statement: String| Item {
        params: case.params.clone(),
        pinned: case.pinned.clone(),
        statement,
        args: case.args.clone(),
    };
    let mut good = Vec::new();
    let mut bad = Vec::new();
    for case in cases {
        match case.expected {
            Some(value) => good.push(item(case, format!("assert {} == {}", case.expr, au(value)))),
            None => bad.push(item(case, format!("print({})", case.expr))),
        }
    }
    let mut source = String::new();
    let (calls, _) = groups(&mut source, "good", &good);
    let (_, lines) = groups(&mut source, "bad", &bad);
    source.push_str(&format!("def main():\n    print(0)\n{calls}"));
    crate::run_source(&source).expect("the oracle agrees with the interpreter");
    let open: Vec<String> = lines
        .iter()
        .map(|line| format!("{line} overflow failed"))
        .collect();
    let open: Vec<&str> = open.iter().map(String::as_str).collect();
    assert_open(&source, &open);
    // Starting the interpreter costs more than a check, so it confirms a
    // spread of the cases that stop, not each one.
    let step = bad.len().div_ceil(8).max(1);
    for case in bad.into_iter().step_by(step) {
        let mut alone = String::new();
        let (calls, _) = groups(&mut alone, "bad", &[case]);
        alone.push_str(&format!("def main():\n{calls}"));
        assert!(crate::run_source(&alone).is_err(), "{alone}");
    }
}

/// Shifts by a count that is not a literal, which read the table of
/// powers of two, and shifts the run time refuses: a count outside the
/// width, or a `<<` that loses bits.
#[test]
fn shifts_match_the_interpreter() {
    for (name, lo, hi) in TYPES {
        let bits = width(lo, hi);
        let mut counts = vec![0, 1, bits / 2, bits - 1, bits, bits + 1];
        if lo < 0 {
            counts.push(-1);
        }
        let mut cases = Vec::new();
        for a in edges(lo, hi) {
            for &count in &counts {
                let results = [
                    ("<<", oracle_shl(a, count, lo, hi)),
                    (">>", oracle_shr(a, count, lo, hi)),
                ];
                for (symbol, expected) in results {
                    cases.push(Case {
                        params: format!("a$: {name}, c$: {name}"),
                        pinned: format!("a$ == {} and c$ == {}", au(a), au(count)),
                        expr: format!("a$ {symbol} c$"),
                        args: format!("{}, {}", au(a), au(count)),
                        expected,
                    });
                    // A literal count in range is covered above.
                    if expected.is_none() && a.abs() <= 1 {
                        cases.push(Case {
                            params: format!("a$: {name}"),
                            pinned: format!("a$ == {}", au(a)),
                            expr: format!("a$ {symbol} {}", au(count)),
                            args: au(a),
                            expected,
                        });
                    }
                }
            }
        }
        check_cases(&cases);
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
        let mut wrong = Vec::new();
        let mut right = Vec::new();
        for &a in &values {
            for &b in &values {
                if a == b {
                    continue;
                }
                let results = format!(
                    "a$ & b$ == {} and a$ | b$ == {} and a$ ^ b$ == {}",
                    au(a & b),
                    au(a | b),
                    au(a ^ b)
                );
                // Each wrong claim is behind a flag of its own: a claim
                // the bounds refute would leave no path to the next one.
                for (items, statement) in [
                    (
                        &mut wrong,
                        format!("if g$:\n        assert not ({results})"),
                    ),
                    (&mut right, format!("assert g$ and {results}")),
                ] {
                    items.push(Item {
                        params: format!("a$: {name}, b$: {name}, g$: bool"),
                        pinned: format!("a$ == {} and b$ == {}", au(a), au(b)),
                        statement,
                        args: format!("{}, {}, true", au(a), au(b)),
                    });
                }
            }
        }
        let mut run = String::new();
        let (calls, _) = groups(&mut run, "right", &right);
        run.push_str(&format!("def main():\n{calls}"));
        crate::run_source(&run).expect("the oracle agrees with the interpreter");
        let mut source = String::new();
        let (_, lines) = groups(&mut source, "wrong", &wrong);
        source.push_str("def main():\n    print(0)\n");
        let open: Vec<String> = lines
            .iter()
            .map(|line| format!("{line} assert unsupported"))
            .collect();
        let open: Vec<&str> = open.iter().map(String::as_str).collect();
        assert_open(&source, &open);
    }
}

// ----- Wrapping and saturating methods, and powers -----------------------

/// A value outside the type, brought back by whole ranges.
fn oracle_wrap(value: i128, lo: i128, hi: i128) -> i128 {
    (value - lo).rem_euclid(hi - lo + 1) + lo
}

/// What the run time gives for `a ** exponent`, or `None` where it stops:
/// a negative exponent, or a result outside the type.
fn oracle_pow(a: i128, exponent: i128, lo: i128, hi: i128) -> Option<i128> {
    if exponent < 0 {
        return None;
    }
    // 0, 1, and -1 repeat, whatever the exponent.
    if exponent > 0 && (-1..=1).contains(&a) {
        return Some(if exponent % 2 == 0 { a * a } else { a });
    }
    let mut value = 1i128;
    for _ in 0..exponent {
        // Past the type, a larger power never comes back into it.
        value = value
            .checked_mul(a)
            .filter(|value| (lo..=hi).contains(value))?;
    }
    Some(value)
}

#[test]
fn a_saturating_sum_does_not_fall() {
    assert_proved(
        r#"
def next(x: int64) -> int64:
    ensures result >= x
    return x.saturating_add(1)

def below(x: uint8, y: uint8) -> uint8:
    ensures result <= x
    return x.saturating_sub(y)

def scaled(x: int32) -> int32:
    requires x >= 0
    ensures result >= x
    return x.saturating_mul(3)

def main():
    print(next(3))
    print(below(3 as uint8, 5 as uint8))
    print(scaled(1000000000))
"#,
    );
    // It stays at the largest value, so it does not always grow.
    assert_open(
        r#"
def next(x: int64) -> int64:
    ensures result > x
    return x.saturating_add(1)

def main():
    print(next(3))
"#,
        &["3 ensures failed"],
    );
}

#[test]
fn a_wrapping_sum_has_no_overflow_check_and_its_exact_value() {
    let source = r#"
def next(x: int64) -> int64:
    ensures result == (x + 1 if x < 9223372036854775807 else -9223372036854775808)
    return x.wrapping_add(1)

def product(x: uint8, y: uint8) -> uint8:
    ensures result as int64 == (x as int64 * y as int64) % 256
    return x.wrapping_mul(y)

def back(x: int8, y: int8) -> int8:
    ensures result == x
    return x.wrapping_sub(y).wrapping_add(y)

def main():
    print(next(9223372036854775807))
    print(product(200 as uint8, 3 as uint8))
    print(back(100 as int8, -100 as int8))
"#;
    assert_proved(source);
    let Some(report) = verify(source) else {
        return;
    };
    // The only overflow checks are the contracts' own arithmetic.
    let on_calls: Vec<usize> = report
        .obligations
        .iter()
        .filter(|obligation| obligation.kind == ObligationKind::Overflow)
        .map(|obligation| obligation.span.line)
        .filter(|line| [4, 8, 12].contains(line))
        .collect();
    assert_eq!(on_calls, Vec::<usize>::new(), "{report:#?}");
    // A wrapped sum is not always larger.
    assert_open(
        r#"
def next(x: int64) -> int64:
    ensures result > x
    return x.wrapping_add(1)

def main():
    print(next(3))
"#,
        &["3 ensures failed"],
    );
}

#[test]
fn shift_methods_check_their_count() {
    assert_proved(
        r#"
def top(x: int8) -> int8:
    requires x > 0
    ensures result == 127
    return x.saturating_shl(7)

def low(x: uint8, c: uint8) -> uint8:
    requires c < 8
    ensures result <= x
    return x.wrapping_shr(c)

def main():
    print(top(3 as int8))
    print(low(200 as uint8, 3 as uint8))
"#,
    );
    assert_open(
        r#"
def shifted(x: int8, c: int8) -> int8:
    return x.saturating_shr(c)

def lost(x: uint8) -> uint8:
    ensures result >= x
    return x.wrapping_shl(1)

def main():
    print(shifted(3 as int8, 1 as int8))
    print(lost(3 as uint8))
"#,
        &["3 overflow failed", "6 ensures failed"],
    );
}

/// Every `wrapping_` and `saturating_` sum, difference, and product of
/// two edge values, against the oracle and the interpreter.
#[test]
fn width_methods_match_the_interpreter() {
    for (name, lo, hi) in TYPES {
        let values = edges(lo, hi);
        let mut source = String::new();
        let mut main = String::from("def main():\n");
        for (index, &a) in values.iter().enumerate() {
            let params: Vec<String> = (0..values.len())
                .map(|at| format!("b{at}: {name}"))
                .collect();
            let pinned: Vec<String> = values
                .iter()
                .enumerate()
                .map(|(at, b)| format!("b{at} == {}", au(*b)))
                .collect();
            source.push_str(&format!(
                "def check_{index}(a: {name}, {}) -> {name}:\n    requires a == {} and {}\n",
                params.join(", "),
                au(a),
                pinned.join(" and ")
            ));
            for (at, &b) in values.iter().enumerate() {
                let clamp = |value: i128| value.clamp(lo, hi);
                // The product of two 64-bit values can pass 127 bits.
                let product = a.checked_mul(b).unwrap_or(hi);
                source.push_str(&format!(
                    "    assert a.wrapping_add(b{at}) == {} and a.wrapping_sub(b{at}) == {} and a.wrapping_mul(b{at}) == {}\n    assert a.saturating_add(b{at}) == {} and a.saturating_sub(b{at}) == {} and a.saturating_mul(b{at}) == {}\n",
                    au(oracle_wrap(a + b, lo, hi)),
                    au(oracle_wrap(a - b, lo, hi)),
                    au(oracle_wrap(a.wrapping_mul(b), lo, hi)),
                    au(clamp(a + b)),
                    au(clamp(a - b)),
                    au(clamp(product)),
                ));
            }
            source.push_str("    return a\n\n");
            let args: Vec<String> = values.iter().map(|b| au(*b)).collect();
            main.push_str(&format!(
                "    check_{index}({}, {})\n",
                au(a),
                args.join(", ")
            ));
        }
        source.push_str(&main);
        assert_proved(&source);
    }
}

/// The shift methods on edge values and counts, among them counts the
/// run time refuses.
#[test]
fn shift_methods_match_the_interpreter() {
    for (name, lo, hi) in TYPES {
        let bits = width(lo, hi);
        let pattern = 0x5555_5555_5555_5555 & hi;
        let mut values = vec![0, 1, pattern, hi];
        let mut counts = vec![0, 1, bits / 2, bits - 1, bits];
        if lo < 0 {
            values.extend([-1, lo]);
            counts.push(-1);
        }
        let mut cases = Vec::new();
        for &a in &values {
            for &count in &counts {
                let valid = (0..bits).contains(&count);
                let scaled = a.checked_shl(count.clamp(0, 64) as u32).expect("it fits");
                let results = [
                    ("wrapping_shl", oracle_wrap(scaled, lo, hi)),
                    ("saturating_shl", scaled.clamp(lo, hi)),
                    ("wrapping_shr", a >> count.clamp(0, 64)),
                    ("saturating_shr", a >> count.clamp(0, 64)),
                ];
                for (method, value) in results {
                    cases.push(Case {
                        params: format!("a$: {name}, c$: {name}"),
                        pinned: format!("a$ == {} and c$ == {}", au(a), au(count)),
                        expr: format!("a$.{method}(c$)"),
                        args: format!("{}, {}", au(a), au(count)),
                        expected: valid.then_some(value),
                    });
                }
            }
        }
        check_cases(&cases);
    }
}

#[test]
fn powers_with_a_literal_prove() {
    assert_proved(
        r#"
def fifth(x: int64) -> int64:
    requires -100 <= x and x <= 100
    ensures result == x * x * x * x * x
    return x ** 5

def bit(k: int64) -> int64:
    requires 0 <= k and k < 63
    ensures result > 0 and result > k
    return 2 ** k

def sign(k: int64) -> int64:
    requires k >= 0
    ensures result == 1 or result == -1
    ensures (k % 2 == 0) == (result == 1)
    return (-1) ** k

def flat(k: uint8) -> uint8:
    ensures result <= 1
    return 0 ** k + 1 ** k - 1

def exponent(k: int64) -> int64:
    requires 0 <= k and k <= 10
    requires 2 ** k == 8 and k ** 2 == 9
    ensures result == 3
    return k

def main():
    print(fifth(3))
    print(bit(10))
    print(sign(3))
    print(flat(0 as uint8))
    print(exponent(3))
"#,
    );
    // `2 ** 63` is outside `int64`, a negative exponent stops the
    // program, and `x ** 5` is not always above `x`.
    assert_open(
        r#"
def bit(k: int64) -> int64:
    requires 0 <= k
    return 2 ** k

def ten(k: int64) -> int64:
    requires k < 5
    return 10 ** k

def fifth(x: int64) -> int64:
    requires -100 <= x and x <= 100
    ensures result >= x
    return x ** 5

def main():
    print(bit(3))
    print(ten(3))
    print(fifth(3))
"#,
        &[
            "4 overflow failed",
            "8 overflow failed",
            "12 ensures failed",
        ],
    );
}

#[test]
fn a_power_of_two_unknowns_has_its_bounds() {
    assert_open(
        r#"
def power(a: int64, k: int64) -> int64:
    requires a >= 1 and k >= 1
    ensures result >= a and result >= 1
    return a ** k

def large(a: int64, k: int64) -> int64:
    requires a >= 0 and k >= 0
    ensures result >= 5
    return a ** k

def negative(a: int64, k: int64) -> int64:
    return a ** k

def huge(a: int64) -> int64:
    return a ** 100

def after(a: int64, k: int64) -> int64:
    requires k >= 0
    r = a ** k
    return r // 2 + 1

def main():
    print(power(2, 3))
    print(large(2, 3))
    print(negative(2, 3))
    print(huge(1))
    print(after(2, 3))
"#,
        &[
            "5 overflow unsupported",
            "9 ensures unsupported",
            "10 overflow unsupported",
            "13 overflow failed",
            "13 overflow unsupported",
            "16 overflow unsupported",
            "20 overflow unsupported",
        ],
    );
}

/// Powers with a literal exponent or a literal base on edge values,
/// against the oracle and the interpreter.
#[test]
fn powers_match_the_interpreter() {
    for (name, lo, hi) in TYPES {
        let bits = width(lo, hi);
        let mut bases = vec![0, 1, 2, 3, 10, hi];
        if lo < 0 {
            bases.extend([-1, -2, -3, lo]);
        }
        let mut cases = Vec::new();
        for &a in &bases {
            // A literal exponent above 63 is not written out.
            for exponent in [0, 1, 2, 3, 5, bits - 2, bits - 1, bits.min(62), 63] {
                cases.push(Case {
                    params: format!("a$: {name}"),
                    pinned: format!("a$ == {}", au(a)),
                    expr: format!("a$ ** {exponent}"),
                    args: au(a),
                    expected: oracle_pow(a, exponent, lo, hi),
                });
            }
        }
        let mut exponents = vec![0, 1, 2, bits / 2, bits - 2, bits - 1, bits, hi];
        if lo < 0 {
            exponents.push(-1);
        }
        for &a in &bases {
            // A literal on the left is read as an `int64` first, and the
            // least value of a type is not a literal by itself.
            if (a == lo && lo < 0) || a > i64::MAX as i128 {
                continue;
            }
            for &exponent in &exponents {
                cases.push(Case {
                    params: format!("k$: {name}"),
                    pinned: format!("k$ == {}", au(exponent)),
                    expr: format!("{} ** k$", au(a)),
                    args: au(exponent),
                    expected: oracle_pow(a, exponent, lo, hi),
                });
            }
        }
        check_cases(&cases);
    }
}

/// The bounds stated for `a ** k` of two unknowns must allow what the run
/// time computes: with both pinned, the claim that the power differs from
/// the oracle's is not proved.
#[test]
fn bounds_of_unknown_powers_allow_the_interpreter_results() {
    for (name, lo, hi) in TYPES {
        let bits = width(lo, hi);
        let mut bases = vec![0, 1, 2, 3, hi];
        if lo < 0 {
            bases.extend([-1, -2, lo]);
        }
        let mut wrong = Vec::new();
        let mut right = Vec::new();
        for &a in &bases {
            for exponent in [0, 1, 2, bits - 2, bits - 1] {
                let Some(value) = oracle_pow(a, exponent, lo, hi) else {
                    continue;
                };
                for (items, statement) in [
                    (
                        &mut wrong,
                        format!("if g$:\n        assert a$ ** k$ != {}", au(value)),
                    ),
                    (
                        &mut right,
                        format!("assert g$ and a$ ** k$ == {}", au(value)),
                    ),
                ] {
                    items.push(Item {
                        params: format!("a$: {name}, k$: {name}, g$: bool"),
                        pinned: format!("a$ == {} and k$ == {exponent}", au(a)),
                        statement,
                        args: format!("{}, {exponent}, true", au(a)),
                    });
                }
            }
        }
        let mut run = String::new();
        let (calls, _) = groups(&mut run, "right", &right);
        run.push_str(&format!("def main():\n{calls}"));
        crate::run_source(&run).expect("the oracle agrees with the interpreter");
        let mut source = String::new();
        let (_, lines) = groups(&mut source, "wrong", &wrong);
        source.push_str("def main():\n    print(0)\n");
        let mut open = Vec::new();
        for line in lines {
            open.push(format!("{line} assert unsupported"));
            open.push(format!("{line} overflow unsupported"));
        }
        let open: Vec<&str> = open.iter().map(String::as_str).collect();
        assert_open(&source, &open);
    }
}

// ----- Casts in contracts ------------------------------------------------

#[test]
fn contracts_read_casts_between_numbers() {
    assert_proved(
        r#"
def widen(x: uint8) -> int64:
    ensures result == x as int64
    return x as int64

def use_widen(x: uint8) -> int64:
    ensures 0 <= result and result <= 255
    return widen(x)

def narrow(x: int64) -> int8:
    requires x as int8 > 0
    ensures result as int64 == x
    return x as int8

def use_narrow() -> int8:
    ensures result == 5
    return narrow(5)

def ratio(n: int64) -> float64:
    requires 0 <= n and n <= 1000
    ensures result == n as float64
    return n as float64

def use_ratio() -> float64:
    ensures result == 3.0
    return ratio(3)

def whole(x: float64) -> int64:
    requires 0.0 <= x and x <= 100.0
    ensures result == x as int64
    return x as int64

def use_whole() -> int64:
    ensures result == 3
    return whole(3.75)

def single(x: float64) -> float32:
    requires -1.0 <= x and x <= 1.0
    ensures result == x as float32
    return x as float32

def use_single() -> float32:
    ensures result as float64 == 0.5
    return single(0.5)

def main():
    print(use_widen(3 as uint8))
    print(use_narrow())
    print(use_ratio())
    print(use_whole())
    print(use_single())
"#,
    );
}

#[test]
fn a_cast_in_a_contract_has_the_run_time_checks() {
    // The caller must show each cast in a `requires` is one the run time
    // accepts: 128 is not an `int8`, a float outside `int64` has no
    // integer value, and 2^53 + 1 is not a `float64`.
    assert_open(
        r#"
def narrow(x: int64) -> int8:
    requires x as int8 > 0
    return x as int8

def use_narrow(x: int64) -> int8:
    requires 0 < x and x <= 128
    return narrow(x)

def whole(x: float64) -> int64:
    requires x as int64 >= 0
    return 1

def use_whole(x: float64) -> int64:
    requires x >= 0.0
    return whole(x)

def exact(n: int64) -> float64:
    requires 9007199254740990 <= n and n <= 9007199254740993
    requires n as float64 >= 0.0
    return 1.0

def use_exact(n: int64) -> float64:
    requires n == 9007199254740992 or n == 9007199254740993
    if n == 9007199254740992:
        return exact(n)
    return exact(n)

def wide(x: int128) -> int64:
    requires x as int64 > 0
    return 1

def use_wide() -> int64:
    return wide(5)

def main():
    print(use_narrow(5))
    print(use_whole(1.5))
    print(use_wide())
"#,
        &[
            "8 requires failed",
            "16 requires failed",
            "27 requires failed",
            "30 overflow unsupported",
        ],
    );
}
