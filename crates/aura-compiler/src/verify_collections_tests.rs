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
    let Some(report) = verify(source) else {
        return;
    };
    assert_eq!(open_checks(&report), expected, "{report:#?}");
}

/// Runs a program whose own `assert`s compare an operation with the facts
/// its model states, over a spread of inputs.
fn assert_runs(source: &str) {
    crate::run_source(source).expect("the model's facts hold when the program runs");
}

#[test]
fn membership_in_a_list_is_the_contract_s_exists() {
    assert_proved(
        r#"
def has(xs: list[int64], k: int64) -> bool:
    ensures result == exists(x in xs, x == k)
    return k in xs

def has_method(xs: list[int64], k: int64) -> bool:
    ensures result == (k in xs)
    return xs.contains(k)

def lacks(xs: list[str], k: str) -> bool:
    ensures result == forall(x in xs, x != k)
    return k not in xs

def found_at(xs: list[int64], i: int64) -> bool:
    requires 0 <= i and i < len(xs)
    ensures result
    return xs[i] in xs

def pick(xs: list[int64], k: int64) -> int64:
    ensures result == 1 or result == 0
    ensures (result == 1) == xs.contains(k)
    if k in xs:
        return 1
    return 0

def caller(xs: list[int64]) -> bool:
    requires len(xs) > 2
    ensures result
    return has(xs, xs[2])

def literal() -> bool:
    ensures result
    return 2 in [1, 2, 3]

def main():
    print(has([1, 2, 3], 2))
    print(has_method([1, 2, 3], 5))
    print(lacks(["a"], "b"))
    print(found_at([4, 5], 1))
    print(pick([4, 5], 4))
    print(caller([1, 2, 3]))
    print(literal())
"#,
    );
}

#[test]
fn a_wrong_claim_about_membership_still_fails() {
    assert_open(
        r#"
def always(xs: list[int64], k: int64) -> bool:
    ensures result
    return k in xs

def never(xs: list[int64], k: int64) -> bool:
    requires len(xs) > 0
    ensures not result
    return k in xs

def first_only(xs: list[int64], k: int64) -> bool:
    requires len(xs) > 0
    ensures result == (xs[0] == k)
    return k in xs

def absent() -> bool:
    ensures result
    return 4 in [1, 2, 3]

def main():
    print(always([1], 1))
    print(never([1], 2))
    print(first_only([1], 1))
    print(absent())
"#,
        &[
            "3 ensures failed",
            "8 ensures failed",
            "13 ensures failed",
            "17 ensures failed",
        ],
    );
}

#[test]
fn membership_of_a_float_stays_unmodeled() {
    assert_open(
        r#"
def has(xs: list[float64], k: float64) -> bool:
    ensures result
    return k in xs

def main():
    print(has([1.0], 1.0))
"#,
        &["3 ensures unsupported"],
    );
}

#[test]
fn membership_agrees_with_a_search_at_run_time() {
    assert_runs(
        r#"
def search(xs: list[int64], k: int64) -> bool:
    mut found = false
    for x in xs:
        if x == k:
            found = true
    return found

def check(xs: list[int64]):
    for k in range(-2, 6):
        assert (k in xs) == search(xs, k)
        assert xs.contains(k) == search(xs, k)

def main():
    check([])
    check([0])
    check([3, 1, 3])
    check([-2, 5, 4, 4, 0])
    names = ["a", "", "b"]
    assert "a" in names
    assert "" in names
    assert "c" not in names
"#,
    );
}
