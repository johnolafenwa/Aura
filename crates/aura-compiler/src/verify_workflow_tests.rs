use super::*;

fn solver_available() -> bool {
    let found = smt::SolverCommand::find(None).is_ok();
    assert!(
        found || std::env::var_os("CI").is_none(),
        "CI must install an SMT solver for the verifier tests"
    );
    if !found {
        eprintln!("skipping: no SMT solver on PATH");
    }
    found
}

fn verify_with(source: &str, options: &Options) -> Option<Report> {
    if !solver_available() {
        return None;
    }
    let program = crate::check_source(source).expect("the program checks");
    let module = crate::mir::lower_for_verification(&program);
    Some(verify_module(&program, &module, options).expect("the verifier runs"))
}

fn verify(source: &str) -> Option<Report> {
    verify_with(source, &Options::default())
}

/// Each obligation that was not proved, as `line kind in function`.
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

/// A report with one obligation of each given status.
fn report_of(statuses: &[Status]) -> Report {
    Report {
        obligations: statuses
            .iter()
            .map(|status| ObligationReport {
                kind: ObligationKind::Assert,
                function: "f".to_string(),
                span: Span::new(1, 1),
                message: "assertion".to_string(),
                status: status.clone(),
            })
            .collect(),
        ..Report::default()
    }
}

fn failed() -> Status {
    Status::Failed {
        counterexample: Vec::new(),
    }
}

/// A new, empty directory for one test.
fn temp_dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aura-verify-wf-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

// --- the module body -------------------------------------------------

const MODULE_BODY_PROVED: &str = r#"
def half(x: int64) -> int64:
    requires x >= 0
    ensures 0 <= result and result <= x
    return x // 2

mut xs = [1, 2, 3]
mut total = xs[0] + xs[2]
assert half(total) <= total
print(total)
"#;

#[test]
fn module_body_is_verified_like_a_function() {
    let Some(report) = verify(MODULE_BODY_PROVED) else {
        return;
    };
    assert_eq!(open_checks(&report), Vec::<String>::new(), "{report:#?}");
    // `half` and the module body.
    assert_eq!(report.functions, 2);
    assert!(report
        .obligations
        .iter()
        .any(|obligation| obligation.function == MODULE_BODY
            && obligation.kind == ObligationKind::IndexInRange));
    crate::run_source(MODULE_BODY_PROVED).expect("the proved module body runs");
}

#[test]
fn module_body_wrong_statements_still_fail() {
    let source = MODULE_BODY_PROVED
        .replace("xs[2]", "xs[3]")
        .replace("assert half(total) <= total", "print(half(0 - total))");
    let Some(report) = verify(&source) else {
        return;
    };
    assert_eq!(
        open_checks(&report),
        [
            // The sum of an element that is not there is not known to fit.
            "8 overflow in <module>".to_string(),
            "8 index in range in <module>".to_string(),
            "9 requires in <module>".to_string()
        ],
        "{report:#?}"
    );
    assert!(crate::run_source(&source).is_err());
}

#[test]
fn module_body_assert_that_can_fail_is_reported() {
    // `pick` prints, so `n` is not evaluated at build time.
    let source =
        "def pick(x: int64) -> int64:\n    print(x)\n    return x\n\nn = pick(3)\nassert n == 3\n";
    let Some(report) = verify(source) else {
        return;
    };
    // `pick` promises nothing about its result.
    assert_eq!(
        open_checks(&report),
        ["6 assert in <module>"],
        "{report:#?}"
    );
}

#[test]
fn a_build_that_removes_proved_checks_leaves_the_module_body_alone() {
    let options = Options {
        prove_sites: true,
        ..Options::default()
    };
    let Some(report) = verify_with(MODULE_BODY_PROVED, &options) else {
        return;
    };
    assert_eq!(report.functions, 1);
    assert!(report
        .obligations
        .iter()
        .all(|obligation| obligation.function != MODULE_BODY));
}

// --- copies made for generator arguments ----------------------------

const GENERATOR_ARGUMENT: &str = r#"
from iter import Generator

def upto(n: int64) -> Generator[int64]:
    requires 0 <= n and n <= 10
    mut i = 0
    while i < n:
        invariant 0 <= i and i <= n
        yield i
        i += 1

def total(source: own Generator[int64]) -> int64:
    mut t = 0
    for x in source:
        t += x
    return t

def main():
    print(total(upto(3)))
"#;

#[test]
fn generator_argument_copy_is_reported_as_the_function_it_copies() {
    let Some(report) = verify(GENERATOR_ARGUMENT) else {
        return;
    };
    assert!(
        report
            .obligations
            .iter()
            .all(|obligation| !obligation.function.contains("__aura_")),
        "{report:#?}"
    );
    // The sum can overflow in `total` and in its copy: one check, at the
    // place it was written, and no value of a name the checker made up.
    assert_eq!(
        open_checks(&report),
        ["15 overflow in total"],
        "{report:#?}"
    );
    let open = report
        .obligations
        .iter()
        .find(|obligation| obligation.status != Status::Proved)
        .expect("the open check");
    assert!(open.span.column < crate::diag::GENERATED_COLUMN_STRIDE);
    let Status::Failed { counterexample } = &open.status else {
        panic!("the sum fails: {open:#?}");
    };
    assert!(
        counterexample
            .iter()
            .all(|(name, _)| !name.starts_with("__aura_")),
        "{counterexample:?}"
    );
    // The copy's checks are not counted a second time.
    let mut places: Vec<(usize, usize, ObligationKind, &str)> = report
        .obligations
        .iter()
        .map(|obligation| {
            let written = obligation.span.written();
            (
                written.line,
                written.column,
                obligation.kind,
                obligation.message.as_str(),
            )
        })
        .collect();
    let count = places.len();
    places.sort();
    places.dedup();
    assert_eq!(places.len(), count, "{report:#?}");
    // Nor is the copy one more function: the count is the same with a
    // generator the caller keeps in a variable, which gets no copy.
    let kept = GENERATOR_ARGUMENT.replace(
        "    print(total(upto(3)))",
        "    g = upto(3)\n    print(1)\n    print(total(g))",
    );
    let kept = verify(&kept).expect("the solver was found above");
    assert_eq!(report.functions, kept.functions);
}

#[test]
fn reported_name_maps_copies_and_the_module_body() {
    let name = |name: &str| workflow::reported_name(name);
    assert_eq!(name("total"), ("total".to_string(), false));
    assert_eq!(name("pair[int64, str]"), ("pair".to_string(), false));
    assert_eq!(
        name("__aura_with_total__0_upto"),
        ("total".to_string(), true)
    );
    assert_eq!(
        name("tools::__aura_with_total__0_upto"),
        ("tools::total".to_string(), true)
    );
    // Not the shape of a copy's name.
    assert_eq!(
        name("__aura_with_total"),
        ("__aura_with_total".to_string(), false)
    );
    assert_eq!(name("__script"), (MODULE_BODY.to_string(), false));
    assert_eq!(
        name("__aura_const_init::<main>::LIMIT"),
        ("LIMIT".to_string(), false)
    );
}

// --- which outcomes fail a run ---------------------------------------

#[test]
fn deny_reads_its_classes() {
    let mut deny = Deny::default();
    assert!(deny.add("not-modeled"));
    assert_eq!(
        deny,
        Deny {
            not_modeled: true,
            undecided: false
        }
    );
    let mut deny = Deny::default();
    assert!(deny.add("undecided"));
    assert_eq!(
        deny,
        Deny {
            not_modeled: false,
            undecided: true
        }
    );
    assert!(deny.add("all"));
    assert_eq!(deny, Deny::ALL);
    let mut deny = Deny::default();
    assert!(!deny.add("failed"));
    assert_eq!(deny, Deny::default());
}

#[test]
fn a_run_passes_unless_a_check_failed_or_has_a_denied_outcome() {
    let lenient = Deny::default();
    let not_modeled = Deny {
        not_modeled: true,
        undecided: false,
    };
    let undecided = Deny {
        not_modeled: false,
        undecided: true,
    };
    let proved = report_of(&[Status::Proved]);
    let has_failed = report_of(&[Status::Proved, failed()]);
    let has_not_modeled = report_of(&[Status::Unsupported("a step".to_string())]);
    let has_undecided = report_of(&[Status::Unknown]);

    for deny in [lenient, not_modeled, undecided, Deny::ALL] {
        assert!(proved.passes(deny));
        assert!(!has_failed.passes(deny));
    }
    assert!(has_not_modeled.passes(lenient));
    assert!(has_not_modeled.passes(undecided));
    assert!(!has_not_modeled.passes(not_modeled));
    assert!(!has_not_modeled.passes(Deny::ALL));
    assert!(has_undecided.passes(lenient));
    assert!(has_undecided.passes(not_modeled));
    assert!(!has_undecided.passes(undecided));
    assert!(!has_undecided.passes(Deny::ALL));
}

// --- functions that must verify --------------------------------------

const MUST_VERIFY: &str = r#"
def half(x: int64) -> int64:
    requires x >= 0
    ensures result <= x
    return x // 2

def double(x: int64) -> int64:
    return x * 2

def parts(x: float64) -> float64:
    ensures result >= 0.0
    return x ** 2.0

def main():
    print(half(4))
    print(double(4))
    print(parts(1.5))
"#;

#[test]
fn a_function_that_must_verify_reports_its_open_checks() {
    let options = Options {
        must_verify: vec![
            "half".to_string(),
            "double".to_string(),
            "parts".to_string(),
            "missing".to_string(),
        ],
        ..Options::default()
    };
    let Some(report) = verify_with(MUST_VERIFY, &options) else {
        return;
    };
    let marked: Vec<(&str, usize, usize)> = report
        .must_verify
        .iter()
        .map(|marked| (marked.function.as_str(), marked.span.line, marked.open))
        .collect();
    // A check that is not modeled is not proved either: float `**` is
    // not modeled, so both of the checks of `parts` are open.
    assert_eq!(
        marked,
        [("half", 2, 0), ("double", 7, 1), ("parts", 10, 2)],
        "{report:#?}"
    );
    let unverified: Vec<&str> = report
        .unverified()
        .map(|marked| marked.function.as_str())
        .collect();
    assert_eq!(unverified, ["double", "parts"]);
    assert_eq!(
        unmatched_must_verify(&options.must_verify, [&report]),
        ["missing"]
    );

    let diagnostics = report.diagnostics();
    let codes: Vec<&str> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    assert_eq!(codes, ["AU5101", "AU5104", "AU5104", "AU5122", "AU5122"]);
    let last = &diagnostics[3];
    assert_eq!(
        last.message,
        "function `double` marked verified has unproved checks"
    );
    assert_eq!(last.span, Some(Span::new(7, 1)));
    assert!(last.notes[0].contains("checks not proved: 1"), "{last:?}");

    // `parts` has no failed check, and still does not pass.
    let only_parts = Options {
        must_verify: vec!["parts".to_string()],
        function_filter: Some("parts".to_string()),
        ..Options::default()
    };
    let report = verify_with(MUST_VERIFY, &only_parts).expect("the solver was found above");
    assert_eq!(report.failed(), 0);
    assert!(!report.passes(Deny::default()));
}

#[test]
fn a_function_that_must_verify_and_does_passes() {
    let options = Options {
        must_verify: vec!["half".to_string()],
        function_filter: Some("half".to_string()),
        ..Options::default()
    };
    let Some(report) = verify_with(MUST_VERIFY, &options) else {
        return;
    };
    assert_eq!(report.unverified().count(), 0);
    assert!(report.passes(Deny::ALL));
    assert!(report
        .diagnostics()
        .iter()
        .all(|diagnostic| diagnostic.code != "AU5122"));
    assert!(unmatched_must_verify(&options.must_verify, [&report]).is_empty());
    // Without the option nothing is marked.
    let report = verify(MUST_VERIFY).expect("the solver was found above");
    assert!(report.must_verify.is_empty());
}

// --- a run over the files of a package -------------------------------

const UTIL: &str = "public def double(x: int64) -> int64:\n    return x * 2\n\n\
public def clamp(x: int64) -> int64:\n    ensures 0 <= result and result <= 9\n    \
if x < 0:\n        return 0\n    if x > 9:\n        return 9\n    return x\n";

const MAIN: &str = "import util\nimport math\n\n\
def main():\n    print(util.clamp(12))\n    print(math.isqrt(9))\n";

/// A package with a module whose function can overflow, and an entry
/// module that imports it.
fn write_package(test: &str) -> std::path::PathBuf {
    let dir = temp_dir(test);
    std::fs::create_dir_all(dir.join("src")).expect("create src");
    std::fs::write(
        dir.join("Aura.toml"),
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    )
    .expect("write the manifest");
    std::fs::write(dir.join("src").join("util.au"), UTIL).expect("write util.au");
    std::fs::write(dir.join("src").join("main.au"), MAIN).expect("write main.au");
    dir
}

#[test]
fn package_source_root_is_src_when_there_is_one() {
    let dir = write_package("root");
    assert_eq!(package_source_root(&dir), dir.join("src"));
    assert_eq!(package_source_root(&dir.join("src")), dir.join("src"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_single_file_run_names_the_imported_modules_it_did_not_check() {
    if !solver_available() {
        return;
    }
    let dir = write_package("unchecked");
    let report = crate::verify_path(&dir.join("src").join("main.au"), &Options::default())
        .expect("main.au verifies");
    // `double` in `util` can overflow, and this run does not look: the
    // standard library is not counted.
    assert!(report.is_proved(), "{report:#?}");
    assert_eq!(report.unchecked_modules, ["util"]);
    let report = crate::verify_path(&dir.join("src").join("util.au"), &Options::default())
        .expect("util.au verifies");
    assert!(report.unchecked_modules.is_empty());
    assert_eq!(open_checks(&report), ["2 overflow in double"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_package_run_verifies_every_module_once() {
    if !solver_available() {
        return;
    }
    let dir = write_package("files");
    let src = dir.join("src");
    std::fs::write(src.join("broken.au"), "def main():\n    return nope\n").expect("broken.au");
    let paths = [
        src.join("broken.au"),
        src.join("main.au"),
        src.join("util.au"),
    ];
    let options = Options {
        must_verify: vec!["clamp".to_string(), "main".to_string(), "gone".to_string()],
        ..Options::default()
    };
    let package = verify_files(&paths, &options);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(package.files.len(), 3);
    assert_eq!(package.files[2].path, paths[2]);
    assert_eq!(package.files[2].source, UTIL);
    let error = package.files[0].result.as_ref().expect_err("broken.au");
    assert_eq!(error.code, "AU2001", "{error:?}");
    assert_eq!(package.reports().count(), 2);

    let totals = package.totals();
    assert_eq!(totals.files, 3);
    assert_eq!(totals.errors, 1);
    // `main`, `double` and `clamp`.
    assert_eq!(totals.functions, 3);
    assert_eq!(totals.failed, 1);
    assert_eq!(totals.not_modeled, 0);
    assert_eq!(totals.undecided, 0);
    assert_eq!(totals.cached, 0);
    assert_eq!(totals.proved + totals.failed, totals.checks);
    assert!(totals.proved >= 2, "{totals:?}");
    assert!(!package.passes(Deny::default()));
    assert_eq!(
        unmatched_must_verify(&options.must_verify, package.reports()),
        ["gone"]
    );

    // A file that does not compile fails the run on its own.
    let only_broken = PackageReport {
        files: vec![package.files[0].clone()],
    };
    assert!(!only_broken.passes(Deny::default()));
    // The proved file passes alone.
    let only_main = PackageReport {
        files: vec![package.files[1].clone()],
    };
    assert!(only_main.passes(Deny::ALL));
    assert_eq!(only_main.totals().errors, 0);
}

#[test]
fn a_package_run_stops_at_a_missing_solver() {
    let dir = write_package("no-solver");
    let src = dir.join("src");
    let options = Options {
        solver: Some(src.join("no-such-solver").display().to_string()),
        ..Options::default()
    };
    let package = verify_files(&[src.join("main.au"), src.join("util.au")], &options);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(package.files.len(), 1);
    let error = package.files[0].result.as_ref().expect_err("no solver");
    assert_eq!(error.code, "AU5103");
    assert!(!package.passes(Deny::default()));
    assert_eq!(package.totals().errors, 1);
}
