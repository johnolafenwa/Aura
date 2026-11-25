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

/// Each obligation that was not proved, as `line kind status in function`.
fn open_checks(report: &Report) -> Vec<String> {
    report
        .obligations
        .iter()
        .filter(|obligation| obligation.status != Status::Proved)
        .map(|obligation| {
            format!(
                "{} {} in {}",
                obligation.span.line,
                obligation.kind.name(),
                obligation.function
            )
        })
        .collect()
}

/// Verifies a program that must prove completely, and runs it to show the
/// interpreter agrees that no check fails.
fn assert_proved(source: &str) -> Option<Report> {
    let report = verify(source)?;
    assert_eq!(open_checks(&report), Vec::<String>::new(), "{report:#?}");
    assert!(report.is_proved());
    assert!(!report.obligations.is_empty());
    crate::run_source(source).expect("a proved program runs without a failed check");
    Some(report)
}

fn assert_open(source: &str, expected: &[&str]) {
    let Some(report) = verify(source) else {
        return;
    };
    assert_eq!(open_checks(&report), expected, "{report:#?}");
}

/// The functions the report names, in order, without repeats.
fn reported_functions(report: &Report) -> Vec<&str> {
    let mut names: Vec<&str> = report
        .obligations
        .iter()
        .map(|obligation| obligation.function.as_str())
        .collect();
    names.dedup();
    names
}

const COUNTDOWN: &str = r#"
from iter import Generator

def countdown(n: int64) -> Generator[int64]:
    requires 0 <= n and n <= 100
    mut i = n
    while i > 0:
        invariant 0 <= i and i <= n
        yield i
        i -= 1

def main():
    for x in countdown(ARG):
        print(x)
"#;

#[test]
fn generator_body_assumes_the_generators_requires() {
    let Some(report) = assert_proved(&COUNTDOWN.replace("ARG", "5")) else {
        return;
    };
    // The body's checks are reported under the generator's own name.
    assert_eq!(reported_functions(&report), ["countdown", "main"]);
    assert!(report
        .obligations
        .iter()
        .any(|obligation| obligation.kind == ObligationKind::LoopInvariant));
}

#[test]
fn generator_call_still_proves_requires() {
    assert_open(&COUNTDOWN.replace("ARG", "-5"), &["13 requires in main"]);
}

#[test]
fn generator_body_without_requires_does_not_prove() {
    assert_open(
        &COUNTDOWN
            .replace("ARG", "5")
            .replace("    requires 0 <= n and n <= 100\n", ""),
        &["7 loop invariant in countdown"],
    );
}

#[test]
fn generator_body_wrong_invariant_fails() {
    assert_open(
        &COUNTDOWN
            .replace("ARG", "5")
            .replace("invariant 0 <= i and i <= n", "invariant 0 < i and i <= n"),
        &["8 loop invariant in countdown"],
    );
}

#[test]
fn generic_generator_body_assumes_requires() {
    let Some(report) = assert_proved(
        r#"
from iter import Generator

def after[T](x: own T, n: int64) -> Generator[T]:
    requires 0 <= n and n <= 10
    mut i = 0
    while i < n:
        invariant 0 <= i and i <= n
        i += 1
    assert i == n
    yield x

def main():
    for x in after[int64](7, 3):
        print(x)
"#,
    ) else {
        return;
    };
    assert_eq!(reported_functions(&report), ["after", "main"]);
}

const WALK: &str = r#"
from iter import Generator

class Bounds:
    public lo: int64
    public hi: int64

def walk(s: own Bounds) -> Generator[int64]:
    requires REQUIRES
    mut i = s.lo
    while i < s.hi:
        invariant s.lo <= i and i <= s.hi
        yield i
        i += 1

def main():
    for x in walk(Bounds(1, 4)):
        print(x)
"#;

#[test]
fn generator_requires_over_an_owned_object() {
    assert_proved(&WALK.replace("REQUIRES", "s.lo <= s.hi"));
    assert_open(
        &WALK.replace("REQUIRES", "s.lo <= s.hi + 1"),
        &["12 loop invariant in walk"],
    );
}

#[test]
fn function_named_like_a_generator_body_assumes_nothing() {
    // Only the body function the checker makes has the generator's span.
    assert_open(
        r#"
def foo(n: int64) -> int64:
    requires n > 0
    return n

def __aura_generator_body_foo(n: int64) -> int64:
    assert n > 0
    return n

def main():
    print(foo(3))
    print(__aura_generator_body_foo(3))
"#,
        &["7 assert in __aura_generator_body_foo"],
    );
}

#[test]
fn generator_with_a_frame_is_verified_once_as_written() {
    // A generator with no clauses also has a state-machine copy, whose
    // checks would not know that `i > 0` held before the `yield`.
    let Some(report) = assert_proved(
        r#"
from iter import Generator

def countdown(n: int64) -> Generator[int64]:
    mut i = n
    while i > 0:
        yield i
        i -= 1

def main():
    for x in countdown(5):
        print(x)
"#,
    ) else {
        return;
    };
    assert_eq!(reported_functions(&report), ["countdown", "main"]);
}

#[test]
fn generator_with_a_frame_still_fails_in_its_body() {
    assert_open(
        r#"
from iter import Generator

def evens() -> Generator[int64]:
    mut i = 0
    while true:
        yield i
        i += 2

def main():
    for x in evens():
        if x > 10:
            break
"#,
        &["8 overflow in evens"],
    );
}

#[test]
fn shared_state_is_unknown_after_a_yield() {
    // The consumer runs between a `yield` and the statement after it.
    assert_open(
        r#"
from iter import Generator
import sync

def watch(a: sync.AtomicInt) -> Generator[int64]:
    before = a.load()
    mut seen = 1
    yield before
    assert seen == 1
    assert a.load() == before

def main():
    a = sync.AtomicInt(0)
    for x in watch(a):
        a.store(1)
"#,
        &["10 assert in watch"],
    );
}
