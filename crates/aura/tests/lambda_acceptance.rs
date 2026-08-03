use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "aura-lambda-acceptance-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should follow the Unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("lambda acceptance temp directory should exist");
        Self { path }
    }

    fn source(&self, source: &str) -> PathBuf {
        let path = self.path.join("acceptance.au");
        fs::write(&path, source).expect("lambda acceptance source should be writable");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} should succeed; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_mir(source: &Path) -> Output {
    Command::new(aura_bin())
        .arg("run")
        .arg("--backend")
        .arg("mir")
        .arg(source)
        .output()
        .expect("forced MIR lambda acceptance run should start")
}

fn build_direct(source: &Path, binary: &Path) -> Output {
    Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(binary)
        .arg(source)
        .output()
        .expect("direct lambda acceptance build should start")
}

fn assert_both_backends(source: &Path, binary: &Path, expected: &[u8]) {
    let mir = run_mir(source);
    assert_success(&mir, "forced MIR lambda acceptance run");
    assert_eq!(mir.stdout, expected);

    let build = build_direct(source, binary);
    assert_success(&build, "direct lambda acceptance build");
    let direct = Command::new(binary)
        .output()
        .expect("direct lambda acceptance binary should start");
    assert_success(&direct, "direct lambda acceptance run");
    assert_eq!(direct.stdout, expected);
    assert_eq!(direct.stdout, mir.stdout);
}

#[test]
fn closures_and_lambda_powered_apis_match_on_both_backends() {
    let temp = TempDir::new();
    let source = temp.source(
        r#"import control

def make_and_drop_consuming_closure() -> None:
    payload = "never-called"
    unused: def() -> str = lambda: payload
    return None

def invoke_with_environment(environment: own str) -> str:
    worker: def() -> str = lambda: environment
    return worker()

def main() -> int32:
    make_and_drop_consuming_closure()
    print(invoke_with_environment("first-instance"))
    print(invoke_with_environment("second-instance"))

    mut offset: int64 = 10
    add_snapshot: def(int64) -> int64 = lambda value: value + offset
    offset = 99
    print(add_snapshot(1))
    print(add_snapshot(2))

    identity: def(int64) -> int64 = lambda value: value
    print(identity(5))

    payload = "single-use"
    take_payload: def() -> str = lambda: payload
    print(take_payload())

    mut values: list[int64] = [1, 3, 2]
    values.sort(key=lambda value: 0 - value)
    print(values)
    print(values.map(lambda value: value * 2))
    print(values.filter(lambda value: value > 1))

    match own control.retry[int64, str](
        lambda: Result[int64, str].Ok(7),
        max_attempts=3,
        initial_backoff=0ms
    ):
        case Result.Ok(value):
            print(value)
        case Result.Err(error):
            print(error)
            return 1

    task_payload: (str, int64) = ("task", 9)
    task_worker: def() -> str = lambda: f"{task_payload}"
    with TaskGroup() as group:
        task = group.start(task_worker)
        print(task.result_or("missing", timeout=1s))
    return 0
"#,
    );
    let expected = b"first-instance\nsecond-instance\n11\n12\n5\nsingle-use\n[3, 2, 1]\n[6, 4, 2]\n[3, 2]\n7\n(task, 9)\n";

    let binary = temp.path.join("acceptance-direct");
    assert_both_backends(&source, &binary, expected);
}

#[test]
fn imported_lambdas_at_the_same_source_position_keep_distinct_closure_ids() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../aura-compiler/tests/fixtures/run-pass/lambda_imported_closure_ids.au");
    let temp = TempDir::new();
    let binary = temp.path.join("imported-closure-ids-direct");
    assert_both_backends(&fixture, &binary, b"11\n21\n");
}
