//! What `aura verify` reports for a check it cannot prove: a failure with
//! a counterexample only when the program can produce it, "not modeled"
//! when the check reads a result the encoding makes up, and a contract
//! that can never hold.

use super::*;

fn verify(source: &str) -> Option<Report> {
    if smt::SolverCommand::find(None).is_err() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must install an SMT solver for the verifier tests"
        );
        eprintln!("skipping: no SMT solver on PATH");
        return None;
    }
    let program = crate::check_source(source).expect("the program checks");
    let module = crate::mir::lower_for_verification(&program);
    Some(verify_module(&program, &module, &Options::default()).expect("the verifier runs"))
}

/// Each check that was not proved, as `function: status`, where a check
/// that is not modeled gives its reason.
fn open(report: &Report) -> Vec<String> {
    report
        .obligations
        .iter()
        .filter_map(|obligation| {
            let status = match &obligation.status {
                Status::Proved => return None,
                Status::Failed { .. } => "failed".to_string(),
                Status::Unknown => "unknown".to_string(),
                Status::Unsupported(reason) => reason.clone(),
            };
            Some(format!("{}: {status}", obligation.function))
        })
        .collect()
}

fn not_modeled(what: &str, line: usize) -> String {
    format!("it depends on {what} at line {line}, which the verifier does not model")
}

#[test]
fn a_check_that_reads_an_unmodeled_result_is_not_a_failure() {
    let Some(report) = verify(
        r#"
def parts(text: str) -> int64:
    ensures result >= 1
    return len(text.split(","))

def fmt(n: int64) -> str:
    ensures len(result) > 0
    return f"n={n}"

def smallest(xs: mut list[int64]) -> int64:
    requires len(xs) > 0
    ensures forall(x in xs, result <= x)
    xs.sort()
    return xs[0]

def has(xs: list[int64], k: int64) -> bool:
    ensures result == exists(x in xs, x == k)
    return k in xs

def low(x: int64) -> int64:
    ensures 0 <= result and result <= 255
    return x & 255

def positive(xs: list[int64]) -> int64:
    ys = [x for x in xs if x > 0]
    return ys[0]

def middle(xs: list[int64]) -> int64:
    requires len(xs) > 3
    ys = xs[1:3]
    return ys[0]

def total(xs: list[int64]) -> int64:
    ensures result >= 0
    return sum(xs)

def next(x: int64) -> int64:
    ensures result >= x
    return x.saturating_add(1)

def main():
    print(1)
"#,
    ) else {
        return;
    };
    assert_eq!(
        open(&report),
        [
            format!("parts: {}", not_modeled("`split`", 4)),
            format!("fmt: {}", not_modeled("an f-string", 8)),
            format!(
                "smallest: {}",
                not_modeled("the order of the elements after `sort`", 13)
            ),
            format!("has: {}", not_modeled("`contains`", 18)),
            format!("low: {}", not_modeled("`&`", 22)),
            format!(
                "positive: {}",
                not_modeled("a comprehension or built-in loop", 25)
            ),
            format!("middle: {}", not_modeled("`slice`", 30)),
            format!(
                "total: {}",
                not_modeled("a comprehension or built-in loop", 35)
            ),
            format!(
                "total: {}",
                not_modeled("a comprehension or built-in loop", 35)
            ),
            format!("next: {}", not_modeled("`saturating_add`", 39)),
        ],
        "{report:#?}"
    );
    assert_eq!(
        (report.failed(), report.not_modeled(), report.undecided()),
        (0, 10, 0)
    );
    let diagnostics = report.diagnostics();
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code == "AU5104"));
    assert!(diagnostics
        .iter()
        .all(|diagnostic| !format!("{diagnostic:?}").contains("counterexample")));
}

#[test]
fn a_real_failure_beside_an_unmodeled_result_still_fails() {
    // The overflow does not read what `split` or `in` returned, and the
    // loop's own variables are the program's, so each is a failure.
    let Some(report) = verify(
        r#"
def beside(x: int64, text: str) -> int64:
    n = len(text.split(","))
    return x + 1

def after(xs: list[int64], k: int64, x: int64) -> int64:
    if k in xs:
        return x + 1
    return 0

def joined(xs: list[int64], k: int64, x: int64) -> int64:
    mut y = 0
    if k in xs:
        y = 1
    return x + 1

def each(xs: list[int64]) -> int64:
    for x in xs:
        assert x == 0
    return 0

def stuck(n: int64) -> int64:
    requires n >= 0
    mut i = n
    while i > 0:
        invariant i >= 0
        decreases i
        if i > 100:
            i -= 1
    return i

def main():
    print(1)
"#,
    ) else {
        return;
    };
    assert_eq!(
        open(&report),
        [
            "beside: failed",
            "after: failed",
            "joined: failed",
            "each: failed",
            "stuck: failed"
        ],
        "{report:#?}"
    );
    assert_eq!(report.failed(), 5);
}

#[test]
fn a_check_guarded_by_an_unmodeled_result_is_not_a_failure() {
    // `split` never returns an empty list, so this `assert` cannot run:
    // the test that leads to it reads a result the encoding makes up.
    let Some(report) = verify(
        r#"
def never(text: str) -> int64:
    n = len(text.split(","))
    assert n > 0
    return n

def main():
    print(never("a"))
"#,
    ) else {
        return;
    };
    assert_eq!(
        open(&report),
        [format!("never: {}", not_modeled("`split`", 3))],
        "{report:#?}"
    );
}

#[test]
fn a_contract_that_can_never_hold_is_reported() {
    let Some(report) = verify(
        r#"
def bad(x: int64) -> int64:
    requires x > 0 and x < 0
    ensures result == 12345
    return x

class Box:
    size: int64
    invariant self.size > 0
    invariant self.size < 0

    public def grow(mut self):
        self.size += 1

def good(x: int64) -> int64:
    requires x > 0
    ensures result > 1
    return x + 1

def main():
    print(good(1))
"#,
    ) else {
        return;
    };
    let unsatisfiable: Vec<(&str, &str)> = report
        .obligations
        .iter()
        .filter(|obligation| obligation.kind == ObligationKind::Satisfiable)
        .map(|obligation| (obligation.function.as_str(), obligation.message.as_str()))
        .collect();
    assert_eq!(
        unsatisfiable,
        [
            ("bad", "the contract `bad` assumes on entry"),
            ("Box.grow", "the contract `Box.grow` assumes on entry"),
        ],
        "{report:#?}"
    );
    assert!(!report.is_proved());
    let codes: Vec<String> = report
        .diagnostics()
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.iter().any(|code| code == "AU5120"), "{codes:?}");
    assert_eq!(ObligationKind::Satisfiable.name(), "satisfiable contract");
}

#[test]
fn a_contract_that_can_hold_adds_no_check() {
    let Some(report) = verify(
        r#"
def good(x: int64) -> int64:
    requires 0 < x and x < 100
    ensures result > 1
    return x + 1

def main():
    print(good(1))
"#,
    ) else {
        return;
    };
    assert!(report.is_proved(), "{report:#?}");
    assert!(report
        .obligations
        .iter()
        .all(|obligation| obligation.kind != ObligationKind::Satisfiable));
}
