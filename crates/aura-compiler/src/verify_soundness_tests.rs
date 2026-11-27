//! Programs the verifier once proved although a check of theirs fails when
//! they run. Each must now leave that check open.

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

fn assert_open(source: &str, expected: &[&str]) {
    if let Some(report) = verify(source) {
        assert_eq!(open_checks(&report), expected, "{report:#?}");
        assert!(!report.is_proved());
    }
}

fn assert_proved(source: &str) {
    if let Some(report) = verify(source) {
        assert_eq!(open_checks(&report), Vec::<String>::new(), "{report:#?}");
        assert!(report.is_proved());
    }
}

// ----- B-HIGH-09: the error path of `try` returns ----------------------

#[test]
fn try_error_return_proves_mut_parameter_invariants() {
    assert_open(
        r#"
class Pos:
    value: int64
    invariant self.value > 0

def break_pos(p: mut Pos, r: own Result[int64, str]) -> Result[int64, str]:
    p.value = 0
    v = try r
    p.value = 1
    return Result.Ok(v)

def main():
    mut p = Pos(value=5)
    _ = break_pos(p, Result[int64, str].Err("e"))
"#,
        &["8 class invariant failed"],
    );
}

#[test]
fn try_error_return_proves_ensures() {
    assert_open(
        r#"
def f(x: mut int64, r: own Result[int64, str]) -> Result[int64, str]:
    ensures x > 0
    x = 0
    v = try r
    x = 1
    return Result.Ok(v)

def main():
    mut x = 5
    _ = f(x, Result[int64, str].Err("e"))
"#,
        &["5 ensures failed"],
    );
}

#[test]
fn try_error_return_keeps_a_contract_that_holds() {
    assert_proved(
        r#"
class Pos:
    value: int64
    invariant self.value > 0

def f(p: mut Pos, x: mut int64, r: own Result[int64, str]) -> Result[int64, str]:
    ensures x > 0
    x = 2
    v = try r
    p.value = 3
    x = 1
    return Result.Ok(v)

def main():
    mut p = Pos(value=5)
    mut x = 5
    _ = f(p, x, Result[int64, str].Err("e"))
"#,
    );
}

// ----- B-HIGH-13: `==` of dicts with float values ---------------------

#[test]
fn dict_float_values_compare_as_numbers() {
    assert_open(
        r#"
def f() -> None:
    a: dict[int64, float64] = {1: 0.0}
    b: dict[int64, float64] = {1: -0.0}
    assert a != b

def main():
    f()
"#,
        &["5 assert failed"],
    );
    assert_proved(
        r#"
def f() -> None:
    a: dict[int64, float64] = {1: 0.0}
    b: dict[int64, float64] = {1: -0.0}
    assert a == b

def main():
    f()
"#,
    );
}

// ----- B-HIGH-14: a `mut` parameter of a contracted callable alias -----

#[test]
fn mut_callable_parameter_write_fits_its_contract() {
    assert_open(
        r#"
type PosFn = def(x: int64) -> int64:
    requires x >= 0
    ensures result > 0

def bad_fn(x: int64) -> int64:
    return 0

def poison(op: mut PosFn) -> None:
    op = bad_fn

def main():
    pass
"#,
        &["10 ensures failed"],
    );
}

// ----- B-HIGH-17: `enumerate` positions past `int64` -------------------

#[test]
fn enumerate_start_counts_within_int64() {
    assert_open(
        r#"
spec def enum_ok(xs: list[int64], start: int64) -> bool:
    return forall((i, v) in enumerate(xs, start), i >= start)

def f(xs: list[int64], start: int64) -> int64:
    requires len(xs) == 2
    ensures enum_ok(xs, start)
    return 1

def main():
    pass
"#,
        &["7 ensures failed", "8 assert failed"],
    );
    assert_proved(
        r#"
spec def enum_ok(xs: list[int64], start: int64) -> bool:
    return forall((i, v) in enumerate(xs, start), i >= start)

def f(xs: list[int64], start: int64) -> int64:
    requires len(xs) == 2 and 0 <= start and start <= 100
    ensures enum_ok(xs, start)
    return 1

def main():
    pass
"#,
    );
}

// ----- B-HIGH-19: integers the encoding has no range for --------------

#[test]
fn wide_integer_arithmetic_failures_are_found_not_proved() {
    assert_open(
        r#"
def f(x: int128) -> int128:
    return x // (0 as int128)

def g(x: int64) -> uint128:
    return x as uint128

def h(a: intsize, b: intsize) -> intsize:
    return a + b

def main():
    pass
"#,
        &[
            "3 division by zero failed",
            "6 overflow failed",
            "9 overflow failed",
        ],
    );
}

#[test]
fn widening_to_int128_needs_no_check() {
    assert_proved(
        r#"
def f(x: int64) -> int128:
    assert x == x
    return x as int128

def main():
    pass
"#,
    );
}
