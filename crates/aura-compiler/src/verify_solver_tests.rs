use super::*;

/// A program with one check the solver proves: `x * 2` cannot overflow
/// when `x` is small.
const SMALL_DOUBLE: &str = "def double(x: int64) -> int64:\n    requires 0 <= x and x < 10\n    return x * 2\n\ndef main():\n    print(1)\n";

/// A program with one check that is not proved: `x * 2` overflows for a
/// large `x`.
const ANY_DOUBLE: &str =
    "def double(x: int64) -> int64:\n    return x * 2\n\ndef main():\n    print(double(2))\n";

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

fn run(source: &str, options: &Options) -> std::result::Result<Report, Diagnostic> {
    let program = crate::check_source(source).expect("the program checks");
    let module = crate::mir::lower_for_verification(&program);
    verify_module(&program, &module, options)
}

/// A directory of stand-in solver scripts for this test.
#[cfg(unix)]
fn script_dir(test: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("aura-verify-solver-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Writes an executable shell script `name` with `body` into `dir`.
#[cfg(unix)]
fn script(dir: &std::path::Path, name: &str, body: &str) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let path = dir.join(name);
    // A child process writes the file, so no process another test forks
    // meanwhile holds it open for writing ("Text file busy").
    let mut writer = Command::new("sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(&path)
        .stdin(Stdio::piped())
        .spawn()
        .expect("sh");
    let mut input = writer.stdin.take().expect("piped stdin");
    input
        .write_all(format!("#!/bin/sh\n[ \"$1\" = --version ] && exit 0\n{body}").as_bytes())
        .expect("script");
    drop(input);
    assert!(writer.wait().expect("sh").success());
    path.to_string_lossy().into_owned()
}

/// A stand-in solver: `$CHECK` is what it does at each `check-sat`, and
/// `$VALUE` at each `get-value`. It echoes the markers the verifier waits
/// for.
#[cfg(unix)]
fn stand_in(check: &str, value: &str) -> String {
    format!(
        r#"while IFS= read -r line; do
  case "$line" in
    *check-sat*) {check} ;;
    *get-value*) {value} ;;
    *echo*) echo "$line" | sed 's/.*echo "\(.*\)".*/\1/' ;;
  esac
done
"#
    )
}

#[cfg(unix)]
#[test]
fn the_watchdog_stops_a_solver_that_does_not_answer() {
    let dir = script_dir("watchdog");
    let command = |name: &str, body: String| smt::SolverCommand {
        kind: smt::SolverKind::Z3,
        program: script(&dir, name, &body).into(),
    };
    // A solver that ignores its own limit is stopped a margin after it.
    let silent = command("z3-silent", stand_in("exec sleep 60", "echo"));
    let mut solver = smt::Solver::start(&silent, 100).expect("starts");
    let started = std::time::Instant::now();
    assert_eq!(solver.check(), Ok(smt::SatResult::Unknown));
    let waited = started.elapsed().as_millis() as u64;
    assert!(
        (100 + smt::WATCHDOG_MARGIN_MS..60_000).contains(&waited),
        "{waited}"
    );
    assert!(solver.stopped());
    // So is one that finds a model and never prints it.
    let mute = command("z3-mute", stand_in("echo sat", "exec sleep 60"));
    let mut solver = smt::Solver::start(&mute, 100).expect("starts");
    assert_eq!(solver.check(), Ok(smt::SatResult::Sat));
    assert_eq!(solver.values(&["x".to_string()]), None);
    assert!(solver.stopped());
    // The verifier reports the check as undecided and goes on.
    let options = Options {
        solver: Some(silent.program.to_string_lossy().into_owned()),
        timeout_ms: 100,
        ..Options::default()
    };
    let report = run(ANY_DOUBLE, &options).expect("runs");
    assert_eq!(open_checks(&report), ["2 overflow unknown"]);
    // A model that never comes leaves a failed check with no inputs.
    let options = Options {
        solver: Some(mute.program.to_string_lossy().into_owned()),
        timeout_ms: 100,
        ..Options::default()
    };
    let report = run(ANY_DOUBLE, &options).expect("runs");
    assert_eq!(open_checks(&report), ["2 overflow failed"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_line_that_is_not_text_is_an_error_and_the_solver_still_answers() {
    let dir = script_dir("garbage");
    let command = smt::SolverCommand {
        kind: smt::SolverKind::Z3,
        program: script(
            &dir,
            "z3-garbage",
            &stand_in(
                "[ -z \"$seen\" ] && printf '\\377\\n'; seen=1; echo unsat",
                "echo",
            ),
        )
        .into(),
    };
    let mut solver = smt::Solver::start(&command, 100).expect("starts");
    let error = solver.check().expect_err("garbage");
    assert!(error.contains("could not read"), "{error}");
    // The reading goes on after the bad line.
    assert_eq!(solver.check(), Ok(smt::SatResult::Unsat));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stand-in Z3 that gives up on a query under the short first limit,
/// and answers `scoped` to one asked in a scope and `alone` to one asked
/// as a problem on its own.
#[cfg(unix)]
fn racing(scoped: &str, alone: &str) -> String {
    stand_in(
        &format!(
            "if [ -n \"$quick\" ]; then echo unknown; elif [ -n \"$scope\" ]; then {scoped}; else {alone}; fi"
        ),
        "echo '(error \"no model\")'",
    )
    .replace(
        "  case \"$line\" in\n",
        &format!(
            "  case \"$line\" in\n    *push*) scope=1 ;;\n    *\":timeout {QUICK_TIMEOUT_MS})\"*) quick=1 ;;\n"
        ),
    )
}

#[cfg(unix)]
#[test]
fn a_check_undecided_at_first_is_raced_in_a_scope_and_as_one_problem() {
    let dir = script_dir("retry");
    let options = |name: &str, scoped: &str, alone: &str| Options {
        solver: Some(script(&dir, name, &racing(scoped, alone))),
        timeout_ms: QUICK_TIMEOUT_MS + 1_000,
        trace: true,
        ..Options::default()
    };
    let open = |options: Options| open_checks(&run(ANY_DOUBLE, &options).expect("runs"));
    // The problem on its own is decided while the scope gives up.
    assert_eq!(
        open(options("z3", "echo unknown", "echo unsat")),
        Vec::<String>::new()
    );
    assert_eq!(
        open(options("z3-sat", "echo unknown", "echo sat")),
        ["2 overflow failed"]
    );
    // The scope decides while the problem on its own is still running.
    let started = std::time::Instant::now();
    assert_eq!(
        open(options("z3-scoped", "echo unsat", "exec sleep 60")),
        Vec::<String>::new()
    );
    assert!(started.elapsed().as_secs() < 30);
    // Neither decides, or one dies and the other gives up.
    assert_eq!(
        open(options("z3-dies", "echo unknown", "exit 3")),
        ["2 overflow unknown"]
    );
    // A limit no longer than the first attempt's asks once, and cvc5 is
    // asked once.
    let once = Options {
        timeout_ms: QUICK_TIMEOUT_MS,
        ..options("z3-once", "echo unknown", "echo unsat")
    };
    assert_eq!(open(once), ["2 overflow unknown"]);
    assert_eq!(
        open(options("cvc5", "echo unknown", "echo unsat")),
        ["2 overflow unknown"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_second_solver_is_the_other_one_or_the_one_named() {
    let empty = std::ffi::OsStr::new("");
    let z3 = smt::SolverCommand {
        kind: smt::SolverKind::Z3,
        program: "z3".into(),
    };
    let error = z3.second_in(None, empty).expect_err("no cvc5");
    assert!(
        error.contains("`cvc5` was not found") && error.contains("--cross-solver"),
        "{error}"
    );
    let cvc5 = smt::SolverCommand {
        kind: smt::SolverKind::Cvc5,
        program: "cvc5".into(),
    };
    let error = cvc5.second_in(None, empty).expect_err("no z3");
    assert!(error.contains("`z3` was not found"), "{error}");
    let error = z3
        .second_in(Some("/nonexistent/cvc5"), empty)
        .expect_err("not there");
    assert!(error.contains("was not found"), "{error}");
    // Without a second solver the run stops before it starts.
    if smt::SolverCommand::find(None).is_ok() {
        let options = Options {
            cross_check: true,
            cross_solver: Some("/nonexistent/cvc5".to_string()),
            ..Options::default()
        };
        let error = run(SMALL_DOUBLE, &options).expect_err("no second solver");
        assert_eq!(error.code, "AU5103");
    }
}

#[test]
fn a_cross_check_with_a_solver_that_agrees_changes_nothing() {
    let Ok(first) = smt::SolverCommand::find(None) else {
        return;
    };
    let options = Options {
        cross_check: true,
        cross_solver: Some(first.program.to_string_lossy().into_owned()),
        trace: true,
        ..Options::default()
    };
    let report = run(SMALL_DOUBLE, &options).expect("runs");
    assert!(report.is_proved(), "{report:#?}");
    assert!(report.disputes.is_empty());
    // A check the first solver refutes is not asked again.
    let report = run(ANY_DOUBLE, &options).expect("runs");
    assert_eq!(open_checks(&report), ["2 overflow failed"]);
    assert!(report.disputes.is_empty());
}

#[cfg(unix)]
#[test]
fn a_second_solver_that_refutes_a_proved_check_is_a_dispute() {
    if smt::SolverCommand::find(None).is_err() {
        return;
    }
    let dir = script_dir("dispute");
    let wrong = script(
        &dir,
        "cvc5",
        &stand_in("echo sat", "echo '(error \"no model\")'"),
    );
    let options = Options {
        cross_check: true,
        cross_solver: Some(wrong.clone()),
        ..Options::default()
    };
    let report = run(SMALL_DOUBLE, &options).expect("runs");
    assert_eq!(open_checks(&report), ["3 overflow unknown"]);
    assert_eq!(report.disputes.len(), 1);
    assert_eq!(report.disputed_by(0), Some(wrong.as_str()));
    assert_eq!(report.undecided(), 1);
    let diagnostics = report.diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "AU5123");
    assert!(
        diagnostics[0].message.contains("solvers disagree"),
        "{}",
        diagnostics[0].message
    );
    // A package run fails on a dispute, whatever it denies.
    assert!(!report.passes(Deny::default()));
    // A second solver that gives up is no dispute.
    let unsure = script(&dir, "cvc5-unsure", &stand_in("echo unknown", "echo"));
    let options = Options {
        cross_solver: Some(unsure),
        ..options
    };
    let report = run(SMALL_DOUBLE, &options).expect("runs");
    assert!(report.is_proved());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Verifies `source` with the solver on this machine, or skips without
/// one.
fn verify(source: &str) -> Option<Report> {
    smt::SolverCommand::find(None).ok()?;
    Some(run(source, &Options::default()).expect("the verifier runs"))
}

#[test]
fn a_sort_reaches_only_the_checks_that_read_its_result() {
    // A short list and a reversed one: the sort's facts reach the checks
    // that read the result.
    let source = "def main():\n    mut xs = [3, 1, 2]\n    xs.sort()\n    assert xs[0] <= xs[1]\n    mut ys: list[int64] = []\n    for i in range(20):\n        ys.append(i % 7)\n    ys.reverse()\n    ys.sort()\n    assert ys.len() == 0 or ys[0] <= ys[ys.len() - 1]\n    print(xs[0])\n";
    let Some(report) = verify(source) else {
        return;
    };
    assert_eq!(open_checks(&report), Vec::<String>::new(), "{report:#?}");
    crate::run_source(source).expect("runs");
    // Wrong claims about the result still fail.
    let wrong = source
        .replace("assert xs[0] <= xs[1]", "assert xs[0] == 3")
        .replace("ys[0] <= ys[ys.len() - 1]", "ys[0] > ys[ys.len() - 1]");
    // The quantified facts of the long list may leave its claim open.
    let report = verify(&wrong).expect("a solver");
    let open = open_checks(&report);
    assert_eq!(open.len(), 2, "{report:#?}");
    assert_eq!(open[0], "4 assert failed");
    assert!(open[1].starts_with("10 assert"), "{open:?}");
}
