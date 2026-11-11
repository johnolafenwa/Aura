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
def power(a: float64, b: float64) -> float64:
    return a ** b

def main():
    print(power(1.0, 2.0))
"#,
        &["3 overflow unsupported"],
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
    assert_eq!(
        smt::render_int("(fp #b0 #b01111111111 #x0000000000000)"),
        "1"
    );
    assert_eq!(
        smt::render_int("(fp #b1 #b10000000 #b10000000000000000000000)"),
        "-3"
    );
    // A value with a long decimal expansion is written with an exponent.
    let fp64 = |value: f64| {
        let bits = value.to_bits();
        format!(
            "(fp #b{} #b{:011b} #b{:052b})",
            bits >> 63,
            (bits >> 52) & 0x7ff,
            bits & ((1 << 52) - 1)
        )
    };
    for (value, rendered) in [
        (5e-324, "5e-324"),
        (-2.5e300, "-2.5e300"),
        (0.00001, "0.00001"),
        (1234567.5, "1234567.5"),
        (1e16, "1e16"),
    ] {
        assert_eq!(smt::render_int(&fp64(value)), rendered);
    }
    assert_eq!(
        smt::render_int("(fp #b0 #b11111110 #b00000000000000000000000)"),
        "1.7014118e38"
    );
    for (special, rendered) in [
        ("(_ +zero 11 53)", "0.0"),
        ("(_ -zero 11 53)", "-0.0"),
        ("(_ +oo 8 24)", "inf"),
        ("(_ -oo 8 24)", "-inf"),
        ("(_ NaN 11 53)", "nan"),
        ("(_ bv5 8)", "(_ bv5 8)"),
        ("(fp #b0 #b01 #b1)", "(fp #b0 #b01 #b1)"),
        ("(fq #b0 #b0 #b0)", "(fq #b0 #b0 #b0)"),
    ] {
        assert_eq!(smt::render_int(special), rendered);
    }
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
        ObligationKind::KeyPresent,
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
            "class invariant",
            "key present"
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
    use std::io::Write;
    use std::process::{Command, Stdio};
    let path = dir.join(name);
    // A child process writes the file. Were it written here, a process
    // another test forks at that moment would hold it open for writing
    // until its own exec, and running the script meanwhile fails with
    // "Text file busy".
    let mut writer = Command::new("sh")
        .args(["-c", "cat > \"$1\" && chmod \"$2\" \"$1\"", "sh"])
        .arg(&path)
        .arg(if executable { "755" } else { "644" })
        .stdin(Stdio::piped())
        .spawn()
        .expect("sh");
    let mut input = writer.stdin.take().expect("piped stdin");
    input
        .write_all(format!("#!/bin/sh\n{body}").as_bytes())
        .expect("script");
    drop(input);
    assert!(writer.wait().expect("sh").success());
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
    // A query stopped at the time limit is undecided too.
    let canceled = UNKNOWN_SOLVER.replace(
        "echo unknown",
        "echo '(error \"line 1 column 7: canceled\")'; echo unknown",
    );
    let report = run(script(&dir, "cvc5", &canceled, true)).expect("runs");
    assert_eq!(open_checks(&report), ["2 overflow unknown"]);
    // An answer after a canceled `push` is for other facts, so it is not
    // taken as a proof.
    let canceled_push = UNKNOWN_SOLVER.replace(
        "echo unknown",
        "echo '(error \"line 9 column 7: push canceled\")'; echo unsat",
    );
    let report = run(script(&dir, "cvc5", &canceled_push, true)).expect("runs");
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

#[cfg(unix)]
#[test]
fn a_solver_that_dies_once_is_restarted_and_one_that_keeps_dying_is_an_error() {
    let dir = script_dir("restart");
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
    // The first process dies at its first query; the second answers.
    let flaky = format!(
        "[ \"$1\" = --version ] && exit 0\nif [ ! -e '{flag}' ]; then : > '{flag}'; while IFS= read -r line; do case \"$line\" in *check-sat*) kill -SEGV $$ ;; esac; done; fi\n{UNKNOWN_SOLVER}",
        flag = dir.join("died").display(),
    );
    let report = run(script(&dir, "z3", &flaky, true)).expect("restarted");
    assert_eq!(open_checks(&report), ["2 overflow unknown"]);
    // A solver that dies every time is reported, with how it ended.
    let error = run(script(
        &dir,
        "z3-dies",
        "while IFS= read -r line; do case \"$line\" in *check-sat*) exit 3 ;; esac; done\n",
        true,
    ))
    .expect_err("keeps dying");
    assert_eq!(error.code, "AU5103");
    assert!(
        error.message.contains("exit status: 3"),
        "{}",
        error.message
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stand-in solver that answers one query `unsat` and exits.
#[cfg(unix)]
const ONE_ANSWER_SOLVER: &str = r#"while IFS= read -r line; do
  case "$line" in
    *check-sat*) echo unsat ;;
    *echo*) echo "$line" | sed 's/.*echo "\(.*\)".*/\1/'; exit 0 ;;
  esac
done
"#;

#[cfg(unix)]
#[test]
fn a_solver_that_dies_between_queries_is_replaced() {
    let dir = script_dir("replace");
    let command = |name: &str, body: &str| {
        smt::SolverCommand::find(Some(&script(&dir, name, body, true))).expect("found")
    };
    // Each process answers one query and exits, so every later step meets
    // a dead solver: closing the query's scope, the next fact, the next
    // query. Whichever step notices, the answers are the same.
    let one_answer = command("z3", ONE_ANSWER_SOLVER);
    let start = || smt::Solver::start(&one_answer, 1000);
    let mut slot = None;
    let mut lane = Lane::new(&mut slot, &start);
    lane.send("(declare-const x Int)").expect("sent");
    for _ in 0..3 {
        assert_eq!(
            lane.ask("(push 1)\n(assert false)\n", None),
            Ok(Status::Proved)
        );
        lane.send("(assert (> x 0))")
            .expect("sent to a fresh solver");
    }
    lane.close();

    // A solver known to be dead before anything more is sent: it read its
    // setup and exited. A process another test forks at that moment holds
    // the dead solver's pipe open until its own exec, and a write then
    // still succeeds, so each case is tried until the write fails.
    let gone = command("z3-gone", "read -r line\n");
    let dead = || {
        let mut dead = smt::Solver::start(&gone, 1000).expect("starts");
        while dead.exit_status().is_none() {}
        assert!(!dead.stopped());
        Some(dead)
    };
    let starts = std::cell::Cell::new(0);
    let counted_start = || {
        starts.set(starts.get() + 1);
        start()
    };
    // The next fact goes to a fresh solver, and so does the query.
    while starts.get() == 0 {
        let mut slot = dead();
        let mut lane = Lane::new(&mut slot, &counted_start);
        lane.send("(declare-const y Int)").expect("replaced");
        assert_eq!(
            lane.ask("(push 1)\n(assert false)\n", None),
            Ok(Status::Proved)
        );
        lane.close();
    }
    // One found dead when its scope is closed is dropped.
    let mut slot = dead();
    while slot.is_some() {
        slot = dead();
        Lane::new(&mut slot, &start).close();
    }
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
            "34 requires unsupported",
            "35 requires unsupported",
            "35 requires unsupported",
        ],
    );
}

#[test]
fn contract_terms_outside_the_model_are_reported_as_unsupported() {
    let report = verify(
        r#"
def g1(a: int64) -> int64:
    requires a ** 2 >= 0
    return a

def g2(t: (int64, int64), a: int64) -> int64:
    requires (a, a)[0] >= 0
    return a

def g3(flags: list[bool]) -> int64:
    requires all(flags)
    return 0

def g4(s: str) -> int64:
    requires s.find("a") != None
    return 0

def g5(d: set[int64]) -> int64:
    requires all([k > 0 for k, v in enumerate(d)])
    return 0

def g6(d: set[int64], xs: list[int64]) -> int64:
    requires all([a == b for a, b in zip(d, xs)])
    return 0

def g7(xs: list[float64], n: int64) -> int64:
    requires all([i >= 0 for i, x in enumerate(xs, n ** 2)])
    return 0

def main():
    print(g1(2))
    print(g2((1, 2), 0))
    print(g3([true]))
    print(g4("A"))
    print(g5({1}))
    print(g6({1}, [1]))
    print(g7([1.0], 1))
"#,
    );
    let Some(report) = report else {
        return;
    };
    assert_eq!(
        open_checks(&report),
        [
            "3 overflow failed",
            "27 overflow failed",
            "27 overflow failed",
            "31 requires unsupported",
            "32 requires unsupported",
            "33 requires unsupported",
            "34 requires unsupported",
            "37 requires unsupported",
        ],
        "{report:#?}"
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
        &[],
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
    // One status more than the entry holds, whichever function it is.
    let held: Vec<Status> =
        serde_json::from_str(&std::fs::read_to_string(&entries[1]).expect("entry")).expect("json");
    let longer = vec![Status::Proved; held.len() + 1];
    std::fs::write(&entries[1], serde_json::to_string(&longer).expect("json"))
        .expect("wrong length");
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
        assert!(removed.bounds >= 2, "{removed:?}");
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

#[test]
fn trait_contracts_hold_for_every_implementation_and_generic_calls_rely_on_them() {
    assert_open(
        r#"
trait Shape:
    def area(self) -> int64:
        ensures result >= 0
    def scaled(self, k: int64) -> int64:
        requires 1 <= k and k <= 10
        ensures result >= 0
        return min(self.area(), 1000) * k

class Square:
    side: int64

class Sheet:
    w: int64

impl Shape for Square:
    def area(self) -> int64:
        return min(max(self.side, 0), 1000) * 2

impl Shape for Sheet:
    def area(self) -> int64:
        return self.w

def first_area[T: Shape](items: list[T]) -> int64:
    ensures result >= 0
    for item in items:
        return item.area()
    return 0

def main():
    print(Square(side=3).scaled(2))
    print(first_area([Square(side=1), Square(side=2)]))
    print(first_area([Sheet(w=4)]))
"#,
        &["4 ensures failed"],
    );
}

#[test]
fn lemmas_are_proved_once_and_relied_on_at_each_call() {
    let source = r#"
spec def tri(n: int64) -> int64:
    return n * (n + 1) // 2

lemma def tri_bounded(n: int64):
    requires 0 <= n and n <= 1000
    decreases n
    ensures 0 <= tri(n) and tri(n) <= 500500
    if n > 0:
        tri_bounded(n - 1)

def sum_to(n: int64) -> int64:
    requires 0 <= n and n <= 1000
    ensures result == tri(n)
    mut total = 0
    mut i = 0
    while i < n:
        invariant 0 <= i and i <= n
        invariant total == tri(i)
        tri_bounded(i)
        i += 1
        total += i
    return total

def main():
    print(sum_to(10))
"#;
    assert_proved(source);
    // A call written as a statement is erased, so a run does not check its
    // `requires`, and a lemma the verifier cannot prove leaves every
    // access checked.
    let erased = r#"
lemma def big(n: int64):
    requires n > 100
    assert n > 50

def main():
    xs = [1, 2]
    big(1)
    print(xs[0])
"#;
    let output = crate::run_source(erased).expect("an erased lemma call does not run");
    assert_eq!(output.stdout, "1\n");
    let options = Options {
        prove_sites: true,
        ..Options::default()
    };
    let Some(report) = verify_with(erased, &options) else {
        return;
    };
    assert_eq!(open_checks(&report), ["8 requires failed"]);
    assert!(report.proved_sites.is_empty());
    let proved = erased.replace("big(1)", "big(101)");
    let report = verify_with(&proved, &options).expect("a solver ran above");
    assert!(report.is_proved());
    // The read, and the lemma's own `assert`.
    assert_eq!(report.proved_sites.len(), 2, "{:?}", report.proved_sites);
}

#[test]
fn a_range_loop_invariant_holds_when_the_loop_ends() {
    assert_proved(
        r#"
def fill(n: int64) -> list[int64]:
    requires 0 <= n and n <= 100
    ensures len(result) == n
    ensures forall(i in range(n), result[i] == i)
    mut out: list[int64] = []
    for i in range(n):
        invariant len(out) == i
        invariant forall(j in range(i), out[j] == j)
        out.append(i)
    return out

def count(a: int64, b: int64) -> int64:
    requires 0 <= a and a <= 100 and 0 <= b and b <= 100
    ensures result == 0 or result == b - a
    mut c = 0
    for i in range(a, b):
        invariant c == i - a
        c += 1
    return c

def main():
    print(fill(3))
    print(count(3, 7))
    print(count(7, 3))
"#,
    );
    // The last check runs with the target at the end of the range.
    assert_open(
        r#"
def steps(n: int64) -> int64:
    requires 0 <= n and n <= 10
    mut s = 0
    for i in range(n):
        invariant s == i
        invariant i < n
        s += 1
    return s

def main():
    print(steps(2))
"#,
        &["7 loop invariant failed"],
    );
}

#[test]
fn lists_of_tuples_and_classes_model_their_elements() {
    assert_proved(
        r#"
class Point:
    x: int64
    y: int64

def first_x(ps: list[Point]) -> int64:
    requires len(ps) > 0 and ps[0].x == 3
    ensures result == 3
    return ps[0].x

def bump(ps: mut list[Point]):
    requires len(ps) > 1
    requires forall(p in ps, 0 <= p.x and p.x < 1000)
    ensures ps[1].x == old(ps[1].x) + 1
    ensures ps[0].y == old(ps[0].y)
    ps[1].x = ps[1].x + 1

def pairs(n: int64) -> list[(int64, bool)]:
    requires 0 <= n and n <= 100
    ensures len(result) == n
    ensures forall(p in result, p[0] >= 0 and p[1])
    mut out: list[(int64, bool)] = []
    for i in range(n):
        invariant len(out) == i
        invariant forall(q in out, q[0] >= 0 and q[1])
        out.append((i, true))
    return out

def sum_x(ps: list[Point]) -> int64:
    requires len(ps) <= 100
    requires forall(p in ps, 0 <= p.x and p.x <= 10)
    ensures 0 <= result and result <= 1000
    mut t = 0
    for i in range(len(ps)):
        invariant 0 <= t and t <= 10 * i
        t += ps[i].x
    return t

def main():
    mut ps = [Point(x=3, y=4), Point(x=5, y=6)]
    print(first_x(ps))
    bump(ps)
    print(ps[1].x)
    print(len(pairs(3)))
"#,
    );
}

#[test]
fn quantifiers_take_several_clauses_and_tuple_targets() {
    assert_proved(
        r#"
def first_le_last(xs: list[int64]) -> bool:
    requires len(xs) > 1
    requires all([xs[i] <= xs[j] for i in range(len(xs)) for j in range(len(xs)) if i <= j])
    ensures result
    return xs[0] <= xs[len(xs) - 1]

def third(xs: list[int64]) -> int64:
    requires len(xs) > 3
    requires forall((i, x) in enumerate(xs), x == i)
    ensures result == 3
    return xs[3]

def ordered(xs: list[int64], ys: list[int64]) -> bool:
    requires len(xs) > 0 and len(ys) > 0
    requires forall((x, y) in zip(xs, ys), x < y)
    ensures result
    return xs[0] < ys[0]

def total(ps: list[(int64, int64)]) -> int64:
    requires len(ps) > 0
    requires forall((a, b) in ps, 0 <= a and a <= 5 and b == 10 - a)
    ensures result == 10
    return ps[0][0] + ps[0][1]

def main():
    print(first_le_last([1, 2, 3]))
    print(third([0, 1, 2, 3]))
    print(ordered([1], [2]))
    print(total([(4, 6)]))
"#,
    );
}

#[test]
fn class_invariants_hold_where_objects_cross_function_boundaries() {
    let source = r#"
class Account:
    balance: int64
    limit: int64 = 1000
    invariant 0 <= self.balance and self.balance <= self.limit
    invariant self.limit <= 1_000_000

    public def headroom(self) -> int64:
        ensures result >= 0
        return self.limit - self.balance

def outside(a: Account) -> int64:
    ensures result >= 0
    return a.headroom()

def make() -> Account:
    return Account(balance=3)

def chain() -> int64:
    ensures result >= 0
    return make().headroom()

def broken() -> int64:
    mut a = Account(balance=3)
    a.balance = -1
    return outside(a)

def main():
    print(outside(Account(balance=10)))
    print(chain())
    xs = [1, 2]
    print(xs[min(outside(Account(balance=1000)), 1)])
"#;
    let options = Options {
        prove_sites: true,
        ..Options::default()
    };
    let Some(report) = verify_with(source, &options) else {
        return;
    };
    assert_eq!(open_checks(&report), ["26 class invariant failed"]);
    // No run checks an object passed to a function, so while that check is
    // open a build removes no check.
    assert!(report.proved_sites.is_empty());
    let fixed = source.replace("a.balance = -1", "a.balance = 4");
    let report = verify_with(&fixed, &options).expect("a solver ran above");
    assert!(report.is_proved(), "{report:#?}");
    assert!(!report.proved_sites.is_empty());
    crate::run_source(&fixed).expect("the program runs");
}

#[test]
fn measures_cover_methods_and_mutual_recursion() {
    assert_proved(
        r#"
def is_even(n: int64) -> bool:
    requires n >= 0
    decreases n
    if n == 0:
        return true
    return is_odd(n - 1)

def is_odd(n: int64) -> bool:
    requires n >= 0
    decreases n
    if n == 0:
        return false
    return is_even(n - 1)

def countdown(k: int64) -> int64:
    requires 0 <= k and k <= 1000
    decreases k
    ensures result == 0
    if k == 0:
        return 0
    return countdown(k - 1)

def start(n: int64) -> int64:
    requires 0 <= n and n <= 10
    decreases n
    ensures result == 0
    # Not a cycle: `countdown` never calls back, so its larger measure is
    # fine here.
    return countdown(100)

class Counter:
    depth: int64

    def down(self, k: int64) -> int64:
        requires k >= 0
        decreases k
        if k == 0:
            return self.depth
        return self.down(k - 1)

def main():
    print(is_even(10))
    print(start(3))
    c = Counter(depth=4)
    print(c.down(3))
"#,
    );
    assert_open(
        r#"
def ping(n: int64) -> int64:
    requires n >= 0
    decreases n
    if n == 0:
        return 0
    return pong(n)

def pong(n: int64) -> int64:
    requires n >= 0
    decreases n
    return ping(n - 1)

def main():
    print(ping(0))
"#,
        &["7 decreases failed", "12 requires failed"],
    );
}

#[test]
fn spec_methods_and_spec_requires_are_unfolded() {
    assert_proved(
        r#"
class Account:
    balance: int64
    limit: int64 = 1000

    spec def headroom(self) -> int64:
        return self.limit - self.balance

    public def deposit(mut self, amount: int64):
        requires 0 <= self.balance and self.balance <= self.limit
        requires amount > 0 and amount <= self.headroom()
        ensures self.headroom() == old(self.headroom()) - amount
        self.balance += amount

spec def ratio(a: int64, b: int64) -> int64:
    requires b > 0
    return a // b

def half(n: int64) -> int64:
    requires 0 <= n and n <= 100
    ensures result == ratio(n, 2)
    return n // 2

def main():
    mut a = Account(balance=10)
    a.deposit(5)
    print(a.headroom())
    print(half(7))
"#,
    );
    assert_open(
        r#"
spec def ratio(a: int64, b: int64) -> int64:
    requires b > 0
    return a // b

def zero(n: int64, d: int64) -> int64:
    requires n >= 0
    ensures ratio(n, d) >= 0 or result == 0
    return 0

def main():
    print(zero(1, 1))
"#,
        &["8 ensures failed", "9 assert failed"],
    );
}

#[test]
fn spec_methods_with_arguments_and_shifted_enumerate_are_modeled() {
    assert_proved(
        r#"
class Box:
    n: int64

    spec def plus(self, extra: int64) -> int64:
        return self.n + extra

    public def grow(mut self, k: int64):
        requires 0 <= self.n and self.n <= 100 and 0 <= k and k <= 100
        ensures self.n == old(self.plus(k))
        self.n = self.plus(k)

def second(xs: list[int64]) -> int64:
    requires len(xs) > 2
    requires forall((i, x) in enumerate(xs, 0), x == i + 1)
    ensures result == 2
    return xs[1]

def pairs(xs: list[int64], ys: list[int64]) -> int64:
    mut n = 0
    for (x, y) in zip(xs, ys):
        if 0 <= x and x <= 10 and 0 <= y and y <= 10 and n < 1000:
            n += x * y
    return n

def main():
    mut b = Box(n=1)
    b.grow(2)
    print(b.n)
    print(second([1, 2, 3]))
    print(pairs([1, 2], [3, 4]))
"#,
    );
    assert_proved(
        r#"
class Node:
    depth: int64

    def count(self, n: int64) -> int64:
        requires n >= 0
        decreases n
        if n == 0:
            return 0
        return (self).count(n - 1)

class Tree:
    root: Node

    def walk(self, n: int64) -> int64:
        requires n >= 0
        decreases n
        if n == 0:
            return self.root.count(3)
        return self.walk(n - 1)

def down(n: int64) -> int64:
    requires n >= 0
    decreases n
    if n == 0:
        return 0
    return (down)(n - 1)

def main():
    t = Tree(root=Node(depth=1))
    print(t.walk(2))
    print(down(3))
"#,
    );
    assert_open(
        r#"
class Box:
    n: int64

    spec def plus(self, extra: int64) -> int64:
        return self.n + extra

def kw(b: Box) -> int64:
    requires b.plus(extra=1) > 0
    return 0

def meth(xs: list[int64]) -> int64:
    requires all([x > 0 for x in xs.copy()])
    return 0

def strs(xs: list[str]) -> int64:
    requires all([len(x) > i for (i, x) in enumerate(xs, start=1)])
    return 0

def shifted(xs: list[int64]) -> int64:
    requires forall((i, x) in enumerate(xs, 1), x == i)
    return 0

def main():
    print(kw(Box(n=1)))
"#,
        &[
            "8 assert failed",
            "17 overflow failed",
            "21 overflow failed",
            "25 requires unsupported",
        ],
    );
}

#[test]
fn trait_implementations_may_weaken_requires_and_strengthen_ensures() {
    let source = r#"
trait Scaler:
    def scale(self, x: int64) -> int64:
        requires 0 <= x and x <= 100
        ensures result >= 0

class Double:
    pad: int64

impl Scaler for Double:
    def scale(self, x: int64) -> int64:
        requires 0 <= x and x <= 1000
        ensures result == 2 * x
        return 2 * x

class Narrow:
    pad: int64

impl Scaler for Narrow:
    def scale(self, x: int64) -> int64:
        requires x == 5
        return x

def through[T: Scaler](s: T) -> int64:
    ensures result >= 0
    return s.scale(50)

def direct(d: Double) -> int64:
    ensures result == 1000
    return d.scale(500)

def main():
    print(through(Double(pad=0)))
    print(direct(Double(pad=0)))
"#;
    assert_open(source, &["21 requires failed"]);
}

#[test]
fn ghost_locals_are_verified_and_erased_when_the_program_runs() {
    let source = r#"
spec def sq(n: int64) -> int64:
    return n * n

def total(xs: list[int64]) -> int64:
    requires len(xs) <= 100
    requires all([0 <= x and x <= 10 for x in xs])
    ensures result <= 1000
    mut s = 0
    ghost mut steps = 0
    for i in range(len(xs)):
        invariant steps == i and 0 <= s and s <= 10 * i
        s += xs[i]
        ghost steps = steps + 1
    assert steps == len(xs)
    return s

def square(n: int64) -> int64:
    requires 0 <= n and n <= 10
    ghost k: int64 = sq(n)
    ghost mut seen = n
    ghost seen += 1
    mut j = 0
    while j < 3:
        invariant 0 <= j and j <= 3 and seen == n + 1 + j and k == n * n
        decreases 3 - j
        ghost seen = seen + 1
        j += 1
    assert seen == n + 4
    return n * n

def main():
    print(total([1, 2, 3]))
    print(square(4))
"#;
    assert_proved(source);
    let program = crate::check_source(source).expect("checks");
    // The program's own lowering has no trace of the ghost locals.
    let plain = format!("{:?}", crate::mir::lower(&program));
    assert!(!plain.contains("ghost"), "{plain}");
    if smt::SolverCommand::find(None).is_ok() {
        let (_, removed) = crate::mir::lower_with_proved_checks(&program, false, |module| {
            let options = Options {
                prove_sites: true,
                ..Options::default()
            };
            verify_module(&program, module, &options).map(|report| report.proved_sites)
        })
        .expect("verifies");
        // The read of `xs[i]` maps across the ghost code around it.
        assert!(removed.bounds >= 1, "{removed:?}");
    }

    // A ghost `assert` no run checks: until it is proved, nothing counts as
    // proved.
    let Some(report) = assert_open(
        r#"
def f(n: int64) -> int64:
    requires 0 <= n and n <= 10
    ghost mut seen = n * 2
    ghost seen = seen + 1
    assert seen == 2 * n + 1
    assert seen > 100
    return n

def main():
    print(f(3))
"#,
        &["7 assert failed"],
    ) else {
        return;
    };
    assert!(report.proved_sites.is_empty());
}

#[test]
fn proved_arithmetic_and_assertions_lose_their_checks_in_a_build() {
    let source = r#"
def total(xs: list[int64]) -> int64:
    requires len(xs) <= 100
    requires all([0 <= x and x <= 10 for x in xs])
    ensures result <= 1000
    mut s = 0
    for i in range(len(xs)):
        invariant 0 <= s and s <= 10 * i
        s += xs[i]
    assert s >= 0
    return s

def avg(a: int64, b: int64) -> int64:
    requires 0 <= a and a <= 1000 and 1 <= b and b <= 10
    return a // b + (a % b) * 2 - -a

def unproved(n: int64, d: int64) -> int64:
    assert n < 5
    return n * 2 + n // d

def main():
    print(total([1, 2, 3]))
    print(avg(17, 4))
    print(unproved(3, 1))
"#;
    if smt::SolverCommand::find(None).is_err() {
        return;
    }
    let program = crate::check_source(source).expect("checks");
    let (module, removed) = crate::mir::lower_with_proved_checks(&program, false, |module| {
        let options = Options {
            prove_sites: true,
            ..Options::default()
        };
        verify_module(&program, module, &options).map(|report| report.proved_sites)
    })
    .expect("verifies");
    assert!(removed.bounds >= 1, "{removed:?}");
    assert!(removed.arithmetic >= 6, "{removed:?}");
    assert!(removed.assertions >= 3, "{removed:?}");
    assert_eq!(
        removed.total(),
        removed.bounds + removed.arithmetic + removed.assertions
    );
    crate::mir::checks::validate_checks(&module).expect("proved traps validate");
    let function = |name: &str| {
        module
            .functions
            .iter()
            .find(|function| function.name == name)
            .expect("the function")
    };
    let traps = |name: &str| {
        let mut traps = Vec::new();
        for block in &function(name).blocks {
            for instruction in &block.instructions {
                if let crate::mir::Instruction::Assign {
                    value:
                        crate::mir::Rvalue::Binary { trap, .. } | crate::mir::Rvalue::Unary { trap, .. },
                    ..
                } = instruction
                {
                    traps.push(*trap);
                }
            }
        }
        traps
    };
    use crate::mir::ArithmeticTrap;
    assert!(
        traps("avg").contains(&ArithmeticTrap::Proved),
        "{:?}",
        traps("avg")
    );
    assert!(!traps("avg")
        .iter()
        .any(|trap| trap.checks_overflow() || trap.checks_divisor()));
    // What is not proved keeps its checks: the `assert`, and `n * 2` and
    // `n // d`.
    let unproved = traps("unproved");
    assert!(!unproved.contains(&ArithmeticTrap::Proved), "{unproved:?}");
    let fails = function("unproved")
        .blocks
        .iter()
        .filter(|block| matches!(block.terminator, crate::mir::Terminator::AssertFail { .. }))
        .count();
    assert!(fails >= 1);
    assert_eq!(
        crate::run_source(source).expect("runs").stdout,
        "6\n23\n9\n"
    );
}

#[test]
fn strings_dicts_and_sets_are_modeled() {
    assert_proved(
        r#"
def greet(name: str) -> str:
    requires len(name) > 0
    ensures len(result) == len(name) + 7
    ensures result == "hello, " + name
    ensures result.starts_with("hello, ") and result.ends_with(name) and "llo" in result
    assert len("a\\b") == 3 and len("\"") == 1 and len("é") == 1
    return "hello, " + name

def order(a: str, b: str) -> bool:
    requires a < b
    ensures result
    ensures not (b <= a) and b > a and b >= a and a != b
    return a.contains("") and a.starts_with("") and b.ends_with("")

def lookup(d: dict[str, int64], k: str) -> int64:
    requires k in d
    requires all([d[v] >= 0 for v in d.keys()])
    ensures result == d[k] and result >= 0
    return d[k]

def counts() -> int64:
    mut d = {"a": 1, "b": 2}
    d["c"] = 3
    d["a"] = 4
    assert len(d) == 3 and not d.is_empty()
    assert "a" in d and not ("z" in d)
    assert d["a"] == 4 and d["b"] == 2
    mut total = 0
    for k in d.keys():
        total = 0 if k == "" else 1
    for v in d.values():
        assert v >= 0 or v < 0
    for (k, v) in d.items():
        assert k in d and d[k] == v
    d.clear()
    assert len(d) == 0 and not ("a" in d)
    return total

def by_number() -> None:
    mut d: dict[int64, bool] = {1: true}
    assert d[1]
    d[2] = false
    assert 2 in d and not d[2] and len(d) == 2
    names = {7: "seven"}
    assert names[7] == "seven" and len(names[7]) == 5

def total(s: set[int64]) -> int64:
    requires 3 in s and len(s) >= 1
    requires all([x > 0 for x in s])
    ensures result == len(s)
    assert 3 in s and not (0 in s)
    mut seen = 0
    for x in s:
        assert x > 0
        seen = x
    return len(s)

def adds(t: mut set[int64]) -> None:
    requires len(t) < 100
    n = len(t)
    t.add(7)
    assert 7 in t and len(t) <= n + 1 and len(t) >= n
    t.remove(7)
    assert not (7 in t)
    t.discard(7)
    t.clear()
    assert len(t) == 0 and t.is_empty()

def words() -> None:
    mut w = {"a", "b"}
    w.add("a")
    assert len(w) == 2 and "b" in w
    same = {"b", "a"}
    assert w == same
    assert {"x": 1} == {"x": 1}
    xs = ["p", "q"]
    assert "q" in xs

def pick(flag: bool) -> str:
    ensures len(result) == 1
    mut s = "a"
    mut d = {1: 1}
    mut t = {1}
    if flag:
        s = "b"
        d[2] = 2
        t.add(2)
    assert 1 in d and 1 in t
    return s

def main():
    print(greet("ada"))
    print(order("a", "b"))
    print(lookup({"x": 1}, "x"))
    print(counts())
    by_number()
    print(total({3, 4}))
    mut s = {1}
    adds(s)
    words()
    print(pick(true))
"#,
    );
    let Some(report) = assert_open(
        r#"
def bad(d: dict[str, int64]) -> int64:
    requires "k" in d
    return d["missing"]

def drop():
    mut t = {1}
    t.remove(2)

def caller(d: dict[str, int64]) -> int64:
    return bad(d)

def main():
    drop()
    print(caller({"k": 1}))
"#,
        &[
            "4 key present failed",
            "8 key present failed",
            "11 requires failed",
        ],
    ) else {
        return;
    };
    assert!(report
        .obligations
        .iter()
        .any(|o| o.message == "key is in the dict"));
}

#[test]
fn collections_outside_the_model_stay_open() {
    assert_open(
        r#"
class P:
    x: int64

def edges(a: str, b: str, d: dict[str, int64], e: dict[str, int64], s: set[str], t: set[str], xs: list[str]) -> bool:
    requires d == e and s == t
    requires "a" in xs and xs.contains("a") and not ("zz" in xs) or true
    requires "b" in "abc" and "a" not in s or true
    requires (a if len(a) > 0 else b) == a or true
    requires a + b == a + b
    requires a <= b or a >= b or a > b
    ensures result == (a >= b)
    return a >= b or (a > b and a <= b)

def unmodeled(m: dict[(int64, int64), int64], q: set[float64], flag: bool) -> int64:
    requires len(m) >= 0
    mut c = {2: P(x=0)}
    if flag:
        c[1] = P(x=1)
    mut w = {(1, 2): 3}
    w[(3, 4)] = 5
    mut z = {1.0}
    z.add(2.0)
    return len(c)

def unsupported(a: str, d: dict[str, P]) -> int64:
    requires "k" in d and d["k"].x >= 0
    requires a.split(",") == []
    return 0

def main():
    print(edges("a", "b", {"x": 1}, {"x": 1}, {"a"}, {"a"}, ["a"]))
    print(unmodeled({(1, 1): 1}, {1.0}, true))
    print(unsupported("ax", {"k": P(x=1)}))
"#,
        &[
            "33 requires unsupported",
            "34 requires unsupported",
            "34 requires unsupported",
        ],
    );
    // Characters past what SMT strings hold leave a literal unmodeled.
    assert_eq!(
        exec::smt_string("a\"\\\u{e9}"),
        Some("\"a\"\"\\u{5c}\\u{e9}\"".to_string())
    );
    assert_eq!(exec::smt_string("\u{10FFFF}"), None);
}

#[test]
fn contracts_read_float_and_str_values() {
    assert_proved(
        r#"
def scale(a: float64, b: float64) -> float64:
    requires a >= 1.0 and a <= 2.0 and b >= 1.0 and b <= 2.0
    ensures result == a * b and result - a * b == 0.0
    ensures result == (a + b) - (a + b) + a * b
    return a * b

def halve(x: float64) -> float64:
    requires x >= 1.0 and x <= 2.0
    ensures result == x / 2.0
    return x / 2.0

def join(a: str, b: str) -> str:
    ensures result == a + b
    ensures len(result) == len(a) + len(b)
    return a + b

def choose(flag: bool, a: str, b: str) -> str:
    ensures result == (a if flag else b)
    if flag:
        return a.clone()
    return b.clone()

def fchoose(flag: bool, x: float64) -> float64:
    requires x >= 1.0 and x <= 2.0
    ensures result == (x if flag else 1.0)
    if flag:
        return x
    return 1.0

def back(x: float32) -> int32:
    requires 0.0 <= x and x <= 10.0
    return x as int32

def main():
    print(scale(1.5, 1.5))
    print(halve(1.5))
    print(join("a", "b"))
    print(choose(true, "a", "b"))
    print(fchoose(false, 1.5))
    print(back(2.5))
"#,
    );
}

#[test]
fn float32_bounds_are_not_taken_from_float64_literals_that_round_away() {
    // `1e-300` is zero and `1e300` is infinite as a float32. Bounds computed
    // from them once made the first, weaker query contradict itself, which
    // proved each of these false claims.
    let Some(report) = assert_open(
        r#"
def div_tiny(x: float32, y: float32, w: float64) -> float32:
    requires w >= 1e-300 and x >= 0.0 and y > 0.0
    ensures x > 0.0
    return x / y

def mul_zero(x: float32, y: float32, w: float64) -> float32:
    requires w <= 1e300 and x >= 0.0 and y == 0.0
    ensures result > 1.0
    return x * y

def sub_huge(x: float32, y: float32, w: float64) -> float32:
    requires w <= 1e300 and x == 0.0 and y > 3.4028235e38
    ensures result > 5.0
    return x - y

def main():
    print(div_tiny(1.0, 5.0, 1.0))
"#,
        &["4 ensures failed", "9 ensures failed", "14 ensures failed"],
    ) else {
        return;
    };
    assert!(report.obligations.len() > 3);
}

#[test]
fn floats_are_ieee_754_values() {
    let Some(report) = assert_open(
        r#"
def half(x: float64) -> float64:
    requires 0.0 <= x and x <= 100.0
    ensures 0.0 <= result and result <= 50.0
    return x / 2.0

def bad(a: float64, b: float64) -> float64:
    return a / b

def conv(n: int64, x: float64) -> int64:
    requires 0 <= n and n <= 1000
    requires 0.0 <= x and x < 10.0
    f = n as float64
    assert f >= 0.0
    g = n.to_float()
    assert g == f
    return x as int64

def small(x: float32) -> float32:
    requires x >= 1.0
    y = x * 2.0
    assert y >= 2.0
    assert -y <= -2.0 and abs(-y) == y
    return y

def mean(xs: list[float64]) -> float64:
    requires len(xs) == 2
    requires all([0.0 <= x and x <= 1.0 for x in xs])
    ensures 0.0 <= result
    return (xs[0] + xs[1]) * 0.5

def narrow(x: float64) -> float32:
    requires 0.0 <= x and x <= 1.0
    ensures result >= 0.0
    ys: list[float32] = [1.5, 2.5]
    assert ys[0] < ys[1]
    return x as float32

def pick(flag: bool, x: float64) -> float64:
    requires x > 1.0
    ensures result > 0.0
    mut y = 1.0
    if flag:
        y = x
    return y

def root(x: float64) -> float64:
    requires x >= 4.0 and x <= 100.0
    return x.sqrt()

def main():
    print(half(3.0))
    print(bad(1.0, 0.5))
    print(conv(3, 2.5))
    print(small(1.5))
    print(mean([0.5, 0.25]))
    print(narrow(0.5))
    print(pick(true, 2.0))
    print(root(9.0))
"#,
        &["8 division by zero failed"],
    ) else {
        return;
    };
    let failed = report
        .obligations
        .iter()
        .find_map(|obligation| match &obligation.status {
            Status::Failed { counterexample } => Some(counterexample.clone()),
            _ => None,
        })
        .expect("a failed check");
    assert!(
        failed
            .iter()
            .any(|(name, value)| name == "b" && value.contains('0')),
        "{failed:?}"
    );
}

#[test]
fn a_call_through_a_function_value_that_names_a_function_is_a_call_of_it() {
    assert_open(
        r#"
def inc(n: int64) -> int64:
    requires n < 100
    ensures result == n + 1
    return n + 1

def apply(f: def(n: int64) -> int64, x: int64) -> int64:
    return f(x)

def direct() -> int64:
    g = inc
    y = g(5)
    assert y == 6
    return g(y)

def wrong() -> int64:
    g = inc
    return g(500)

def escapes() -> int64:
    return apply(inc, 3)


def keep(n: int64) -> int64:
    return n

def chosen(flag: bool) -> int64:
    mut g = keep
    if flag:
        g = inc
    return g(1)

def stored() -> int64:
    fs = [inc]
    return fs[0](2)

def main():
    print(direct())
    print(escapes())
    print(chosen(true))
    print(stored())
"#,
        &[
            "18 requires failed",
            "21 requires unsupported",
            "30 requires unsupported",
            "34 requires unsupported",
        ],
    );
}

#[test]
fn a_task_start_proves_its_target_requires_and_its_result_has_the_ensures() {
    assert_open(
        r#"
def work(n: int64) -> int64:
    requires 0 <= n and n < 1000
    ensures result == n * 2
    return n * 2

def run() -> int64:
    mut out = 0
    with group = TaskGroup():
        task = group.start(work, 21)
        group.start_soon(work, 3)
        match task.result():
            case TaskResult.Ready(value):
                assert value == 42
                out = value
            case _:
                out = 0
    return out

def fallback() -> int64:
    mut v = 0
    with group = TaskGroup():
        task = group.start(work, 5)
        v = task.result_or(10)
    assert v == 10
    return v

def bad() -> int64:
    mut v = 0
    with group = TaskGroup():
        task = group.start(work, 5000)
        v = task.result_or(0)
    return v

def main():
    print(run())
    print(fallback())
"#,
        &["31 requires failed"],
    );
}

#[test]
fn extern_functions_have_trusted_contracts() {
    let dir = script_dir("extern-contract");
    std::fs::write(
        dir.join("Aura.toml"),
        "[package]\nname = \"ffi_spec\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    )
    .expect("manifest");
    std::fs::create_dir_all(dir.join("src")).expect("src");
    let main = dir.join("src/main.au");
    std::fs::write(
        &main,
        r#"extern "C" def abs_value(x: int32) -> int32:
    requires x > -2147483648
    ensures result >= 0
    ensures result == x or result == -x

extern "C" def plain(x: int32) -> int32

extern "C" def fill(data: mut list[uint8], n: int64) -> None:
    requires n <= len(data)
    ensures len(data) >= n

def use_it(x: int32) -> int32:
    requires x > -100
    y = abs_value(x)
    assert y >= 0
    return plain(y)

def bad() -> int32:
    return abs_value(-2147483648)

def buffers() -> int64:
    mut data: list[uint8] = [0, 0, 0]
    fill(data, 2)
    assert len(data) >= 2
    fill(data, 9)
    return len(data)

def main():
    print(use_it(-5))

extern "C" opaque class Store drop close_store
extern "C" class Div:
    quot: int32
    rem: int32

extern "C" def magnitude(x: int32) -> int32 symbol "abs":
    requires x > -2147483648
    ensures result >= 0
extern "C" def div(numerator: int32, denominator: int32) -> Div:
    requires denominator != 0
    ensures result.quot * denominator + result.rem == numerator
extern "C" def open_store(path: cstr, out: mut Store | None) -> int32:
    ensures result >= 0
extern "C" def close_store(store: own Store) -> int32
extern "C" def apply(f: def(value: int64) -> int64, value: int64) -> int64:
    requires value >= 0
extern "C" def total(values: ptr[float64], count: int64) -> float64:
    requires 0 <= count and count <= len(values)

def renamed(x: int32) -> int32:
    requires x > -100
    ensures result >= 0
    return magnitude(x)

def halves(total: int32) -> int32:
    requires total >= 0
    ensures result * 2 + (total - result * 2) == total
    parts = div(total, 2)
    return parts.quot

def by_itself(total: int32) -> int32:
    return div(total, total).rem

def same(value: int64) -> int64:
    return value

def applied(start: int64) -> int64:
    return apply(same, start)

def opened() -> int32:
    ensures result >= 0
    mut slot: Store | None = None
    return open_store(":memory:", slot)

def summed() -> float64:
    values: list[float64] = [1.0, 2.0]
    return total(values, 3)
"#,
    )
    .expect("source");
    let program = crate::check_path(&main).expect("an extern contract checks");
    if smt::SolverCommand::find(None).is_ok() {
        let module = crate::mir::lower_for_verification(&program);
        let report = verify_module(&program, &module, &Options::default()).expect("verifies");
        assert_eq!(
            open_checks(&report),
            // A contract is found by the declared name, not the C symbol,
            // and covers struct results, callbacks, and array pointers.
            [
                "19 requires failed",
                "25 requires failed",
                "62 requires failed",
                "68 requires failed",
                "77 requires failed"
            ],
            "{report:#?}"
        );
    }
    // The contract checks like a function's.
    std::fs::write(
        &main,
        "extern \"C\" def f(x: int32) -> int32:\n    ensures result > y\n\ndef main():\n    print(f(1))\n",
    )
    .expect("source");
    let error = crate::check_path(&main).expect_err("an unknown name");
    assert!(error.message.contains("`y`"), "{}", error.message);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn callable_aliases_carry_contracts() {
    assert_open(
        r#"
type Step = def(n: int64) -> int64:
    requires 0 <= n and n < 100
    ensures result > n

type Boxed = Callable[def(n: int64) -> int64]:
    requires 0 <= n and n < 500
    ensures result >= n

def inc(n: int64) -> int64:
    requires n < 1000
    ensures result == n + 1
    return n + 1

def jump(n: int64) -> int64:
    requires n < 10
    return n + 5

def odd(n: int64) -> int64:
    requires n ** 2 >= 0
    return n + 1

def apply_twice(f: Step, x: int64) -> int64:
    requires 0 <= x and x < 50
    ensures result > x
    y = f(x)
    if y < 100:
        return f(y)
    return y

def again(f: Step) -> int64:
    return apply_twice(f, 1)

def plain(f: def(n: int64) -> int64) -> int64:
    return f(1)

def leak(f: Step) -> int64:
    return plain(f)

def boxed(g: Boxed) -> int64:
    ensures result >= 2
    return g(2)

def main():
    print(apply_twice(inc, 3))
    print(apply_twice(jump, 3))
    print(again(inc))
    print(apply_twice(lambda n: n + 1, 3))
    print(boxed(Boxed(inc)))
    print(leak(inc))
    print(apply_twice(odd, 3))
"#,
        &[
            "20 overflow failed",
            "21 overflow failed",
            "38 requires unsupported",
            "46 requires failed",
            "46 ensures failed",
            "48 requires unsupported",
            "48 overflow failed",
            "51 requires unsupported",
            "51 ensures unsupported",
        ],
    );
    for (source, message) in [
        (
            "type S[T] = def(n: T) -> T:\n    requires true\n",
            "a contract on a generic type alias is not supported yet",
        ),
        (
            "type S = int64:\n    requires true\n",
            "only a callable type such as `Callable[def(n: int64) -> int64]` can have a contract",
        ),
        (
            "type S = def(int64) -> int64:\n    requires true\n",
            "a callable type with a contract names each parameter, as in `def(n: int64)`",
        ),
        (
            "type S = def(n: mut list[int64]) -> int64:\n    requires true\n",
            "a callable type with a contract cannot take a `mut` parameter yet",
        ),
        (
            "type S = def(n: int64) -> int64:\n    ensures result > m\n",
            "unknown name `m`",
        ),
        (
            "type S = def(n: int64) -> int64:\n    decreases n\n",
            "a callable type states only `requires` and `ensures`",
        ),
    ] {
        let error = crate::check_source(&format!("{source}\ndef main():\n    pass\n"))
            .expect_err("a contract the checker refuses");
        assert_eq!(error.message, message, "{source}");
    }
}

#[test]
fn operator_trait_calls_on_type_parameters_rely_on_the_implementations() {
    let source = r#"
class Meters:
    v: int64

impl Ord[Meters] for Meters:
    def lt(self, rhs: Meters) -> bool:
        ensures result == (self.v < rhs.v)
        return self.v < rhs.v
    def le(self, rhs: Meters) -> bool:
        return self.v <= rhs.v
    def gt(self, rhs: Meters) -> bool:
        return self.v > rhs.v
    def ge(self, rhs: Meters) -> bool:
        return self.v >= rhs.v

def smaller[T: Ord[T]](a: T, b: T) -> bool:
    return a < b

def main():
    print(smaller(Meters(v=1), Meters(v=2)))
    print(smaller(1, 2))
"#;
    assert_proved(source);
}

#[test]
fn record_equality_lookups_and_unmodeled_spec_methods() {
    let Some(report) = verify(
        r#"
class P:
    x: int64
    y: int64

    spec def power(self) -> int64:
        return self.x ** 2

def same(a: P) -> bool:
    b = P(x=a.x, y=a.y)
    assert a == b
    return a == b

def at(xs: list[int64]) -> int64:
    requires len(xs) >= 1
    match xs.lookup(0):
        case Lookup.Found(v):
            return v
        case Lookup.Missing:
            assert false
            return 0

def squared(p: P) -> int64:
    requires p.x == 3
    ensures result >= 0
    return p.power()

def main():
    print(same(P(x=1, y=2)))
    print(at([1]))
"#,
    ) else {
        return;
    };
    assert_eq!(open_checks(&report), ["25 ensures failed"], "{report:#?}");
}

#[test]
fn dict_get_and_remove_are_lookups_of_the_modeled_dict() {
    assert_open(
        r#"
def price_or(prices: dict[str, int64], item: str) -> int64:
    requires all([prices[k] >= 0 for k in prices.keys()])
    ensures result >= 0
    match prices.get(item):
        case Lookup.Found(p):
            return p
        case Lookup.Missing:
            return 0

def found_when_present(m: dict[int64, int64], k: int64) -> bool:
    requires k in m
    ensures result
    match m.get(k):
        case Lookup.Found(v):
            return true
        case Lookup.Missing:
            return false

def take(m: dict[int64, int64], k: int64) -> int64:
    requires k in m and len(m) == 3
    ensures result == 2
    mut mm = m.copy()
    mm.remove(k)
    assert not (k in mm)
    return len(mm)

def removed_value(k: int64) -> int64:
    ensures result == 7
    mut m: dict[int64, int64] = {}
    m[k] = 7
    match m.remove(k):
        case Lookup.Found(v):
            assert len(m) == 0
            return v
        case Lookup.Missing:
            return 0

def missing(m: dict[int64, int64], k: int64) -> int64:
    ensures result > 0
    match m.get(k):
        case Lookup.Found(v):
            return 1
        case Lookup.Missing:
            return 0

def main():
    print(price_or({"a": 1}, "a"))
    print(found_when_present({1: 2}, 1))
    print(take({1: 1, 2: 2, 3: 3}, 1))
    print(removed_value(3))
    print(missing({1: 1}, 1))
"#,
        &["40 ensures failed"],
    );
}

#[test]
fn callable_fields_and_results_carry_their_alias_contract() {
    assert_open(
        r#"
type Step = def(n: int64) -> int64:
    requires 0 <= n and n < 100
    ensures result > n

def inc(n: int64) -> int64:
    requires n < 1000
    ensures result == n + 1
    return n + 1

def dec(n: int64) -> int64:
    requires n > -1000
    ensures result == n - 1
    return n - 1

class Stepper:
    step: Step
    count: int64

    def advance(self, n: int64) -> int64:
        requires 0 <= n and n < 100
        ensures result > n
        return self.step(n)

def make() -> Step:
    return inc

def make_bad() -> Step:
    return dec

def pass_on(f: Step) -> Step:
    return f

def unknown(f: def(n: int64) -> int64) -> Step:
    return f

def use_made() -> int64:
    ensures result > 5
    s = make()
    return s(5)

def build() -> Stepper:
    return Stepper(step=inc, count=0)

def build_bad() -> Stepper:
    return Stepper(step=dec, count=0)

def reset(s: mut Stepper):
    s.step = dec

def run(s: Stepper) -> int64:
    ensures result > 3
    return s.step(3)

def main():
    print(use_made())
    print(build().advance(1))
    print(run(build()))
    print(make_bad()(1))
    print(pass_on(inc)(1))
    print(unknown(inc)(1))
    print(build_bad().count)
    mut s = build()
    reset(s)
"#,
        &[
            "29 ensures failed",
            "35 requires unsupported",
            "46 ensures failed",
            "49 ensures failed",
            "61 requires unsupported",
        ],
    );
}

#[test]
fn float_division_and_square_roots_prove_from_their_rounding_facts() {
    let started = std::time::Instant::now();
    let Some(report) = assert_open(
        r#"
def ratio(x: float64, y: float64) -> float64:
    requires 0.0 <= x and x <= 100.0
    requires 1.0 <= y and y <= 10.0
    ensures 0.0 <= result and result <= x
    return x / y

def share(part: float64, total: float64) -> float64:
    requires 0.0 <= part and part <= total and total > 0.0 and total <= 1000000.0
    ensures 0.0 <= result and result <= 1.0
    return part / total

def bounded(x: float64, y: float64) -> float64:
    requires 0.0 <= x and x <= 8.0
    requires 2.0 <= y and y <= 4.0
    ensures 0.25 <= result or x < 1.0
    ensures result <= 4.0
    return x / y

def negative(x: float64, y: float64) -> float64:
    requires -8.0 <= x and x <= 0.0 and 2.0 <= y
    ensures -4.0 <= result and result <= 0.0
    return x / y

def root(x: float64) -> float64:
    requires 1.0 <= x and x <= 100.0
    ensures 1.0 <= result and result <= 10.0 and result <= x
    return x.sqrt()

def small_root(x: float32) -> float32:
    requires 0.0 <= x and x <= 1.0
    ensures x <= result and result <= 1.0
    return sqrt(x)

def mean(total: float64, n: int64) -> float64:
    requires 1 <= n and n <= 1000
    requires 0.0 <= total and total <= 1000.0
    ensures 0.0 <= result and result <= 1000.0
    return total / n.to_float()

def same(x: float64) -> float64:
    requires 1.0 <= x and x <= 2.0
    ensures result < x
    return x / 1.0

def main():
    print(ratio(3.0, 2.0))
    print(share(1.0, 2.0))
    print(bounded(4.0, 2.0))
    print(negative(-4.0, 2.0))
    print(root(4.0))
    print(small_root(0.25))
    print(mean(10.0, 3))
    print(same(1.5))
"#,
        &["43 ensures failed"],
    ) else {
        return;
    };
    assert!(report.obligations.len() > 10);
    // Each of these took the exact query past its time limit.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn steps_outside_the_model_stay_open_with_site_proofs_and_in_the_cache() {
    let dir = std::env::temp_dir().join(format!("aura-verify-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let source = r#"
def invert(a: int64) -> int64:
    return ~a

def power(a: float64, b: float64) -> float64:
    return a ** b

def shift(a: int64, b: int64) -> int64:
    return a << b

def int_power(a: int64, b: int64) -> int64:
    return a ** b

def main():
    print(invert(1))
    print(power(2.0, 2.0))
    print(shift(3, 1))
    print(int_power(3, 1))
"#;
    let plain = Options {
        cache_dir: Some(dir.clone()),
        ..Options::default()
    };
    let Some(first) = verify_with(source, &plain) else {
        return;
    };
    let open = open_checks(&first);
    assert_eq!(
        open,
        [
            "6 overflow unsupported",
            "9 overflow unsupported",
            "12 overflow unsupported"
        ],
        "{first:#?}"
    );
    // An open step is part of the function's cache key, and its answer is
    // never stored as a proof.
    let again = verify_with(source, &plain).expect("a solver");
    assert_eq!(open_checks(&again), open);
    // Asking for site proofs, as a build does, leaves the same steps open.
    let sites = Options {
        prove_sites: true,
        ..Options::default()
    };
    let report = verify_with(source, &sites).expect("a solver");
    assert_eq!(open_checks(&report), open, "{report:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn false_float_claims_fail_with_inputs_the_program_can_have() {
    // Each claim holds for many inputs and fails for many. The search for
    // a failing input can outlast the time limit; an input tried directly
    // shows the failure at once.
    let Some(report) = assert_open(
        r#"
def roots(a: float64) -> float64:
    requires a >= 1.0 and a <= 1000000.0
    ensures result == a
    return a.sqrt() * a.sqrt()

def thirds(a: float64, b: float64) -> float64:
    requires a >= 1.0 and a <= 1000.0 and b >= 1.0 and b <= 1000.0
    ensures result == a
    return a / b * b

def narrow(a: float32) -> float32:
    requires a >= 1.0 and a <= 1000.0
    ensures result == a
    return a / 3.0 * 3.0

def branch(a: float64, n: int64) -> float64:
    requires a >= 1.0 and a <= 1000.0 and n >= 1 and n <= 10
    ensures result == a
    if n > 5:
        return a * 0.1 / 0.1
    return a

def main():
    print(roots(4.0))
    print(thirds(1.0, 2.0))
    print(narrow(3.0))
    print(branch(1.0, 1))
"#,
        &[
            "4 ensures failed",
            "9 ensures failed",
            "14 ensures failed",
            "19 ensures failed",
        ],
    ) else {
        return;
    };
    let values = |line: usize| -> Vec<(String, String)> {
        let obligation = report
            .obligations
            .iter()
            .find(|obligation| obligation.span.line == line)
            .expect("a check on the line");
        let Status::Failed { counterexample } = &obligation.status else {
            panic!("{obligation:?}");
        };
        counterexample.clone()
    };
    let float = |pairs: &[(String, String)], name: &str| -> f64 {
        let (_, value) = pairs
            .iter()
            .find(|(input, _)| input == name)
            .unwrap_or_else(|| panic!("no `{name}` in {pairs:?}"));
        value.parse().expect("a float")
    };
    // Every reported input is inside the `requires` and breaks the claim.
    let a = float(&values(4), "a");
    assert!((1.0..=1e6).contains(&a) && a.sqrt() * a.sqrt() != a, "{a}");
    let pairs = values(9);
    let (a, b) = (float(&pairs, "a"), float(&pairs, "b"));
    assert!((1.0..=1000.0).contains(&a) && (1.0..=1000.0).contains(&b));
    assert!(a / b * b != a, "{a} {b}");
    let a = float(&values(14), "a") as f32;
    assert!((1.0..=1000.0).contains(&a) && a / 3.0 * 3.0 != a, "{a}");
    let pairs = values(19);
    let a = float(&pairs, "a");
    let n: i64 = pairs
        .iter()
        .find(|(input, _)| input == "n")
        .expect("n")
        .1
        .parse()
        .expect("an integer");
    assert!((1.0..=1000.0).contains(&a) && (6..=10).contains(&n));
    assert!(a * 0.1 / 0.1 != a, "{a}");
}

/// A stand-in solver for checks whose float inputs are fixed. It answers
/// `RANGE` when asked whether the function can have the inputs and `ANSWER`
/// when asked a check with them, counts both in `LOG`, and answers every
/// other check `unknown`.
#[cfg(unix)]
const SAMPLED_SOLVER: &str = r#"fixed=0
range=0
while IFS= read -r line; do
  case "$line" in
    *define-fun*) fixed=1 ;;
    *"inputs the function can have"*) range=1 ;;
  esac
  case "$line" in
    *check-sat*)
      if [ "$fixed" = 0 ]; then echo unknown
      elif [ "$range" = 1 ]; then echo range >> "LOG"; echo RANGE
      else echo check >> "LOG"; echo ANSWER; fi
      fixed=0
      range=0 ;;
    *get-value*) echo '(error "no model")' ;;
    *echo*) echo "$line" | sed 's/.*echo "\(.*\)".*/\1/' ;;
  esac
done
"#;

#[cfg(unix)]
#[test]
fn a_float_check_is_tried_with_fixed_inputs_before_the_exact_search() {
    let dir = script_dir("samples");
    let program = crate::check_source(
        "def thirds(a: float64) -> float64:\n    requires a >= 1.0 and a <= 8.0\n    ensures result == a\n    return a / 3.0 * 3.0\n\ndef whole(n: int64) -> int64:\n    requires n >= 0 and n <= 8\n    ensures result == n\n    return n + 0\n\ndef main():\n    print(thirds(3.0))\n    print(whole(1))\n",
    )
    .expect("checks");
    let module = crate::mir::lower_for_verification(&program);
    let run = |name: &str, range: &str, answer: &str, function: &str| {
        let log = dir.join(format!("{name}.log"));
        let body = SAMPLED_SOLVER
            .replace("LOG", &log.to_string_lossy())
            .replace("RANGE", range)
            .replace("ANSWER", answer);
        let options = Options {
            solver: Some(script(&dir, "z3", &body, true)),
            function_filter: Some(function.to_string()),
            ..Options::default()
        };
        let report = verify_module(&program, &module, &options).expect("runs");
        let log = std::fs::read_to_string(&log).unwrap_or_default();
        let count = |kind: &str| log.lines().filter(|line| *line == kind).count();
        (
            open_checks(&report),
            report.obligations.len(),
            count("range"),
            count("check"),
        )
    };
    let all = |open: &[String], status: &str| open.iter().all(|check| check.ends_with(status));
    // A check that fails with its inputs fixed has failed: the exact
    // search, which this solver cannot finish, is not asked. The allowed
    // inputs are found once for the function.
    let (open, checks, ranged, fixed) = run("fails", "sat", "sat", "thirds");
    assert!(checks > 0);
    assert!(all(&open, "failed"), "{open:?}");
    assert_eq!(
        (open.len(), ranged, fixed),
        (checks, float::SAMPLES, checks)
    );
    // Inputs that satisfy a check decide nothing: every allowed row is
    // tried, up to the function's allowance, and the exact search answers.
    let (open, checks, ranged, fixed) = run("holds", "sat", "unsat", "thirds");
    assert!(all(&open, "unknown"), "{open:?}");
    assert_eq!((open.len(), ranged), (checks, float::SAMPLES));
    assert_eq!(
        fixed,
        (checks * float::SAMPLES).min(SAMPLE_CHECKS * float::SAMPLES)
    );
    // A solver that cannot compute a fixed check is not asked for another.
    let (open, checks, ranged, fixed) = run("slow", "sat", "unknown", "thirds");
    assert!(all(&open, "unknown"), "{open:?}");
    assert_eq!((open.len(), ranged, fixed), (checks, float::SAMPLES, 1));
    // When the function can have none of the candidates, no check is tried.
    let (open, checks, ranged, fixed) = run("none", "unsat", "sat", "thirds");
    assert!(all(&open, "unknown"), "{open:?}");
    assert_eq!((open.len(), ranged, fixed), (checks, float::CANDIDATES, 0));
    // Nor when the solver cannot say whether it can have one.
    let (open, _, ranged, fixed) = run("undecided", "unknown", "sat", "thirds");
    assert!(all(&open, "unknown"), "{open:?}");
    assert_eq!((ranged, fixed), (1, 0));
    // A function without float arithmetic has no fixed checks.
    let (_, checks, ranged, fixed) = run("whole", "sat", "sat", "whole");
    assert!(checks > 0);
    assert_eq!((ranged, fixed), (0, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn only_declared_float_inputs_are_fixed() {
    let program = crate::check_source(
        "def mix(a: float64, b: float32, n: int64, xs: list[float64]) -> float64:\n    requires len(xs) > 0 and b >= 0.0 and n >= 0\n    return a / 2.0 + xs[0]\n\ndef plain(n: int64) -> int64:\n    return n\n\ndef main():\n    print(mix(1.0, 1.0, 1, [1.0]))\n    print(plain(1))\n",
    )
    .expect("checks");
    let module = crate::mir::lower_for_verification(&program);
    let cx = VerifyContext::new(&program, &module);
    let preludes = |name: &str| {
        let function = module
            .functions
            .iter()
            .find(|function| function.name.ends_with(name))
            .expect("the function");
        let mut exec = exec::Exec::new(&cx, function);
        let entry = exec.entry_state();
        exec.assume_entry_requires(&entry);
        exec.assume_entry_parameters(&entry);
        exec.run(entry);
        let fixed = sample_preludes(&exec, &[2.0]);
        (fixed, exec.decls.len())
    };
    let (fixed, decls) = preludes("mix");
    assert_eq!(fixed.len(), float::CANDIDATES);
    for text in &fixed {
        // Both float parameters are defined, each in its own type, and
        // everything else is declared as before.
        assert_eq!(text.lines().count(), decls, "{text}");
        let defined: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("(define-fun "))
            .collect();
        assert_eq!(defined.len(), 2, "{text}");
        assert!(defined[0].contains("() Float64 (fp #b"), "{text}");
        assert!(defined[1].contains("() Float32 (fp #b"), "{text}");
    }
    // No float input, no samples.
    assert!(preludes("plain").0.is_empty());
}

#[test]
fn cross_module_contracts_invariants_specs_and_lemmas_are_verified() {
    if smt::SolverCommand::find(None).is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("aura_verify_xmod_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("Aura.toml"),
        "[package]\nname = \"xmod\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    )
    .expect("manifest");
    std::fs::write(
        dir.join("src/bank.au"),
        r#"spec def private_positive(x: int64) -> bool:
    return x > 0

public spec def within_limit(amount: int64, balance: int64) -> bool:
    return private_positive(amount) and amount <= balance

public class Account:
    public balance: int64
    invariant self.balance >= 0

    public spec def ok_balance(self) -> bool:
        return self.balance >= 0

    public def deposit(mut self, amount: int64):
        requires amount > 0
        ensures self.balance == old(self.balance) + amount
        self.balance += amount

public def withdraw(balance: int64, amount: int64) -> int64:
    requires within_limit(amount, balance)
    ensures result == balance - amount
    return balance - amount

public lemma def bogus_bounds(xs: list[int64]):
    ensures len(xs) >= 5
    pass

public extern "C" def clamp_idx(i: int64, n: int64) -> int64 symbol "c_clamp_idx":
    requires n > 0
    ensures 0 <= result and result < n
"#,
    )
    .expect("bank.au");

    let main = dir.join("src/main.au");
    std::fs::write(
        &main,
        r#"import bank
from bank import Account, withdraw, within_limit, bogus_bounds, clamp_idx

def bad_invariant() -> bool:
    bad_acc = Account(balance=-5)
    return bad_acc.ok_balance()

def bad_extern_requires() -> int64:
    return clamp_idx(0, 0)

def bad_fn_requires() -> int64:
    return withdraw(10, 99)

def good_calls() -> int64:
    requires within_limit(3, 10)
    requires bank.within_limit(4, 10)
    mut acc = Account(balance=10)
    acc.deposit(5)
    assert acc.balance == 15
    idx = clamp_idx(99, 4)
    xs = [10, 20, 30, 40]
    return xs[idx] + withdraw(10, 3)

def uses_unproved_imported_lemma(xs: list[int64]) -> int64:
    bogus_bounds(xs)
    return xs[0]

def main():
    print(good_calls())
"#,
    )
    .expect("main.au");

    let program = crate::check_path(&main).expect("cross-module contracts check");
    let module = crate::mir::lower_for_verification(&program);
    let options = Options {
        prove_sites: true,
        ..Default::default()
    };
    let report = verify_module(&program, &module, &options).expect("verifies");
    let open = open_checks(&report);
    assert!(
        open.iter()
            .any(|item| item.starts_with("5 class invariant failed")),
        "imported class invariant violation must fail verification: {open:?}"
    );
    assert!(
        open.iter()
            .any(|item| item.starts_with("9 requires failed")),
        "imported extern with symbol requires violation must fail verification: {open:?}"
    );
    assert!(
        open.iter()
            .any(|item| item.starts_with("12 requires failed")),
        "imported function requires violation must fail verification: {open:?}"
    );
    assert!(
        open.iter().any(|item| item.contains("ensures failed")),
        "unproved imported lemma must surface an open obligation: {open:?}"
    );
    assert!(
        report.proved_sites.is_empty(),
        "an unproved imported lemma must clear proved_sites: {:?}",
        report.proved_sites
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn default_argument_well_definedness_is_verified() {
    assert_open(
        r#"
def boom(x: int64 = 1 // 0) -> int64:
    return x

def main():
    print(boom())
"#,
        &["2 division by zero failed"],
    );
}

#[test]
fn recursive_spec_defs_verify_for_concrete_and_bounded_inputs_and_reject_unbounded() {
    assert_proved(
        r#"
spec def tri_rec(n: int64) -> int64:
    return 0 if n <= 0 else n + tri_rec(n - 1)

def concrete_tri() -> int64:
    ensures result == tri_rec(4)
    return 10

def bounded_tri(n: int64) -> int64:
    requires 0 <= n and n <= 5
    ensures result == tri_rec(n)
    mut i = 0
    mut acc = 0
    while i < n:
        invariant 0 <= i and i <= n
        invariant acc == tri_rec(i)
        decreases n - i
        i += 1
        acc += i
    return acc

def main():
    print(concrete_tri())
    print(bounded_tri(4))
"#,
    );

    assert_open(
        r#"
spec def loop_forever(n: int64) -> int64:
    return loop_forever(n)

def impossible(n: int64) -> int64:
    ensures result == loop_forever(n)
    return 0

def main():
    print(impossible(1))
"#,
        &["6 ensures failed", "7 assert failed"],
    );
}

const TRAIT_OBJECT_SHAPES: &str = r#"
trait Shape:
    def area(self) -> int64:
        ensures result >= 0

    def grow(mut self, by: int64) -> None:
        requires by >= 0 and by < 100

class Square:
    side: int64

class Slab:
    width: int64

impl Shape for Square:
    def area(self) -> int64:
        ensures result == 9
        return 9

    def grow(mut self, by: int64) -> None:
        if self.side < 1000:
            self.side = by

impl Shape for Slab:
    def area(self) -> int64:
        ensures result == 2
        return 2

    def grow(mut self, by: int64) -> None:
        self.width = 0
"#;

#[test]
fn a_trait_object_made_from_a_known_type_keeps_that_type_and_its_value() {
    let source = format!(
        r#"{TRAIT_OBJECT_SHAPES}
def received(shape: dyn Shape) -> int64:
    ensures result >= 0
    return shape.area()

def first(shapes: list[dyn Shape]) -> int64:
    requires len(shapes) > 0
    ensures result >= 0
    return shapes[0].area()

def exact() -> int64:
    ensures result == 9
    shape: dyn Shape = Square(side=3)
    return shape.area()

def grown() -> int64:
    ensures result == 9
    mut shape: dyn Shape = Square(side=3)
    shape.grow(5)
    return shape.area()

def merged(flag: bool) -> int64:
    ensures result == 2
    mut shape: dyn Shape = Square(side=1)
    if flag:
        shape = Square(side=2)
    match shape:
        case Square as square:
            return 2
        case _:
            return 0

def arm(shape: dyn Shape) -> int64:
    ensures result >= 0
    match shape:
        case Square as square:
            return square.area()
        case _:
            return shape.area()

def known() -> int64:
    ensures result == 4
    shape = Square(side=4) as dyn Shape
    match shape:
        case Square as square:
            return square.side
        case _:
            return 0

def changed() -> int64:
    ensures result == 7
    mut shape: dyn Shape = Square(side=3)
    match mut shape:
        case Square as square:
            square.side = 7
        case _:
            pass
    match shape:
        case Slab as slab:
            return slab.width
        case Square as square:
            return square.side
        case _:
            return 0

def main():
    print(received(Slab(width=1)))
    print(first([Square(side=1)]))
    print(exact())
    print(grown())
    print(arm(Slab(width=1)))
    print(known())
    print(changed())
    print(merged(true))
"#
    );
    assert_proved(&source);
}

#[test]
fn a_trait_object_of_an_unknown_type_gives_only_the_trait_contract() {
    // The held type decides the call, so a wrong claim about it stays
    // open. Two paths that hold different types merge to an unknown one,
    // and a trait object the function receives may hold any type.
    let source = format!(
        r#"{TRAIT_OBJECT_SHAPES}
def wrong() -> int64:
    ensures result == 2
    shape: dyn Shape = Square(side=3)
    return shape.area()

def either(flag: bool) -> int64:
    ensures result == 9
    mut shape: dyn Shape = Square(side=1)
    if flag:
        shape = Slab(width=1)
    return shape.area()

def received(shape: dyn Shape) -> int64:
    ensures result == 9
    match shape:
        case Slab as slab:
            return 9
        case _:
            return shape.area()

def main():
    print(wrong())
    print(either(false))
    print(received(Square(side=1)))
"#
    );
    assert_open(
        &source,
        &[
            "33 ensures failed",
            "38 ensures failed",
            "45 ensures failed",
        ],
    );
}

#[test]
fn a_recursive_spec_def_past_its_unfolding_limit_says_so() {
    let Some(report) = verify(
        r#"
spec def tri(n: int64) -> int64:
    return 0 if n <= 0 else n + tri(n - 1)

def total(n: int64) -> int64:
    requires n >= 0
    ensures result == tri(n)
    return 0

def main():
    print(total(3))
"#,
    ) else {
        return;
    };
    let messages: Vec<&str> = report
        .obligations
        .iter()
        .filter(|obligation| obligation.status != Status::Proved)
        .map(|obligation| obligation.message.as_str())
        .collect();
    assert!(
        messages.contains(&"`spec def tri` needs at most 8 levels of recursion here"),
        "{messages:?}"
    );
}

#[test]
fn a_spec_def_unfolds_for_concrete_arguments_of_every_plain_kind() {
    assert_proved(
        r#"
spec def pick(n: int64, flag: bool, label: str, pair: (int64, int64), nothing: None) -> int64:
    return 0 if n >= 0 else pick(n + 1, not flag, label, pair, nothing) + pair[0]

def run() -> int64:
    ensures result == pick(-3, true, "a", (2, 5), None)
    return 6

def main():
    print(run())
"#,
    );
}

#[test]
fn contracts_and_defaults_of_a_nested_module_are_verified_through_its_dotted_path() {
    if smt::SolverCommand::find(None).is_err() {
        assert!(std::env::var_os("CI").is_none());
        return;
    }
    let dir = std::env::temp_dir().join(format!("aura_verify_nested_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("geo")).expect("geo");
    std::fs::write(
        dir.join("geo/limits.au"),
        r#"public spec def small(n: int64) -> bool:
    return n < 10

public def clamp(n: int64, top: int64 = 100) -> int64:
    requires small(n)
    ensures result <= top
    return n if n < top else top

public def boom(x: int64 = 1 // 0) -> int64:
    return x
"#,
    )
    .expect("limits.au");
    let main = dir.join("main.au");
    std::fs::write(
        &main,
        r#"import geo.limits

def good() -> int64:
    requires geo.limits.small(3)
    return geo.limits.clamp(3)

def bad() -> int64:
    return geo.limits.clamp(30)

def defaulted() -> int64:
    return geo.limits.boom()

def main():
    print(good())
"#,
    )
    .expect("main.au");
    let program = crate::check_path(&main).expect("the nested module checks");
    let module = crate::mir::lower_for_verification(&program);
    let report = verify_module(&program, &module, &Options::default()).expect("verifies");
    let open = open_checks(&report);
    assert!(
        open.iter()
            .any(|item| item.starts_with("8 requires failed")),
        "{open:?}"
    );
    assert!(
        !open.iter().any(|item| item.starts_with("5 ")),
        "the call that keeps the contract proves: {open:?}"
    );
    assert!(
        open.iter().any(|item| item.starts_with("11 ")),
        "the imported default is checked at the call: {open:?}"
    );

    // Ordinary code cannot call the imported `spec def`.
    std::fs::write(
        &main,
        "import geo.limits\n\ndef main():\n    print(geo.limits.small(3))\n",
    )
    .expect("main.au");
    let error = crate::check_path(&main).expect_err("a spec def is not callable from code");
    assert!(
        error.message.contains("`geo.limits.small` is a `spec def`"),
        "{error:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
