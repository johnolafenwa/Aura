use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should follow the Unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("retry test temp directory should exist");
        Self { path }
    }

    fn source(&self, label: &str, source: &str) -> PathBuf {
        let path = self.path.join(format!("{label}.au"));
        fs::write(&path, source).expect("retry test source should be writable");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn command_output_with_timeout(
    mut command: Command,
    timeout: Duration,
    context: &str,
) -> (Output, Duration) {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("{context}: failed to start: {error}"));
    loop {
        if child
            .try_wait()
            .unwrap_or_else(|error| panic!("{context}: failed to poll: {error}"))
            .is_some()
        {
            let output = child
                .wait_with_output()
                .unwrap_or_else(|error| panic!("{context}: failed to collect output: {error}"));
            return (output, started.elapsed());
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .unwrap_or_else(|error| panic!("{context}: failed to collect timeout: {error}"));
            panic!(
                "{context} timed out after {timeout:?}; stdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn mir_run(source: &Path, timeout: Duration) -> (Output, Duration) {
    let mut command = Command::new(aura_bin());
    command.arg("run").arg("--backend").arg("mir").arg(source);
    command_output_with_timeout(command, timeout, "control.retry MIR run")
}

fn direct_binary(temp: &TempDir, label: &str, source: &Path) -> PathBuf {
    let binary = temp.path.join(format!("{label}-direct"));
    let mut command = Command::new(aura_bin());
    command
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&binary)
        .arg(source);
    let (build, _) = command_output_with_timeout(
        command,
        Duration::from_secs(90),
        "control.retry direct build",
    );
    assert!(
        build.status.success(),
        "control.retry direct build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    binary
}

fn direct_run(binary: &Path, timeout: Duration) -> (Output, Duration) {
    command_output_with_timeout(Command::new(binary), timeout, "control.retry direct run")
}

fn assert_success(output: &Output, backend: &str) {
    assert!(
        output.status.success(),
        "{backend} should succeed; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output, backend: &str) {
    assert!(
        !output.status.success(),
        "{backend} should fail, but stdout was:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn escaped_aura_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[test]
fn retry_reaches_a_later_success_and_doubles_delays_on_both_backends() {
    let temp = TempDir::new("aura-control-retry-later-success");
    let state = escaped_aura_path(&temp.path.join("state.txt"));
    let source = format!(
        r#"
import control
import fs
import sys

def store_state(value: str) -> None:
    match fs.write_string("{state}", value):
        case Result.Ok(_):
            pass
        case Result.Err(_):
            print("state-write-failed")

def flaky_worker() -> Result[list[str], str]:
    print(f"attempt {{sys.monotonic_time_ms()}}")
    match own fs.read_to_string("{state}"):
        case Result.Ok(value):
            if value == "one":
                store_state("two")
                return Result.Err("first")
            if value == "two":
                store_state("three")
                return Result.Err("second")
            return Result.Ok(["owned", value])
        case Result.Err(_):
            return Result.Err("state-read-failed")

def main() -> int32:
    store_state("one")
    match own control.retry[list[str], str](
        initial_backoff=20ms,
        worker=flaky_worker,
        max_attempts=3
    ):
        case Result.Ok(values):
            for value in own values:
                print(value)
        case Result.Err(error):
            print(error)
            return 1
    return 0
"#
    );
    let source_path = temp.source("later-success", &source);

    let (mir, _) = mir_run(&source_path, Duration::from_secs(10));
    assert_success(&mir, "MIR");
    assert_retry_timing_and_output(&mir.stdout, "MIR");

    let binary = direct_binary(&temp, "later-success", &source_path);
    let (direct, _) = direct_run(&binary, Duration::from_secs(10));
    assert_success(&direct, "direct");
    assert_retry_timing_and_output(&direct.stdout, "direct");
}

fn assert_retry_timing_and_output(stdout: &[u8], backend: &str) {
    let stdout = String::from_utf8_lossy(stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        5,
        "{backend} should make exactly three attempts and print the owned result:\n{stdout}"
    );
    let timestamps = lines[..3]
        .iter()
        .map(|line| {
            line.strip_prefix("attempt ")
                .unwrap_or_else(|| panic!("{backend} emitted an unexpected attempt line: {line}"))
                .parse::<i64>()
                .unwrap_or_else(|error| panic!("{backend} emitted an invalid timestamp: {error}"))
        })
        .collect::<Vec<_>>();
    assert!(
        timestamps[1] - timestamps[0] >= 15,
        "{backend} skipped the initial 20ms retry delay: {timestamps:?}"
    );
    assert!(
        timestamps[2] - timestamps[1] >= 35,
        "{backend} did not double the second retry delay to 40ms: {timestamps:?}"
    );
    assert_eq!(&lines[3..], ["owned", "three"]);
}

#[test]
fn specialized_retry_function_value_preserves_retry_semantics_on_both_backends() {
    let temp = TempDir::new("aura-control-retry-function-value");
    let source = r#"
import control

def worker() -> Result[int32, str]:
    print("attempt")
    return Result.Ok(42)

def main() -> int32:
    retry = control.retry[int32, str]
    match retry(worker, max_attempts=2, initial_backoff=0ms):
        case Result.Ok(value):
            print(value)
        case Result.Err(error):
            print(error)
            return 1
    return 0
"#;
    let source_path = temp.source("function-value", source);
    let expected = "attempt\n42\n";

    let (mir, _) = mir_run(&source_path, Duration::from_secs(5));
    assert_success(&mir, "MIR");
    assert_eq!(String::from_utf8_lossy(&mir.stdout), expected);

    let binary = direct_binary(&temp, "function-value", &source_path);
    let (direct, _) = direct_run(&binary, Duration::from_secs(5));
    assert_success(&direct, "direct");
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected);
}

#[test]
fn retry_validates_arguments_before_invoking_the_worker_on_both_backends() {
    let cases = [
        (
            "zero-attempts",
            "max_attempts=0, initial_backoff=0ms",
            &["AU4003", "max_attempts", "at least 1"][..],
        ),
        (
            "negative-backoff",
            "max_attempts=1, initial_backoff=Duration.ms(-1)",
            &["AU4001", "initial_backoff", "cannot be negative"][..],
        ),
        (
            "unrepresentable-backoff",
            "max_attempts=1, initial_backoff=Duration.seconds(9223372036854775807)",
            &["AU4001", "initial_backoff", "host timer range"][..],
        ),
    ];

    for (label, arguments, expected_diagnostic) in cases {
        let temp = TempDir::new(&format!("aura-control-retry-{label}"));
        let marker = temp.path.join("worker-was-called");
        let marker_source = escaped_aura_path(&marker);
        let source = format!(
            r#"
import control
import fs

def worker() -> Result[int32, str]:
    match fs.write_string("{marker_source}", "called"):
        case Result.Ok(_):
            pass
        case Result.Err(_):
            pass
    return Result.Err("worker-result")

def main() -> int32:
    print(control.retry[int32, str](worker=worker, {arguments}))
    return 0
"#
        );
        let source_path = temp.source(label, &source);

        let (mir, _) = mir_run(&source_path, Duration::from_secs(5));
        assert_failure(&mir, "MIR");
        assert_diagnostic_contains(&mir, expected_diagnostic, "MIR");
        assert!(
            !marker.exists(),
            "MIR invoked the retry worker before validating {label}"
        );

        let binary = direct_binary(&temp, label, &source_path);
        let _ = fs::remove_file(&marker);
        let (direct, _) = direct_run(&binary, Duration::from_secs(5));
        assert_failure(&direct, "direct");
        assert_diagnostic_contains(&direct, expected_diagnostic, "direct");
        assert!(
            !marker.exists(),
            "direct invoked the retry worker before validating {label}"
        );
    }
}

fn assert_diagnostic_contains(output: &Output, needles: &[&str], backend: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    for needle in needles {
        assert!(
            stderr.contains(needle),
            "{backend} diagnostic should contain `{needle}`:\n{stderr}"
        );
    }
}

#[test]
fn retry_propagates_worker_traps_without_retrying_on_both_backends() {
    let temp = TempDir::new("aura-control-retry-trap");
    let source = r#"
import control

def trapping_worker() -> Result[int32, str]:
    print("trap-attempt")
    return Result.Ok(1 // 0)

def main() -> int32:
    print(control.retry[int32, str](trapping_worker, max_attempts=3))
    return 0
"#;
    let source_path = temp.source("trap", source);

    let (mir, _) = mir_run(&source_path, Duration::from_secs(5));
    assert_retry_trap(&mir, "MIR");

    let binary = direct_binary(&temp, "trap", &source_path);
    let (direct, _) = direct_run(&binary, Duration::from_secs(5));
    assert_retry_trap(&direct, "direct");
}

fn assert_retry_trap(output: &Output, backend: &str) {
    assert_failure(output, backend);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "trap-attempt\n",
        "{backend} should invoke a trapping worker exactly once"
    );
    assert_diagnostic_contains(
        output,
        &["AU4004", "division by zero", "trapping_worker"],
        backend,
    );
}

#[test]
fn retry_propagates_task_cancellation_instead_of_returning_the_last_error() {
    let temp = TempDir::new("aura-control-retry-cancel");
    let source = r#"
import control

def sleeping_worker() -> Result[int32, str]:
    print("worker-start")
    sleep(60m)
    print("worker-resumed")
    return Result.Err("must-not-escape")

def invoke_retry() -> Result[int32, str]:
    return control.retry[int32, str](
        sleeping_worker,
        max_attempts=3,
        initial_backoff=1s
    )

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(invoke_retry)
        sleep(20ms)
        group.cancel()
        match own task.result(timeout=1s):
            case TaskResult.Ready(Result.Ok(value)):
                print(value)
            case TaskResult.Ready(Result.Err(error)):
                print(f"escaped-{error}")
            case TaskResult.Error(message):
                print(f"error-{message}")
            case TaskResult.TimedOut:
                print("timed-out")
            case TaskResult.Cancelled:
                print("cancelled")
    return 0
"#;
    let source_path = temp.source("cancel", source);
    let expected = "worker-start\nworker-resumed\ncancelled\n";

    let (mir, _) = mir_run(&source_path, Duration::from_secs(5));
    assert_success(&mir, "MIR");
    assert_eq!(String::from_utf8_lossy(&mir.stdout), expected);

    let binary = direct_binary(&temp, "cancel", &source_path);
    let (direct, _) = direct_run(&binary, Duration::from_secs(5));
    assert_success(&direct, "direct");
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected);
}

#[test]
fn retry_rejects_nonzero_arity_and_non_result_workers_with_teaching_diagnostics() {
    let cases = [
        (
            "argument-worker",
            r#"
def invalid_worker(value: int32) -> Result[int32, str]:
    return Result.Ok(value)
"#,
            "invalid_worker",
            "expected `def() -> Result[T, E]`",
            "def(int32) -> Result[int32, str]",
        ),
        (
            "defaulted-argument-worker",
            r#"
def invalid_worker(value: int32 = 1) -> Result[int32, str]:
    return Result.Ok(value)
"#,
            "invalid_worker",
            "expected `def() -> Result[T, E]`",
            "def(int32) -> Result[int32, str]",
        ),
        (
            "mutable-argument-worker",
            r#"
class Counter:
    value: int32

def invalid_worker(counter: mut Counter) -> Result[int32, str]:
    counter.value += 1
    return Result.Ok(counter.value)
"#,
            "invalid_worker",
            "expected `def() -> Result[T, E]`",
            "def(mut Counter) -> Result[int32, str]",
        ),
        (
            "owned-argument-worker",
            r#"
def invalid_worker(value: own str) -> Result[int32, str]:
    return Result.Ok(value.len() as int32)
"#,
            "invalid_worker",
            "expected `def() -> Result[T, E]`",
            "def(own str) -> Result[int32, str]",
        ),
        (
            "non-result-worker",
            r#"
def invalid_worker() -> int32:
    return 1
"#,
            "invalid_worker",
            "expected `Result[T, E]`",
            "found `int32`",
        ),
    ];

    for (label, declaration, worker, expected_contract, found_type) in cases {
        let temp = TempDir::new(&format!("aura-control-retry-worker-{label}"));
        let source = format!(
            r#"
import control
{declaration}
def main() -> int32:
    print(control.retry[int32, str]({worker}))
    return 0
"#
        );
        let source_path = temp.source(label, &source);
        let output = Command::new(aura_bin())
            .arg("check")
            .arg(&source_path)
            .output()
            .expect("control.retry worker check should run");
        assert_failure(&output, "checker");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected_contract),
            "worker diagnostic should state the reusable zero-argument Result contract:\n{stderr}"
        );
        assert!(
            stderr.contains(found_type),
            "worker diagnostic should report the actual function type `{found_type}`:\n{stderr}"
        );
    }
}
