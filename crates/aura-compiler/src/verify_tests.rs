use super::*;

/// Verifies `source` with the solver on this machine. Without one the test
/// is skipped, except under CI, which installs a pinned Z3.
fn verify(source: &str) -> Option<Report> {
    verify_with(source, &Options::default())
}

fn verify_with(source: &str, options: &Options) -> Option<Report> {
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
    Some(verify_module(&program, &module, options).expect("the verifier runs"))
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

fn assert_open(source: &str, expected: &[&str]) -> Option<Report> {
    let report = verify(source)?;
    assert_eq!(open_checks(&report), expected, "{report:#?}");
    Some(report)
}

#[test]
fn proves_contracts_traps_and_loop_invariants() {
    assert_proved(
        r#"
def find(xs: list[int64], key: int64) -> int64:
    ensures result >= -1 and result < len(xs)
    mut lo = 0
    mut hi = len(xs)
    while lo < hi:
        invariant 0 <= lo and lo <= hi and hi <= len(xs)
        decreases hi - lo
        mid = lo + (hi - lo) // 2
        if xs[mid] < key:
            lo = mid + 1
        elif xs[mid] > key:
            hi = mid
        else:
            return mid
    return -1

def clamp(x: int64, lo: int64, hi: int64) -> int64:
    requires lo <= hi
    ensures lo <= result and result <= hi
    if x < lo:
        return lo
    if x > hi:
        return hi
    return x

def use_clamp(x: int64) -> int64:
    y = clamp(x, 0, 10)
    assert y >= 0
    return y * 2

def sum_to(n: int64) -> int64:
    requires 0 <= n and n <= 1000
    mut i = 0
    mut acc = 0
    while i < n:
        invariant 0 <= i and i <= n
        invariant 0 <= acc and acc <= i * 1000
        acc += i
        i += 1
    return acc

def push(xs: mut list[int64], x: int64):
    ensures len(xs) == len(old(xs)) + 1
    ensures xs[len(xs) - 1] == x
    xs.append(x)

def use_push():
    mut xs = [1, 2]
    push(xs, 3)
    assert len(xs) == 3
    assert xs[2] == 3

def safe_div(a: int64, b: int64) -> int64:
    requires b != 0
    requires not (a == -9223372036854775807 - 1 and b == -1)
    return a // b

def last(xs: list[int64]) -> int64:
    requires len(xs) > 0
    return xs[-1]

def first_zero(xs: list[int64]) -> int64:
    mut i = 0
    while true:
        if i >= len(xs):
            break
        if xs[i] == 0:
            return i
        i += 1
    return -1

def main():
    print(find([1, 3, 5], 3))
    print(use_clamp(5))
    print(sum_to(10))
    use_push()
    print(safe_div(7, 2))
    print(last([4]))
    print(first_zero([1, 0]))
"#,
    );
}

#[test]
fn integer_division_and_negative_indexing_follow_the_language() {
    assert_proved(
        r#"
def main():
    assert -7 // 2 == -4
    assert -7 % 2 == 1
    assert 7 % -2 == -1
    assert 7 // -2 == -4
    assert 7 // 2 == 3
    xs = [10, 20, 30]
    assert xs[-1] == 30
    assert xs[-3] == 10
    assert len(xs) == 3
    mut ys = [1]
    ys.append(2)
    ys[0] = 5
    assert ys[0] + ys[1] == 7
    assert ys.pop() == 2
    assert len(ys) == 1
"#,
    );
    assert_open(
        r#"
def main():
    assert -7 // 2 == -3
"#,
        &["3 assert failed"],
    );
}

#[test]
fn a_call_proves_requires_and_assumes_ensures_only_after_it() {
    let report = assert_open(
        r#"
def clamp(x: int64, lo: int64, hi: int64) -> int64:
    requires lo <= hi
    ensures lo <= result and result <= hi
    if x < lo:
        return lo
    if x > hi:
        return hi
    return x

def bad(x: int64) -> int64:
    return clamp(x, 10, 0)

def main():
    print(bad(1))
"#,
        &["12 requires failed"],
    );
    if let Some(report) = report {
        let failed = report
            .obligations
            .iter()
            .find(|obligation| obligation.kind == ObligationKind::Requires)
            .expect("the requires obligation");
        assert_eq!(failed.function, "bad");
        assert!(failed.message.contains("`requires` of `clamp`"));
        let diagnostics = report.diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "AU5101");
    }
}

#[test]
fn reports_overflow_with_a_counterexample() {
    let Some(report) = assert_open(
        r#"
def double(x: int64) -> int64:
    return x * 2

def main():
    print(double(2))
"#,
        &["3 overflow failed"],
    ) else {
        return;
    };
    let Status::Failed { counterexample } = &report.obligations[0].status else {
        panic!("a failed overflow");
    };
    assert_eq!(counterexample.len(), 1);
    assert_eq!(counterexample[0].0, "x");
    let x: i128 = counterexample[0].1.parse().expect("an integer");
    assert!(x * 2 > i64::MAX as i128 || x * 2 < i64::MIN as i128);
    let rendered = report.diagnostics()[0].render_with_source("double.au", "");
    assert!(rendered.contains("counterexample: x = "), "{rendered}");
}

#[test]
fn loop_invariants_must_hold_on_entry_and_be_preserved() {
    assert_open(
        r#"
def entry(n: int64) -> int64:
    mut i = 1
    while i < n:
        invariant i == 0
        i += 1
    return i

def kept(n: int64) -> int64:
    requires n >= 0
    mut i = 0
    while i < n:
        invariant i <= 5
        i += 1
    return i

def main():
    print(entry(1))
    print(kept(3))
"#,
        &["5 loop invariant failed", "13 loop invariant failed"],
    );
}

#[test]
fn decreases_must_shrink_and_stay_non_negative() {
    assert_open(
        r#"
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
    print(stuck(0))
"#,
        &["7 decreases failed"],
    );
}

#[test]
fn models_enums_tuples_classes_and_unions() {
    assert_proved(
        r#"
enum Shape:
    Circle(int64)
    Rect(int64, int64)

def width(s: Shape) -> int64:
    match s:
        case Circle(r):
            return r // 2
        case Rect(w, h):
            return w // 2

def tag(s: Shape) -> int64:
    ensures result == 1
    match Shape.Rect(1, 2):
        case Circle(_):
            return 0
        case Rect(w, h):
            return w

def maybe(xs: list[int64], i: int64) -> int64 | None:
    if 0 <= i and i < len(xs):
        return xs[i]
    return None

def use_maybe(xs: list[int64]) -> int64:
    v = maybe(xs, 0)
    if v is None:
        return 0
    return v // 2

def pair_sum(p: (int32, int32)) -> int64:
    a, b = p
    return (a as int64) + (b as int64)

class Point:
    x: int32
    y: int32

def norm1(p: Point) -> int64:
    return abs(p.x as int64) + abs(p.y as int64)

def moved(p: Point) -> Point:
    ensures result.x == 0
    return Point(x=0, y=p.y)

def main():
    print(width(Shape.Circle(1)))
    print(tag(Shape.Circle(1)))
    print(use_maybe([3]))
    print(pair_sum((1, 2)))
    print(norm1(Point(x=1, y=-2)))
    print(moved(Point(x=1, y=2)).x)
"#,
    );
}

#[test]
fn class_invariants_hold_at_construction_and_around_public_methods() {
    assert_open(
        r#"
class Account:
    balance: int64
    invariant self.balance >= 0
    invariant self.balance <= 1000000

    public def deposit(mut self, amount: int64):
        requires amount > 0 and amount <= 1000
        if self.balance + amount <= 1000000:
            self.balance += amount

    public def drain(mut self):
        self.balance -= 1

def main():
    mut a = Account(balance=1)
    a.deposit(5)
    b = Account(balance=-1)
    print(a.balance + b.balance)
"#,
        &["13 class invariant failed", "18 class invariant failed"],
    );
}

#[test]
fn reports_steps_outside_the_modeled_subset() {
    assert_open(
        r#"
def ratio(a: float64, b: float64) -> float64:
    return a / b

def main():
    print(ratio(1.0, 2.0))
"#,
        &["3 division by zero unsupported"],
    );
}

#[test]
fn a_missing_solver_is_au5103() {
    let program = crate::check_source("def main():\n    print(1)\n").expect("checks");
    let module = crate::mir::lower_for_verification(&program);
    let options = Options {
        solver: Some("/nonexistent/z3".to_string()),
        ..Options::default()
    };
    let error = verify_module(&program, &module, &options).expect_err("no solver");
    assert_eq!(error.code, "AU5103");
    let options = Options {
        solver: Some("/bin/sh".to_string()),
        ..Options::default()
    };
    let error = verify_module(&program, &module, &options).expect_err("not a solver");
    assert_eq!(error.code, "AU5103");
}

#[test]
fn the_function_filter_limits_the_report() {
    let options = Options {
        function_filter: Some("double".to_string()),
        ..Options::default()
    };
    let Some(report) = verify_with(
        "def double(x: int64) -> int64:\n    return x * 2\n\ndef main():\n    print(1 + 1)\n",
        &options,
    ) else {
        return;
    };
    assert_eq!(report.functions, 1);
    assert_eq!(report.obligations.len(), 1);
}

#[test]
fn solver_answers_parse() {
    assert_eq!(smt::render_int("(- 5)"), "-5");
    assert_eq!(smt::render_int(" 7 "), "7");
}

#[test]
fn proves_spec_defs_methods_loans_merges_and_list_operations() {
    assert_proved(
        r#"
enum Res:
    Good(int64)
    Bad

class Box:
    v: int64

    def set(mut self, x: int64):
        ensures self.v == x
        self.v = x

spec def small(x: int64) -> bool:
    return -100 <= x and x <= 100

spec def both_small(a: int64, b: int64) -> bool:
    return small(a) and small(b)

def add_small(a: int64, b: int64) -> int64:
    requires both_small(a, b)
    ensures result == a + b
    return a + b

def pick(c: bool, a: int64, b: int64) -> int64:
    ensures result == (a if c else b)
    if c:
        return a
    return b

def chain(x: int64) -> int64:
    requires 0 <= x < 10
    ensures 0 <= result <= 20
    return x * 2

def fancy(xs: list[int64], t: (int64, int64), b: Box) -> int64:
    requires len(xs) > 2 and xs[0] == 1 and t[0] == 3 and b.v == 4
    requires abs(t[1]) < 10 and min(t[1], 0) <= 0 and max(t[1], 0) >= 0
    requires not (xs[1] == 5)
    return xs[0] + t[0] + b.v

def bump_all(xs: mut list[int64]):
    for x in mut xs:
        if x < 100 and x > -100:
            x += 1

def merges(c: bool) -> int64:
    mut t = (1, 2)
    mut r = Res.Bad
    mut p = Box(v=1)
    mut u: int64 | None = None
    if c:
        t = (3, 4)
        r = Res.Good(5)
        p = Box(v=2)
        u = 7
    match r:
        case Good(v):
            assert v == 5
        case Bad:
            pass
    a, b = t
    if u is not None:
        return u + a + b + p.v
    return a + b + p.v

def results(x: int64) -> Result[int64, str]:
    if x > 0:
        return Ok(x)
    return Err("neg")

def uses_try(x: int64) -> Result[int64, str]:
    v = try results(x)
    return Ok(v // 1)

def lists():
    mut xs: list[int64] = []
    assert xs.is_empty()
    xs.append(3)
    xs.append(1)
    assert not xs.is_empty()
    assert 3 in xs or true
    xs.sort()
    assert len(xs) == 2
    xs.reverse()
    xs.clear()
    assert len(xs) == 0
    mut ys = [1, 2, 3]
    ys.extend([4])
    zs = ys.copy()
    assert len(zs) == len(ys)
    print(2 ** 3)
    flags = [true, false]
    assert flags[0]
    assert not flags[1]

def methods():
    mut b = Box(v=0)
    b.set(5)
    assert b.v == 5
    print(add_small(b=2, a=1))

def negs(x: int64) -> int64:
    requires x > -1000 and x < 1000
    y = -x
    return -y

def main():
    print(add_small(1, 2))
    print(pick(true, 1, 2))
    print(chain(3))
    print(fancy([1, 2, 3], (3, 4), Box(v=4)))
    mut xs = [1, 2]
    bump_all(xs)
    print(merges(true))
    print(uses_try(3))
    lists()
    methods()
    print(negs(4))
"#,
    );
}

#[test]
fn every_kind_and_status_has_a_report() {
    let kinds = [
        ObligationKind::Overflow,
        ObligationKind::DivisionByZero,
        ObligationKind::IndexInRange,
        ObligationKind::Assert,
        ObligationKind::Requires,
        ObligationKind::Ensures,
        ObligationKind::LoopInvariant,
        ObligationKind::Decreases,
        ObligationKind::ClassInvariant,
    ];
    let names: Vec<&str> = kinds.iter().map(|kind| kind.name()).collect();
    assert_eq!(
        names,
        [
            "overflow",
            "division by zero",
            "index in range",
            "assert",
            "requires",
            "ensures",
            "loop invariant",
            "decreases",
            "class invariant"
        ]
    );
    let statuses = [
        Status::Proved,
        Status::Unsupported("floats".to_string()),
        Status::Unknown,
        Status::Failed {
            counterexample: Vec::new(),
        },
    ];
    let ranks: Vec<u8> = statuses.iter().map(Status::rank).collect();
    assert_eq!(ranks, [0, 1, 2, 3]);
    let report = Report {
        solver: "z3".to_string(),
        functions: 1,
        cached: 0,
        proved_sites: BTreeSet::new(),
        obligations: statuses
            .into_iter()
            .zip(kinds)
            .map(|(status, kind)| ObligationReport {
                kind,
                function: "f".to_string(),
                span: Span { line: 1, column: 1 },
                message: "the check".to_string(),
                status,
            })
            .collect(),
    };
    assert_eq!(report.proved(), 1);
    assert!(!report.is_proved());
    let codes: Vec<String> = report
        .diagnostics()
        .into_iter()
        .map(|diagnostic| diagnostic.code.to_string())
        .collect();
    assert_eq!(codes, ["AU5104", "AU5102", "AU5101"]);
}

#[test]
fn trace_prints_the_queries() {
    let options = Options {
        trace: true,
        ..Options::default()
    };
    let Some(report) = verify_with(
        "def double(x: int64) -> int64:\n    requires 0 <= x and x < 10\n    return x * 2\n\ndef main():\n    print(double(2))\n",
        &options,
    ) else {
        return;
    };
    assert!(report.is_proved());
}

/// Every run-pass fixture verifies without the verifier failing, whatever
/// it proves. The short limit keeps the sweep fast; a check the solver
/// cannot decide in time is only reported as unknown.
#[test]
fn every_run_pass_fixture_verifies_without_an_internal_error() {
    if smt::SolverCommand::find(None).is_err() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must install an SMT solver"
        );
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-pass");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixtures")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "au"))
        .collect();
    paths.sort();
    let options = Options {
        timeout_ms: 200,
        trace: false,
        ..Options::default()
    };
    let mut verified = 0;
    for path in &paths {
        let Ok(program) = crate::check_path(path) else {
            continue;
        };
        let module = crate::mir::lower_for_verification(&program);
        let report = verify_module(&program, &module, &options)
            .unwrap_or_else(|error| panic!("{}: {error:?}", path.display()));
        assert!(report.proved() <= report.obligations.len());
        verified += 1;
    }
    assert!(verified > 500, "only {verified} fixtures checked");
}

/// A directory of stand-in solver scripts for this test.
#[cfg(unix)]
fn script_dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aura-verify-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[cfg(unix)]
fn script(dir: &std::path::Path, name: &str, body: &str, executable: bool) -> String {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).expect("script");
    let mode = if executable { 0o755 } else { 0o644 };
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("mode");
    path.to_string_lossy().into_owned()
}

#[test]
fn solvers_are_found_by_name_path_and_environment() {
    let empty = std::ffi::OsStr::new("");
    let error = smt::SolverCommand::find_in(None, None, empty).expect_err("nothing on PATH");
    assert!(error.contains("no SMT solver found"), "{error}");
    let error = smt::SolverCommand::find_in(None, Some("z3"), empty).expect_err("not on PATH");
    assert!(error.contains("was not found"), "{error}");
    let error =
        smt::SolverCommand::find_in(Some("/bin/sh"), None, empty).expect_err("not a solver");
    assert!(error.contains("not a supported SMT solver"), "{error}");
    #[cfg(unix)]
    {
        let dir = script_dir("find");
        script(&dir, "cvc5", "exit 0\n", true);
        let found = smt::SolverCommand::find_in(None, Some(""), dir.as_os_str()).expect("cvc5");
        assert_eq!(found.kind, smt::SolverKind::Cvc5);
        script(&dir, "z3", "echo 'Z3 version 0'\n", true);
        let found = smt::SolverCommand::find_in(None, None, dir.as_os_str()).expect("z3");
        assert_eq!(found.kind, smt::SolverKind::Z3);
        assert_eq!(found.version(), "Z3 version 0");
        let named = dir.join("z3").to_string_lossy().into_owned();
        let found = smt::SolverCommand::find_in(None, Some(&named), empty).expect("by path");
        assert_eq!(found.kind, smt::SolverKind::Z3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A stand-in solver that answers every query `unknown` and has no model.
#[cfg(unix)]
const UNKNOWN_SOLVER: &str = r#"while IFS= read -r line; do
  case "$line" in
    *check-sat*) echo unknown ;;
    *get-value*) echo '(error "no model")' ;;
    *echo*) echo "$line" | sed 's/.*echo "\(.*\)".*/\1/' ;;
  esac
done
"#;

#[cfg(unix)]
#[test]
fn solver_failures_and_unknown_answers_are_reported() {
    let dir = script_dir("answers");
    let program = crate::check_source(
        "def double(x: int64) -> int64:\n    return x * 2\n\ndef main():\n    print(double(2))\n",
    )
    .expect("checks");
    let module = crate::mir::lower_for_verification(&program);
    let run = |solver: String| {
        let options = Options {
            solver: Some(solver),
            ..Options::default()
        };
        verify_module(&program, &module, &options)
    };
    // A solver that answers `unknown` leaves the check undecided.
    let report = run(script(&dir, "cvc5", UNKNOWN_SOLVER, true)).expect("runs");
    assert_eq!(open_checks(&report), ["2 overflow unknown"]);
    let command = smt::SolverCommand {
        kind: smt::SolverKind::Z3,
        program: dir.join("cvc5"),
    };
    let mut solver = smt::Solver::start(&command, 100).expect("starts");
    assert_eq!(solver.check(), Ok(smt::SatResult::Unknown));
    assert_eq!(solver.values(&["x".to_string()]), None);
    assert_eq!(solver.values(&[]), Some(Vec::new()));
    // A solver that stops before answering, and one that cannot start.
    let error = run(script(
        &dir,
        "z3",
        "while IFS= read -r line; do case \"$line\" in *check-sat*) exit 0 ;; esac; done\n",
        true,
    ))
    .expect_err("stops");
    assert_eq!(error.code, "AU5103");
    let error = run(script(&dir, "z3-locked", "exit 0\n", false)).expect_err("cannot start");
    assert_eq!(error.code, "AU5103");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_query_the_solver_refuses_is_an_error() {
    let Ok(command) = smt::SolverCommand::find(None) else {
        return;
    };
    let mut solver = smt::Solver::start(&command, 1000).expect("starts");
    solver.send("(assert (undeclared_name))").expect("sent");
    let error = solver.check().expect_err("refused");
    assert!(error.contains("refused"), "{error}");
}

#[test]
fn an_irreducible_body_is_reported_unsupported() {
    use crate::mir::{BasicBlock, Operand, Terminator};
    if smt::SolverCommand::find(None).is_err() {
        return;
    }
    let program =
        crate::check_source("def spin(flag: bool):\n    print(1)\n\ndef main():\n    spin(true)\n")
            .expect("checks");
    let mut module = crate::mir::lower_for_verification(&program);
    let function = module
        .functions
        .iter_mut()
        .find(|function| function.name.ends_with("spin"))
        .expect("spin");
    // Two blocks that jump to each other, both entered from the entry.
    let entry = function.entry.clone();
    let block = |label: &str, terminator: Terminator| BasicBlock {
        label: label.to_string(),
        instructions: Vec::new(),
        terminator,
    };
    function.blocks = vec![
        block(
            &entry,
            Terminator::Branch {
                condition: Operand::Place("flag".to_string()),
                then_label: "a".to_string(),
                else_label: "b".to_string(),
            },
        ),
        block("a", Terminator::Goto("b".to_string())),
        block("b", Terminator::Goto("a".to_string())),
    ];
    let report = verify_module(&program, &module, &Options::default()).expect("runs");
    assert_eq!(open_checks(&report), ["1 assert unsupported"]);
}

#[cfg(unix)]
#[test]
fn a_solver_that_stops_or_prints_garbage_is_an_error() {
    let dir = script_dir("garbage");
    let command = |name: &str, body: &str| smt::SolverCommand {
        kind: smt::SolverKind::Z3,
        program: script(&dir, name, body, true).into(),
    };
    // Bytes that are not UTF-8 cannot be read as an answer.
    let mut solver = smt::Solver::start(
        &command("z3-garbage", "while IFS= read -r line; do case \"$line\" in *check-sat*) printf '\\377\\n' ;; esac; done\n"),
        100,
    )
    .expect("starts");
    let error = solver.check().expect_err("garbage");
    assert!(error.contains("could not read"), "{error}");
    // Writing to a solver that has exited fails.
    let mut solver =
        smt::Solver::start(&command("z3-exit", "read -r line\nexit 0\n"), 100).expect("starts");
    let text = "(assert true)\n".repeat(10_000);
    let mut failed = None;
    for _ in 0..200 {
        if let Err(error) = solver.send(&text) {
            failed = Some(error);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let error = failed.expect("a write to an exited solver fails");
    assert!(error.contains("stopped"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn models_generic_types_function_values_and_boolean_operators() {
    assert_open(
        r#"
class Box[T]:
    value: T

enum Maybe[T]:
    Some(T)
    Nothing

def half(x: int64) -> int64:
    requires x >= 0
    return x // 2

def apply(f: def(int64) -> int64, x: int64) -> int64:
    return f(x)

def bits(a: bool, b: bool) -> bool:
    c = a and b
    d = a or b
    e = a != b
    f = a == b
    return c or d or e or f

def powers(x: int64) -> int64:
    requires 0 <= x and x <= 100
    return x ** 0 + x ** 1 + x ** 2

def unbox(b: Box[int64]) -> int64:
    requires 0 <= b.value and b.value < 1000
    return b.value + 1

def pick(m: Maybe[int64]) -> int64:
    match m:
        case Maybe.Some(v):
            if v > 0 and v < 100:
                return v * 2
            return 0
        case Maybe.Nothing:
            return 1

def main():
    print(apply(half, 4))
    print(bits(true, false))
    print(powers(3))
    print(unbox(Box[int64](value=5)))
    print(pick(Maybe[int64].Some(4)))
"#,
        &["41 requires unsupported"],
    );
}

#[test]
fn contracts_report_what_the_model_does_not_read() {
    assert_open(
        r#"
spec def twice(v: int64) -> int64:
    return v * 2

class P:
    x: int64

def f1(a: int64, b: int64) -> int64:
    requires 0 <= a and a < 100 and 0 <= b and b < 100
    requires a == b or a > b or a < b
    requires (a * 2) // 3 >= 0
    requires 2 * a >= 0
    requires a % 3 != 5
    requires (1 if a > 0 else 2) > 0
    requires (true if a > 0 else false) == (a > 0)
    requires -a <= 0
    requires (~a) < 0
    requires not (a < 0 and b < 0)
    ensures result >= 0
    return a

def f2(xs: list[int64], names: list[str], name: str, p: P, t: (int64, int64)) -> int64:
    requires 3 in xs
    requires (name if len(xs) > 0 else "x") == name
    requires names[0] == "a"
    requires t[0] >= 0
    requires p.x >= 0
    requires xs.contains(3)
    requires str(p.x) == "1"
    requires twice(v=len(xs)) == 2
    return 0

def main():
    print(f1(5, 3))
    print(f2([3], ["a"], "A", P(x=1), (1, 2)))
"#,
        &[
            "22 assert failed",
            "25 index in range failed",
            "34 requires unsupported",
            "35 requires unsupported",
            "35 requires unsupported",
            "35 requires unsupported",
            "35 requires unsupported",
            "35 requires unsupported",
            "35 requires unsupported",
        ],
    );
}

#[test]
fn comparison_chains_are_modeled() {
    assert_open(
        r#"
def f(a: int64, xs: list[int64]) -> int64:
    requires 0 <= a < 100
    requires 100 > a >= 0
    requires a == a != a + 1
    requires 0 <= a in xs
    return a

def main():
    print(f(1, [1]))
"#,
        &["10 requires unsupported"],
    );
}

#[test]
fn models_wildcard_arms_unary_operators_and_field_writes_through_mut() {
    assert_proved(
        r#"
class Bag:
    items: list[int64]
    count: int64

def flip(x: int64) -> int64:
    return ~x

def classify(n: int64) -> int64:
    match n:
        case 0:
            return 10
        case _:
            return 20

def fill(bag: mut Bag, x: int64):
    bag.items.append(x)
    bag.items[0] = x
    bag.count = len(bag.items)

def main():
    print(flip(3))
    print(classify(0))
    mut bag = Bag(items=[1], count=1)
    fill(bag, 4)
    print(bag.count)
"#,
    );
}

#[test]
fn the_proof_cache_answers_unchanged_functions() {
    let dir = std::env::temp_dir().join(format!("aura-verify-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let options = Options {
        cache_dir: Some(dir.clone()),
        ..Options::default()
    };
    let source =
        "def double(x: int64) -> int64:\n    return x * 2\n\ndef main():\n    print(double(2))\n";
    let Some(first) = verify_with(source, &options) else {
        return;
    };
    assert_eq!(first.cached, 0);
    let second = verify_with(source, &options).expect("a solver");
    assert_eq!(second.cached, 2);
    assert_eq!(open_checks(&second), open_checks(&first));
    assert!(matches!(
        &second.obligations[0].status,
        Status::Failed { counterexample } if !counterexample.is_empty()
    ));
    // An entry that does not parse, or does not match the function, is
    // solved again and replaced.
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("cache")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(entries.len(), 2);
    std::fs::write(&entries[0], "not json").expect("corrupt");
    std::fs::write(&entries[1], "[]").expect("wrong length");
    let third = verify_with(source, &options).expect("a solver");
    assert_eq!(third.cached, 0);
    assert_eq!(verify_with(source, &options).expect("a solver").cached, 2);
    // A cache that cannot be written only loses its entries.
    let blocked = dir.join("blocked");
    std::fs::write(&blocked, "a file, not a directory").expect("blocker");
    let options = Options {
        cache_dir: Some(blocked),
        ..Options::default()
    };
    assert_eq!(verify_with(source, &options).expect("a solver").cached, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn undecided_answers_are_not_cached() {
    let dir = script_dir("cache-unknown");
    let options = Options {
        solver: Some(script(&dir, "z3", UNKNOWN_SOLVER, true)),
        cache_dir: Some(dir.join("cache")),
        ..Options::default()
    };
    let program =
        crate::check_source("def double(x: int64) -> int64:\n    return x * 2\n").expect("checks");
    let module = crate::mir::lower_for_verification(&program);
    for _ in 0..2 {
        let report = verify_module(&program, &module, &options).expect("runs");
        assert_eq!(report.cached, 0);
        assert_eq!(open_checks(&report), ["2 overflow unknown"]);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// How many element reads and loans of each function skip their check.
fn unchecked_accesses(module: &crate::mir::MirModule) -> BTreeMap<String, usize> {
    module
        .functions
        .iter()
        .map(|function| {
            let text = format!("{:?}", function.blocks);
            let count = text.matches("Unchecked").count() + text.matches("checked: false").count();
            (crate::mir::public_function_name(&function.name), count)
        })
        .collect()
}

#[test]
fn builds_drop_only_the_bounds_checks_proved_in_range() {
    if smt::SolverCommand::find(None).is_err() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must install an SMT solver"
        );
        return;
    }
    let source = r#"
class Cell:
    value: int64

def middle(xs: list[int64]) -> int64:
    requires len(xs) > 2
    i = len(xs) // 2
    return xs[i]

def last(xs: list[int64]) -> int64:
    requires len(xs) > 0
    i = -1
    return xs[i]

def pick(xs: list[int64], i: int64) -> int64:
    return xs[i]

def bump(cells: mut list[Cell], i: int64):
    requires 0 <= i and i < len(cells)
    view mut cell = cells[i]
    cell.value += 1

def main():
    xs = [1, 2, 3]
    print(middle(xs))
    print(last(xs))
    print(pick(xs, 0))
    mut cells = [Cell(value=1)]
    bump(cells, 0)
    print(cells[0].value)
"#;
    let program = crate::check_source(source).expect("checks");
    let plain = unchecked_accesses(&crate::mir::lower(&program));
    for debug in [false, true] {
        let (module, removed) = crate::mir::lower_with_proved_checks(&program, debug, |module| {
            let options = Options {
                prove_sites: true,
                ..Options::default()
            };
            verify_module(&program, module, &options).map(|report| report.proved_sites)
        })
        .expect("verifies");
        let proved = unchecked_accesses(&module);
        // `middle` reads in range, and `bump` borrows in range; `last`
        // counts from the end, and `pick` may read anywhere.
        assert_eq!(proved["middle"], plain["middle"] + 1, "{proved:?}");
        assert_eq!(proved["last"], plain["last"], "{proved:?}");
        assert_eq!(proved["pick"], plain["pick"], "{proved:?}");
        assert!(proved["bump"] > plain["bump"], "{proved:?}");
        assert!(removed >= 2, "{removed}");
    }
}

#[test]
fn quantified_contracts_are_proved_and_assumed() {
    assert_proved(
        r#"
def cart_total(prices: list[int64]) -> int64:
    requires len(prices) <= 10_000
    requires all([0 <= p and p <= 1_000_000 for p in prices])
    mut total = 0
    for i in range(len(prices)):
        invariant 0 <= total and total <= i * 1_000_000
        total += prices[i]
    return total

def first_positive(xs: list[int64]) -> int64:
    requires exists(x in xs, x > 0)
    requires len(xs) <= 100
    ensures result >= 0
    for i in range(len(xs)):
        if xs[i] > 0:
            return i
    return 0

def squares(n: int64) -> list[int64]:
    requires 0 <= n and n <= 1000
    ensures len(result) == n
    ensures forall(i in range(n), result[i] == i * i)
    mut out: list[int64] = []
    mut i = 0
    while i < n:
        invariant 0 <= i and i <= n
        invariant len(out) == i
        invariant forall(j in range(i), out[j] == j * j)
        out.append(i * i)
        i += 1
    return out

def main():
    print(cart_total([1250, 499, 2000]))
    print(first_positive([-1, 0, 3]))
    print(squares(4))
"#,
    );
}

#[test]
fn quantified_contracts_report_what_fails_and_what_is_not_modeled() {
    assert_open(
        r#"
def evens(n: int64) -> list[int64]:
    requires 0 <= n and n <= 100
    ensures forall(i in range(n), result[i] % 2 == 0)
    mut out: list[int64] = []
    mut i = 0
    while i < n:
        invariant 0 <= i and i <= n and len(out) == i
        invariant forall(j in range(i), out[j] % 2 == 0)
        out.append(i)
        i += 1
    return out

def window(xs: list[int64], lo: int64, hi: int64) -> int64:
    requires all([x >= 0 for x in xs if x > 10])
    requires all([xs[k] < 50 for k in range(lo, hi)])
    requires any([x == y for x in xs for y in xs])
    requires all([len(name) > 0 for name in ["a"]])
    requires all([x > 0 for (x, y) in [(1, 2)]])
    assert all([x < 1000 for x in xs])
    return 0

def main():
    print(evens(1))
    print(window([1, 20], 0, 2))
"#,
        &[
            "9 loop invariant failed",
            "20 assert failed",
            "25 requires unsupported",
            "25 requires unsupported",
            "25 requires unsupported",
        ],
    );
}

#[test]
fn mut_parameters_change_only_what_their_contracts_say() {
    assert_proved(
        r#"
def bump(xs: mut list[int64], k: int64):
    requires 0 <= k and k < len(xs)
    requires xs[k] < 100
    ensures len(xs) == len(old(xs))
    ensures xs[k] == old(xs[k]) + 1
    ensures forall(i in range(len(xs)), i == k or xs[i] == old(xs)[i])
    xs[k] += 1

def push_one(xs: mut list[int64]):
    requires len(xs) < 1000
    ensures len(xs) == len(old(xs)) + 1
    ensures forall(i in range(len(old(xs))), xs[i] == old(xs)[i])
    xs.append(1)

def main():
    mut xs = [1, 2, 3]
    n = 7
    bump(xs, 1)
    assert xs[1] == 3
    assert n == 7
    assert len(xs) == 3
    push_one(xs)
    assert len(xs) == 4
    assert xs[0] == 1 and xs[1] == 3
"#,
    );
}

#[test]
fn recursive_functions_are_proved_to_end() {
    assert_proved(
        r#"
def gcd(a: int64, b: int64) -> int64:
    requires a >= 0 and b >= 0
    decreases b
    ensures result >= 0
    if b == 0:
        return a
    return gcd(b, a % b)

def count(n: int64, step: int64 = 1) -> int64:
    requires 0 <= n and n <= 1000 and step == 1
    decreases n
    ensures 0 <= result and result <= n
    if n == 0:
        return 0
    return 1 + count(n - step)

def main():
    print(gcd(84, 36))
    print(count(10))
"#,
    );
    assert_open(
        r#"
def spin(n: int64, step: int64) -> int64:
    decreases n
    if n <= 0:
        return 0
    return spin(n - step, step)

def main():
    print(spin(3, 1))
"#,
        &[
            "3 decreases failed",
            "6 decreases failed",
            "6 overflow failed",
        ],
    );
}
