use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use rcgen::generate_simple_self_signed;

const READ_ALL_CAP_BYTES: usize = 64 * 1024 * 1024;

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

fn generated_binary(path: &PathBuf) -> Command {
    let mut command = Command::new(path);
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        #[cfg(unix)]
        command.env("LLVM_PROFILE_FILE", "/dev/null");
        #[cfg(windows)]
        command.env("LLVM_PROFILE_FILE", "NUL");
    }
    command
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap()
        .to_path_buf()
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("failed to create temp dir");
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn assert_default_backend_example_runs(example: &str, binary_name: &str, expected_stdout: &str) {
    let fixture = repo_root().join(example);
    let output_dir = TempDir::new("aurora-build-auto-full");
    let output_path = output_dir.path().join(binary_name);

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&fixture)
        .output()
        .expect("failed to run aura build");

    assert!(
        build.status.success(),
        "default build should support {}, stderr was:\n{}",
        example,
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run built binary");

    assert!(
        run.status.success(),
        "built binary for {} should exit successfully, stderr was:\n{}",
        example,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected_stdout);
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: std::time::Duration,
) -> Option<std::process::ExitStatus> {
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("failed to poll child process") {
            return Some(status);
        }
        if start.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn assert_direct_backend_example_runs(example: &str, binary_name: &str, expected_stdout: &str) {
    let fixture = repo_root().join(example);
    let output_dir = TempDir::new("aurora-build-direct-full");
    let output_path = output_dir.path().join(binary_name);

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&fixture)
        .output()
        .expect("failed to run aura build --backend direct");

    assert!(
        build.status.success(),
        "direct backend should support {}, stderr was:\n{}",
        example,
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run direct-backend binary");

    assert!(
        run.status.success(),
        "direct-backend binary for {} should exit successfully, stderr was:\n{}",
        example,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected_stdout);
}

fn write_temp_source(prefix: &str, source: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new(prefix);
    let source_path = temp.path().join("main.au");
    fs::write(&source_path, source).expect("failed to write temporary Aurora source");
    (temp, source_path)
}

fn build_and_run_direct_source(
    prefix: &str,
    source: &str,
) -> (std::process::Output, std::process::Output) {
    let (temp, source_path) = write_temp_source(prefix, source);
    let output_path = temp.path().join("out");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build --backend direct");

    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run direct-backend binary");

    (build, run)
}

fn build_and_run_default_source(
    prefix: &str,
    source: &str,
) -> (std::process::Output, std::process::Output) {
    let (temp, source_path) = write_temp_source(prefix, source);
    let output_path = temp.path().join("out");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build");

    assert!(
        build.status.success(),
        "default backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run default-backend binary");

    (build, run)
}

fn assert_run_and_direct_source_stdout(prefix: &str, source: &str, expected_stdout: &str) {
    let (temp, source_path) = write_temp_source(prefix, source);
    let run = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run");
    assert!(
        run.status.success(),
        "aura run should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected_stdout);

    let output_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build --backend direct");
    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let direct = generated_binary(&output_path)
        .output()
        .expect("failed to run direct-backend binary");
    assert!(
        direct.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected_stdout);
}

fn assert_run_and_direct_source_stdout_with_timeout(
    prefix: &str,
    source: &str,
    timeout: std::time::Duration,
    expected_stdout: &str,
) {
    let (_temp, _source_path, mut run_child) =
        run_aura_source_with_timeout(prefix, source, timeout);
    let run_status = wait_with_timeout(&mut run_child, timeout).unwrap_or_else(|| {
        run_child
            .kill()
            .expect("failed to kill timed out aura run process");
        panic!("aura run timed out after {:?}", timeout);
    });
    let run = run_child
        .wait_with_output()
        .expect("failed to collect aura run output");
    assert!(
        run_status.success(),
        "aura run should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected_stdout);

    let (_temp, _source_path, mut direct_child) =
        build_direct_source_with_timeout(&format!("{prefix}-direct"), source, timeout);
    let direct_status = wait_with_timeout(&mut direct_child, timeout).unwrap_or_else(|| {
        direct_child
            .kill()
            .expect("failed to kill timed out direct-backend process");
        panic!("direct-backend run timed out after {:?}", timeout);
    });
    let direct = direct_child
        .wait_with_output()
        .expect("failed to collect direct-backend output");
    assert!(
        direct_status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected_stdout);
}

fn assert_run_and_direct_source_failure_with_timeout(
    prefix: &str,
    source: &str,
    timeout: std::time::Duration,
    expected_stdout: &str,
    expected_stderr_substring: &str,
) {
    let (_temp, _source_path, mut run_child) =
        run_aura_source_with_timeout(prefix, source, timeout);
    let run_status = wait_with_timeout(&mut run_child, timeout).unwrap_or_else(|| {
        run_child
            .kill()
            .expect("failed to kill timed out aura run process");
        panic!("aura run timed out after {:?}", timeout);
    });
    let run = run_child
        .wait_with_output()
        .expect("failed to collect aura run output");
    assert!(
        !run_status.success(),
        "aura run should fail, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected_stdout);
    assert!(
        String::from_utf8_lossy(&run.stderr).contains(expected_stderr_substring),
        "aura run stderr should mention `{expected_stderr_substring}`, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let (_temp, _source_path, mut direct_child) =
        build_direct_source_with_timeout(&format!("{prefix}-direct"), source, timeout);
    let direct_status = wait_with_timeout(&mut direct_child, timeout).unwrap_or_else(|| {
        direct_child
            .kill()
            .expect("failed to kill timed out direct-backend process");
        panic!("direct-backend run timed out after {:?}", timeout);
    });
    let direct = direct_child
        .wait_with_output()
        .expect("failed to collect direct-backend output");
    assert!(
        !direct_status.success(),
        "direct-backend binary should fail, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected_stdout);
    assert!(
        String::from_utf8_lossy(&direct.stderr).contains(expected_stderr_substring),
        "direct-backend stderr should mention `{expected_stderr_substring}`, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
}

fn run_aura_source_with_timeout(
    prefix: &str,
    source: &str,
    timeout: std::time::Duration,
) -> (TempDir, PathBuf, std::process::Child) {
    let (temp, source_path) = write_temp_source(prefix, source);
    let child = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("failed to spawn aura run: {error}"));
    assert!(
        timeout > std::time::Duration::ZERO,
        "timeout should be positive"
    );
    (temp, source_path, child)
}

fn build_direct_source_with_timeout(
    prefix: &str,
    source: &str,
    timeout: std::time::Duration,
) -> (TempDir, PathBuf, std::process::Child) {
    let (temp, source_path) = write_temp_source(prefix, source);
    let output_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build --backend direct");
    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let child = generated_binary(&output_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("failed to spawn direct-backend binary: {error}"));
    assert!(
        timeout > std::time::Duration::ZERO,
        "timeout should be positive"
    );
    (temp, source_path, child)
}

#[test]
fn ast_exits_cleanly_when_stdout_pipe_closes() {
    let fixture = repo_root().join("examples/point.au");
    let mut child = Command::new(aura_bin())
        .arg("ast")
        .arg(fixture)
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura ast");

    drop(child.stdout.take());

    let status = child.wait().expect("failed to wait for aura ast");
    assert!(status.success(), "ast should exit cleanly on broken pipe");
}

#[test]
fn lsp_service_handles_multiple_requests_in_one_process() {
    let mut child = Command::new(aura_bin())
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start aura lsp service");
    let input = [
        serde_json::json!({
            "id": 1,
            "method": "analyze",
            "path": "/virtual/main.au",
            "source": "def main() -> int32:\n    return 0\n"
        }),
        serde_json::json!({
            "id": 2,
            "method": "complete",
            "path": "/virtual/main.au",
            "source": "def main() -> int32:\n    value: String = \"hi\"\n    value.\n    return 0\n",
            "line": 2,
            "character": 10,
            "trigger": "."
        }),
    ]
    .into_iter()
    .map(|request| request.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    child
        .stdin
        .take()
        .expect("lsp stdin should be piped")
        .write_all(format!("{input}\n").as_bytes())
        .expect("lsp requests should write");

    let output = child
        .wait_with_output()
        .expect("lsp service should exit after stdin closes");
    assert!(
        output.status.success(),
        "lsp service should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = String::from_utf8(output.stdout)
        .expect("lsp responses should be utf-8")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid response JSON"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["id"], 1);
    assert!(responses[0]["result"]["diagnostics"].is_array());
    assert_eq!(responses[1]["id"], 2);
    assert!(responses[1]["result"]
        .as_array()
        .expect("completion result should be an array")
        .iter()
        .any(|item| item["name"] == "len"));
}

#[test]
fn new_fmt_and_test_commands_cover_the_project_workflow() {
    let temp = TempDir::new("aurora-project-workflow");
    let create = Command::new(aura_bin())
        .current_dir(temp.path())
        .args(["new", "agent-app"])
        .output()
        .expect("failed to run aura new");
    assert!(
        create.status.success(),
        "aura new should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&create.stderr)
    );
    let project = temp.path().join("agent-app");
    assert!(project.join("Aurora.toml").is_file());
    assert!(project.join("src/main.au").is_file());
    assert!(project.join("tests/smoke.au").is_file());
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).expect("gitignore should read"),
        "target/\n"
    );

    fs::write(
        project.join("src/main.au"),
        "def main() -> int32:   \r\n    print(\"ready\")\t\r\n    return 0\r\n",
    )
    .expect("unformatted source should write");
    let check = Command::new(aura_bin())
        .current_dir(&project)
        .args(["fmt", "--check", "src/main.au"])
        .output()
        .expect("failed to run aura fmt --check");
    assert!(
        !check.status.success(),
        "unformatted source should fail --check"
    );

    let format = Command::new(aura_bin())
        .current_dir(&project)
        .args(["fmt", "src/main.au"])
        .output()
        .expect("failed to run aura fmt");
    assert!(format.status.success(), "aura fmt should succeed");
    assert_eq!(
        fs::read_to_string(project.join("src/main.au")).expect("formatted source should read"),
        "def main() -> int32:\n    print(\"ready\")\n    return 0\n"
    );

    fs::write(
        project.join("src/helpers.au"),
        "public def answer() -> int32:\n    return 42\n",
    )
    .expect("project helper source should write");
    fs::write(
        project.join("tests/smoke.au"),
        "from helpers import answer\n\ndef main() -> int32:\n    print(answer())\n    return 0\n",
    )
    .expect("test source should write");
    let tests = Command::new(aura_bin())
        .current_dir(&project)
        .arg("test")
        .output()
        .expect("failed to run aura test");
    assert!(
        tests.status.success(),
        "aura test should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&tests.stderr)
    );
    assert!(String::from_utf8_lossy(&tests.stdout).contains("1 passed; 0 failed"));

    fs::write(
        project.join("tests/slow.au"),
        "def main() -> int32:\n    sleep(1s)\n    return 0\n",
    )
    .expect("slow test source should write");
    let timed_out = Command::new(aura_bin())
        .current_dir(&project)
        .args(["test", "--timeout-ms", "10", "tests/slow.au"])
        .output()
        .expect("failed to run timed-out aura test");
    assert!(!timed_out.status.success(), "timed-out test should fail");
    assert!(String::from_utf8_lossy(&timed_out.stderr).contains("timed out after 10ms"));

    let recreate = Command::new(aura_bin())
        .current_dir(temp.path())
        .args(["new", "agent-app"])
        .output()
        .expect("failed to rerun aura new");
    assert!(
        !recreate.status.success(),
        "aura new must not overwrite a project"
    );
}

#[test]
fn run_and_built_programs_receive_arguments_and_environment() {
    let source = r#"import sys

def print_child_arguments():
    for argument in sys.args():
        print("child:" + argument)

def main() -> int32:
    for argument in sys.args():
        print("main:" + argument)
    with TaskGroup() as group:
        group.start_soon(print_child_arguments)
    match sys.env("AURORA_CLI_TEST_VALUE"):
        case Option.Some(value):
            print(value)
        case Option.None:
            return 1
    return 0
"#;
    let (temp, source_path) = write_temp_source("aurora-program-args", source);
    let interpreted = Command::new(aura_bin())
        .args(["run", source_path.to_str().expect("UTF-8 temp path"), "--"])
        .args(["alpha", "beta"])
        .env("AURORA_CLI_TEST_VALUE", "from-env")
        .env("AURORA_PROGRAM_ARGS_JSON", "[\"spoofed\"]")
        .output()
        .expect("failed to run aura program with arguments");
    assert!(
        interpreted.status.success(),
        "aura run should accept program arguments, stderr was:\n{}",
        String::from_utf8_lossy(&interpreted.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&interpreted.stdout),
        "main:alpha\nmain:beta\nchild:alpha\nchild:beta\nfrom-env\n"
    );

    let mut stdin_child = Command::new(aura_bin())
        .arg("run")
        .arg("--stdin")
        .arg(&source_path)
        .arg("--")
        .args(["alpha", "beta"])
        .env("AURORA_CLI_TEST_VALUE", "from-env")
        .env("AURORA_PROGRAM_ARGS_JSON", "[\"spoofed\"]")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run stdin Aurora program with arguments");
    stdin_child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write argv test source to stdin");
    let stdin_interpreted = stdin_child
        .wait_with_output()
        .expect("failed to collect stdin argv test output");
    assert!(
        stdin_interpreted.status.success(),
        "stdin aura run should accept explicit program arguments, stderr was:\n{}",
        String::from_utf8_lossy(&stdin_interpreted.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&stdin_interpreted.stdout),
        "main:alpha\nmain:beta\nchild:alpha\nchild:beta\nfrom-env\n"
    );

    let output_path = temp.path().join("program");
    let build = Command::new(aura_bin())
        .args(["build", "--backend", "direct", "-o"])
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to build argument-aware program");
    assert!(
        build.status.success(),
        "argument-aware direct build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let direct = generated_binary(&output_path)
        .args(["alpha", "beta"])
        .env("AURORA_CLI_TEST_VALUE", "from-env")
        .env("AURORA_PROGRAM_ARGS_JSON", "[\"spoofed\"]")
        .output()
        .expect("failed to run built program with arguments");
    assert!(
        direct.status.success(),
        "built program should accept arguments, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&direct.stdout),
        "main:alpha\nmain:beta\nchild:alpha\nchild:beta\nfrom-env\n"
    );
}

#[test]
fn mir_and_forced_direct_support_one_thousand_simultaneously_suspended_tasks() {
    let source = r#"def suspend(started: Queue[int32], release: Queue[int32]):
    started.put(1)
    match release.get():
        case QueueReceive.Item(_):
            pass
        case QueueReceive.Closed:
            pass
        case QueueReceive.TimedOut:
            pass
        case QueueReceive.Cancelled:
            pass

def main() -> int32:
    started = Queue[int32]()
    release = Queue[int32]()
    mut ready: int32 = 0

    with TaskGroup() as group:
        mut spawned: int32 = 0
        while spawned < 1000:
            group.start_soon(suspend, started, release)
            spawned += 1

        while ready < 1000:
            match started.get():
                case QueueReceive.Item(_):
                    ready += 1
                case QueueReceive.Closed:
                    return 2
                case QueueReceive.TimedOut:
                    pass
                case QueueReceive.Cancelled:
                    return 3

        release.close()

    print(ready)
    return 0
"#;

    assert_run_and_direct_source_stdout("aurora-thousand-suspended-direct-tasks", source, "1000\n");
}

#[test]
fn mir_exits_cleanly_when_stdout_pipe_closes() {
    let fixture = repo_root().join("examples/control_flow/while_break_continue.au");
    let mut child = Command::new(aura_bin())
        .arg("mir")
        .arg(fixture)
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura mir");

    drop(child.stdout.take());

    let status = child.wait().expect("failed to wait for aura mir");
    assert!(status.success(), "mir should exit cleanly on broken pipe");
}

#[test]
fn task_group_scope_exit_cancels_blocked_children() {
    let source = r#"def wait_forever(q: Queue[int32]) -> None:
    match q.get():
        case QueueReceive.Item(value):
            print(value)
        case QueueReceive.Closed:
            print("closed")
        case QueueReceive.TimedOut:
            print("timed out")
        case QueueReceive.Cancelled:
            print("cancelled")

def main() -> int32:
    q = Queue[int32]()
    with TaskGroup() as group:
        group.start_soon(wait_forever, q)
    print("done")
    return 0
"#;

    let (_temp, _source_path, mut child) = run_aura_source_with_timeout(
        "aurora-task-group-close",
        source,
        std::time::Duration::from_secs(15),
    );
    let status = wait_with_timeout(&mut child, std::time::Duration::from_secs(15))
        .expect("task-group scope exit should not hang indefinitely");
    let output = child
        .wait_with_output()
        .expect("failed to collect aura run output");
    assert!(
        status.success(),
        "task-group scope exit should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "cancelled\ndone\n");

    let (_temp, _source_path, mut direct_child) = build_direct_source_with_timeout(
        "aurora-task-group-close-direct",
        source,
        std::time::Duration::from_secs(15),
    );
    let status = wait_with_timeout(&mut direct_child, std::time::Duration::from_secs(15))
        .expect("direct task-group scope exit should not hang indefinitely");
    let output = direct_child
        .wait_with_output()
        .expect("failed to collect direct-backend output");
    assert!(
        status.success(),
        "direct task-group scope exit should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "cancelled\ndone\n");
}

#[test]
fn queue_consumers_share_work_without_starvation() {
    let source = r#"def consumer(q: Queue[int32]) -> int32:
    mut got: int32 = 0
    for value in q:
        got += 1
    return got

def main() -> int32:
    q = Queue[int32](capacity=16)
    with TaskGroup() as group:
        c1 = group.start(consumer, q)
        c2 = group.start(consumer, q)
        c3 = group.start(consumer, q)
        c4 = group.start(consumer, q)

        mut i: int32 = 0
        while i < 1000:
            match q.put(i):
                case Result.Ok(_):
                    pass
                case Result.Err(_):
                    return 1
            i += 1
        q.close()

        print(c1.result_or(-1, timeout=5s))
        print(c2.result_or(-1, timeout=5s))
        print(c3.result_or(-1, timeout=5s))
        print(c4.result_or(-1, timeout=5s))
    return 0
"#;

    let (_temp, source_path) = write_temp_source("aurora-queue-fairness", source);
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run queue fairness source");

    assert!(
        output.status.success(),
        "queue fairness source should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let counts = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.parse::<i32>().expect("counts should be integers"))
        .collect::<Vec<_>>();
    assert_eq!(counts.len(), 4, "expected four consumer counts");
    assert_eq!(
        counts.iter().sum::<i32>(),
        1000,
        "counts should sum to all items"
    );
    let min = *counts.iter().min().expect("counts should not be empty");
    let max = *counts.iter().max().expect("counts should not be empty");
    assert!(
        max - min <= 1,
        "queue consumers should share work fairly, got {:?}",
        counts
    );
}

#[test]
fn cancelled_sleeping_children_resume_and_can_observe_cancellation() {
    let source = r#"def long_sleeper() -> int32:
    sleep(5s)
    print("after-sleep")
    if cancelled():
        print("observed-cancel")
        return 7
    return 99

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(long_sleeper)
        sleep(20ms)
        group.cancel()
        match task.result(timeout=1s):
            case TaskResult.Ready(value):
                print(value)
            case TaskResult.Error(message):
                print(message)
            case TaskResult.TimedOut:
                print("timedout")
            case TaskResult.Cancelled:
                print("cancelled")
    return 0
"#;

    assert_run_and_direct_source_stdout(
        "aurora-sleep-cancel-observed",
        source,
        "after-sleep\nobserved-cancel\n7\n",
    );
}

#[test]
fn large_http_responses_complete_without_timing_out() {
    let temp = TempDir::new("aurora-http-large-response");
    let body_path = temp.path().join("body.txt");
    fs::write(&body_path, "x".repeat(50_000)).expect("failed to write HTTP response body");
    let source = format!(
        r#"import fs
import io
import net

def serve(listener: net.HttpListener, path: String) -> Result[None, io.Error]:
    server = listener
    req = try server.accept(timeout=2s)
    body = try fs.read_to_string(path)
    try req.respond_text(200, body, {{}})
    return Result.Ok(None)

def run() -> Result[None, io.Error]:
    with TaskGroup() as group:
        listener = try net.http_listen("127.0.0.1:0")
        address = try listener.local_addr()
        group.start_soon(serve, listener, "{body_path}")
        resp = try net.http_request_text_timeout("GET", "http://" + address + "/big", "x", {{}}, 2s)
        with r = resp:
            print(r.status())
            text = try r.text()
            print(text.len())
        return Result.Ok(None)

def main() -> int32:
    match run():
        case Result.Ok(_):
            return 0
        case Result.Err(err):
            print(err)
            return 1
"#,
        body_path = body_path.display()
    );
    let source_path = temp.path().join("main.au");
    fs::write(&source_path, source).expect("failed to write HTTP regression source");

    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run large HTTP response source");

    assert!(
        output.status.success(),
        "large HTTP response source should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "200\n50000\n");
}

#[test]
fn check_rejects_huge_left_associative_expression_chains_without_crashing() {
    let mut expr = String::from("1");
    for _ in 0..5000 {
        expr.push_str(" + 1");
    }
    let source = format!("def main() -> int32:\n    value = {expr}\n    return value\n");
    let (_temp, source_path) = write_temp_source("aurora-huge-chain", &source);

    let output = Command::new(aura_bin())
        .arg("check")
        .arg(&source_path)
        .output()
        .expect("failed to run aura check");

    assert!(
        !output.status.success(),
        "huge left-associative chains should fail gracefully"
    );
    assert_ne!(
        output.status.code(),
        None,
        "aura check should not die by signal on huge expression chains"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("expression chain")
            || String::from_utf8_lossy(&output.stderr).contains("expression nesting"),
        "expected a structural diagnostic, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn direct_backend_reports_recursion_overflow_without_signalling() {
    let source = r#"def recurse(n: int32) -> int32:
    if n == 0:
        return 0
    return recurse(n - 1)

def main() -> int32:
    return recurse(10000000)
"#;

    let (_build, run) = build_and_run_direct_source("aurora-direct-recursion", source);
    assert!(
        !run.status.success(),
        "deep recursion should fail cleanly in the direct backend"
    );
    assert_ne!(
        run.status.code(),
        None,
        "direct backend recursion overflow should not terminate by signal"
    );
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("maximum call depth"),
        "expected a direct-backend recursion diagnostic, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn help_flags_exit_successfully() {
    for args in [["help"], ["--help"], ["-h"]] {
        let output = Command::new(aura_bin())
            .args(args)
            .output()
            .expect("failed to run aura help");

        assert!(
            output.status.success(),
            "help path {:?} should succeed, stderr was:\n{}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("usage: aura"),
            "help path {:?} should print usage",
            args
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("run-mir"),
            "help path {:?} should no longer advertise `run-mir`, stdout was:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn run_mir_command_is_rejected() {
    let fixture = repo_root().join("examples/basics/simple_example.au");
    let output = Command::new(aura_bin())
        .arg("run-mir")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run-mir");

    assert!(
        !output.status.success(),
        "`run-mir` should be rejected now, stdout was:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("usage: aura"),
        "`run-mir` rejection should print usage, stderr was:\n{}",
        stderr
    );
}

#[test]
fn version_flags_exit_successfully() {
    for args in [["version"], ["--version"], ["-V"]] {
        let output = Command::new(aura_bin())
            .args(args)
            .output()
            .expect("failed to run aura version");

        assert!(
            output.status.success(),
            "version path {:?} should succeed, stderr was:\n{}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("aura {}\n", env!("CARGO_PKG_VERSION"))
        );
    }
}

#[test]
fn nested_package_module_can_be_checked_directly() {
    let fixture = repo_root().join("examples/modules/pkg/user.au");
    let output = Command::new(aura_bin())
        .arg("check")
        .arg(&fixture)
        .output()
        .expect("failed to run aura check");

    assert!(
        output.status.success(),
        "direct check of nested package module should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n");
}

#[test]
fn nested_package_module_can_be_analyzed_directly() {
    let fixture = repo_root().join("examples/modules/pkg/user.au");
    let output = Command::new(aura_bin())
        .arg("analyze")
        .arg(&fixture)
        .output()
        .expect("failed to run aura analyze");

    assert!(
        output.status.success(),
        "direct analyze of nested package module should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"diagnostics\":[]"),
        "analysis should not report false import diagnostics, stdout was:\n{}",
        stdout
    );
    assert!(
        stdout.contains("\"name\":\"User\""),
        "analysis should still include symbols, stdout was:\n{}",
        stdout
    );
}

#[test]
fn analyze_recovers_symbols_for_dangling_dot_stdin_buffers() {
    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    counter.",
        "    return 0",
    ]
    .join("\n");

    let mut child = Command::new(aura_bin())
        .arg("analyze")
        .arg("--stdin")
        .arg("/virtual/counter.au")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura analyze");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura analyze output");

    assert!(
        output.status.success(),
        "analyze should succeed on dangling-dot buffers"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("analyze should return valid JSON");
    let symbols = json["symbols"]
        .as_array()
        .expect("symbols should be an array");
    let occurrences = json["occurrences"]
        .as_array()
        .expect("occurrences should be an array");
    assert!(
        !symbols.is_empty(),
        "dangling-dot analysis should still return symbols"
    );
    assert!(
        !occurrences.is_empty(),
        "dangling-dot analysis should still return occurrences"
    );
}

#[test]
fn analyze_recovers_symbols_for_dangling_dot_at_eof_stdin_buffers() {
    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    counter.",
    ]
    .join("\n");

    let mut child = Command::new(aura_bin())
        .arg("analyze")
        .arg("--stdin")
        .arg("/virtual/counter.au")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura analyze");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura analyze output");

    assert!(
        output.status.success(),
        "analyze should succeed on dangling-dot EOF buffers, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("analyze should return valid JSON");
    assert!(
        !json["symbols"]
            .as_array()
            .expect("symbols should be an array")
            .is_empty(),
        "dangling-dot EOF analysis should still return symbols"
    );
    assert!(
        !json["occurrences"]
            .as_array()
            .expect("occurrences should be an array")
            .is_empty(),
        "dangling-dot EOF analysis should still return occurrences"
    );
}

#[test]
fn analyze_stdin_resolves_local_module_imports() {
    let temp = TempDir::new("aurora-cli-analyze-modules");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write helper module");
    let main_path = temp.path().join("main.au");
    let source = "import helpers.math\n\ndef main() -> int32:\n    print(helpers.math.double(value=5))\n    return 0\n";

    let mut child = Command::new(aura_bin())
        .arg("analyze")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura analyze");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura analyze output");

    assert!(
        output.status.success(),
        "analyze should succeed for module-aware stdin buffers, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("analyze should return valid JSON");
    assert_eq!(
        json["diagnostics"]
            .as_array()
            .expect("diagnostics should be an array")
            .len(),
        0,
        "analysis should not report diagnostics for a valid local-module program"
    );
    assert!(
        json["occurrences"]
            .as_array()
            .expect("occurrences should be an array")
            .iter()
            .any(|occurrence| occurrence["hover"]
                .as_str()
                .unwrap_or_default()
                .contains("function double")),
        "analysis should include occurrences for imported module members"
    );
}

#[test]
fn check_stdin_resolves_local_module_imports() {
    let temp = TempDir::new("aurora-cli-check-modules");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write helper module");
    let main_path = temp.path().join("main.au");
    let source =
        "import helpers.math\n\ndef main() -> int32:\n    print(helpers.math.double(value=5))\n    return 0\n";

    let mut child = Command::new(aura_bin())
        .arg("check")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura check");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura check output");

    assert!(
        output.status.success(),
        "check should succeed for module-aware stdin buffers, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n");
}

#[test]
fn run_stdin_resolves_local_module_imports() {
    let temp = TempDir::new("aurora-cli-run-modules-stdin");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write helper module");
    let main_path = temp.path().join("main.au");
    let source =
        "import helpers.math\n\ndef main() -> int32:\n    print(helpers.math.double(value=5))\n    return 0\n";

    let mut child = Command::new(aura_bin())
        .arg("run")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura run");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura run output");

    assert!(
        output.status.success(),
        "run should succeed for module-aware stdin buffers, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "10\n");
}

#[test]
fn run_stdin_with_path_resolves_local_module_imports() {
    let temp = TempDir::new("aurora-cli-run-modules-stdin");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write helper module");
    let main_path = temp.path().join("main.au");
    let source =
        "import helpers.math\n\ndef main() -> int32:\n    print(helpers.math.double(value=5))\n    return 0\n";

    let mut child = Command::new(aura_bin())
        .arg("run")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura run");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura run output");

    assert!(
        output.status.success(),
        "run should succeed for module-aware stdin buffers, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "10\n");
}

#[test]
fn mir_stdin_resolves_local_module_imports() {
    let temp = TempDir::new("aurora-cli-mir-modules-stdin");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write helper module");
    let main_path = temp.path().join("main.au");
    let source =
        "import helpers.math\n\ndef main() -> int32:\n    print(helpers.math.double(value=5))\n    return 0\n";

    let mut child = Command::new(aura_bin())
        .arg("mir")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura mir");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura mir output");

    assert!(
        output.status.success(),
        "mir should succeed for module-aware stdin buffers, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("double"),
        "MIR dump should include imported module calls"
    );
}

#[test]
fn complete_stdin_resolves_local_module_member_completions() {
    let temp = TempDir::new("aurora-cli-complete-modules");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write helper module");
    let main_path = temp.path().join("main.au");
    let source = "import helpers.math\n\ndef main() -> int32:\n    helpers.math.\n    return 0\n";

    let mut child = Command::new(aura_bin())
        .arg("complete")
        .arg("--line")
        .arg("3")
        .arg("--character")
        .arg("17")
        .arg("--trigger")
        .arg(".")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura complete");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura complete output");

    assert!(
        output.status.success(),
        "complete should succeed for module-aware stdin buffers, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("complete should return valid JSON");
    assert!(
        json.as_array()
            .expect("completions should be an array")
            .iter()
            .any(|item| item["name"].as_str() == Some("double")),
        "module member completions should include exported functions"
    );
}

#[test]
fn editor_stdin_analysis_and_completion_do_not_write_package_lockfile() {
    let temp = TempDir::new("aurora-cli-editor-no-lock");
    fs::create_dir_all(temp.path().join("app/src")).expect("failed to create app src");
    fs::create_dir_all(temp.path().join("util/src")).expect("failed to create util src");
    fs::write(
        temp.path().join("app/Aurora.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[dependencies]\nutil = { path = \"../util\" }\n",
    )
    .expect("failed to write app manifest");
    fs::write(
        temp.path().join("util/Aurora.toml"),
        "[package]\nname = \"util\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    )
    .expect("failed to write util manifest");
    fs::write(
        temp.path().join("util/src/math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write util module");

    let main_path = temp.path().join("app/src/main.au");
    let analyze_source =
        "import util.math\n\ndef main() -> int32:\n    print(util.math.double(5))\n    return 0\n";
    let lockfile = temp.path().join("app/Aurora.lock");
    assert!(
        !lockfile.exists(),
        "test package should start without a lockfile"
    );

    let mut analyze = Command::new(aura_bin())
        .arg("analyze")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura analyze");
    analyze
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(analyze_source.as_bytes())
        .expect("failed to write analyze source");
    let analyze_output = analyze
        .wait_with_output()
        .expect("failed to collect aura analyze output");
    assert!(
        analyze_output.status.success(),
        "analyze should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&analyze_output.stderr)
    );
    assert!(
        !lockfile.exists(),
        "analyze --stdin should not write Aurora.lock for editor buffers"
    );

    let completion_source =
        "import util.math\n\ndef main() -> int32:\n    util.math.\n    return 0\n";
    let mut complete = Command::new(aura_bin())
        .arg("complete")
        .arg("--line")
        .arg("3")
        .arg("--character")
        .arg("14")
        .arg("--trigger")
        .arg(".")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura complete");
    complete
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(completion_source.as_bytes())
        .expect("failed to write completion source");
    let complete_output = complete
        .wait_with_output()
        .expect("failed to collect aura complete output");
    assert!(
        complete_output.status.success(),
        "complete should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&complete_output.stderr)
    );
    assert!(
        !lockfile.exists(),
        "complete --stdin should not write Aurora.lock for editor buffers"
    );
}

#[test]
fn complete_stdin_includes_imported_trait_methods() {
    let temp = TempDir::new("aurora-cli-complete-imported-trait");
    fs::create_dir_all(temp.path().join("pkg")).expect("failed to create package dir");
    fs::write(
        temp.path().join("pkg/named.au"),
        "public trait Named:\n    def name(borrow self) -> String\n",
    )
    .expect("failed to write trait module");
    fs::write(
        temp.path().join("pkg/user.au"),
        "from pkg.named import Named\n\npublic class User:\n    public label: String\n\nimpl Named for User:\n    def name(borrow self) -> String:\n        return self.label.clone()\n",
    )
    .expect("failed to write user module");
    let main_path = temp.path().join("main.au");
    let source =
        "from pkg.user import User\n\ndef main() -> int32:\n    user = User(label=\"Ada\")\n    user.\n    return 0\n";

    let mut child = Command::new(aura_bin())
        .arg("complete")
        .arg("--line")
        .arg("4")
        .arg("--character")
        .arg("9")
        .arg("--trigger")
        .arg(".")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura complete");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura complete output");

    assert!(
        output.status.success(),
        "complete should succeed for imported trait impl members, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .expect("complete should return valid JSON");
    let names = json
        .as_array()
        .expect("completions should be an array")
        .iter()
        .filter_map(|item| item["name"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    assert!(
        names.contains(&"label".to_string()),
        "completions should still include class fields"
    );
    assert!(
        names.contains(&"name".to_string()),
        "completions should include imported trait methods"
    );
}

#[test]
fn complete_recovers_member_completions_for_dangling_dot_at_eof_stdin_buffers() {
    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    counter.",
    ]
    .join("\n");

    let mut child = Command::new(aura_bin())
        .arg("complete")
        .arg("--line")
        .arg("5")
        .arg("--character")
        .arg("12")
        .arg("--trigger")
        .arg(".")
        .arg("--stdin")
        .arg("/virtual/counter.au")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura complete");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura complete output");

    assert!(
        output.status.success(),
        "complete should succeed on dangling-dot EOF buffers, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("complete should return valid JSON");
    assert!(
        json.as_array()
            .expect("completions should be an array")
            .iter()
            .any(|item| item["name"].as_str() == Some("value")),
        "dangling-dot EOF completions should still include members"
    );
}

#[test]
fn analyze_recovers_symbols_for_multiple_dangling_dots_with_imports() {
    let temp = TempDir::new("aurora-analyze-multi-dangling-imports");
    let helpers_dir = temp.path().join("helpers");
    fs::create_dir_all(&helpers_dir).expect("failed to create helpers dir");
    fs::write(
        helpers_dir.join("math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write helper math module");
    fs::write(
        helpers_dir.join("counter.au"),
        "public class Counter:\n    public value: int32\n",
    )
    .expect("failed to write helper counter module");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "import helpers.math\nfrom helpers.counter import Counter\n\ndef main() -> int32:\n    counter = Counter(value=3)\n    print(helpers.math.\n    print(counter.\n    return 0\n",
    )
    .expect("failed to write main module");

    let output = Command::new(aura_bin())
        .arg("analyze")
        .arg(&source_path)
        .output()
        .expect("failed to run aura analyze");

    assert!(
        output.status.success(),
        "analyze should succeed on recoverable multiple dangling-dot buffers, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .expect("analyze should return valid JSON");
    assert!(
        json["symbols"]
            .as_array()
            .is_some_and(|symbols| !symbols.is_empty()),
        "analyze should still recover symbols for multiple dangling dots"
    );
    assert!(
        json["occurrences"]
            .as_array()
            .is_some_and(|occurrences| !occurrences.is_empty()),
        "analyze should still recover occurrences for multiple dangling dots"
    );
}

#[test]
fn complete_recovers_member_completions_for_multiple_dangling_dots_with_imports() {
    let temp = TempDir::new("aurora-complete-multi-dangling-imports");
    let helpers_dir = temp.path().join("helpers");
    fs::create_dir_all(&helpers_dir).expect("failed to create helpers dir");
    fs::write(
        helpers_dir.join("math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\npublic def triple(value: int32) -> int32:\n    return value * 3\n",
    )
    .expect("failed to write helper math module");
    fs::write(
        helpers_dir.join("counter.au"),
        "public class Counter:\n    public value: int32\n",
    )
    .expect("failed to write helper counter module");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "import helpers.math\nfrom helpers.counter import Counter\n\ndef main() -> int32:\n    counter = Counter(value=3)\n    print(helpers.math.\n    print(counter.\n    return 0\n",
    )
    .expect("failed to write main module");

    let output = Command::new(aura_bin())
        .arg("complete")
        .arg("--line")
        .arg("5")
        .arg("--character")
        .arg("23")
        .arg("--trigger")
        .arg(".")
        .arg(&source_path)
        .output()
        .expect("failed to run aura complete");

    assert!(
        output.status.success(),
        "complete should succeed on recoverable multiple dangling-dot buffers, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .expect("complete should return valid JSON");
    let names = json
        .as_array()
        .expect("completions should be an array")
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"double"));
    assert!(names.contains(&"triple"));
}

#[test]
fn build_produces_a_runnable_binary() {
    let fixture = repo_root().join("examples/point.au");
    let output_dir = TempDir::new("aurora-build");
    let output_path = output_dir.path().join("point");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&fixture)
        .output()
        .expect("failed to run aura build");

    assert!(
        build.status.success(),
        "build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(output_path.exists(), "build should create an output binary");

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run built output");

    assert!(
        run.status.success(),
        "built binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "5.0\n");
}

#[test]
fn build_with_direct_backend_produces_runnable_binary_for_supported_program() {
    let temp = TempDir::new("aurora-build-direct");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "def helper(value: int32) -> int32:\n    return value + 2\n\n\
def main() -> int32:\n    mut current: int32 = 1\n    if current < 5:\n        current = helper(value=current)\n    print(current)\n    return 0\n",
    )
    .expect("failed to write direct-backend source");
    let output_path = temp.path().join("direct-main");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build --backend direct");

    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run direct-backend binary");

    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n");
}

#[test]
fn build_with_direct_backend_rejects_unsupported_programs() {
    let fixture = repo_root().join("examples/modules/helpers/math.au");
    let output_dir = TempDir::new("aurora-build-direct-unsupported");
    let output_path = output_dir.path().join("helper-module-direct");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&fixture)
        .output()
        .expect("failed to run aura build --backend direct on non-entry module");

    assert!(
        !build.status.success(),
        "direct backend should reject non-entry modules"
    );
    assert!(
        String::from_utf8_lossy(&build.stderr)
            .contains("requires a `main` function or top-level script"),
        "non-entry direct backend errors should explain the missing entrypoint, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
}

#[test]
fn build_rejects_removed_mir_runtime_backend_option() {
    let fixture = repo_root().join("examples/point.au");
    let output_dir = TempDir::new("aurora-build-removed-backend");
    let output_path = output_dir.path().join("point-removed-backend");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("mir-runtime")
        .arg("-o")
        .arg(&output_path)
        .arg(&fixture)
        .output()
        .expect("failed to run aura build with removed backend");

    assert!(
        !build.status.success(),
        "removed mir-runtime backend option should fail"
    );
    assert!(
        String::from_utf8_lossy(&build.stderr).contains("usage:")
            || String::from_utf8_lossy(&build.stderr).contains("auto|direct"),
        "removed backend option should report current build usage, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
}

#[test]
fn build_with_direct_backend_supports_point_example() {
    let fixture = repo_root().join("examples/point.au");
    let output_dir = TempDir::new("aurora-build-direct-point");
    let output_path = output_dir.path().join("point-direct");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&fixture)
        .output()
        .expect("failed to run aura build --backend direct on point example");

    assert!(
        build.status.success(),
        "direct backend should support point example, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run direct-backend point binary");

    assert!(
        run.status.success(),
        "direct-backend point binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "5.0\n");
}

#[test]
fn build_with_direct_backend_supports_class_methods_example() {
    assert_direct_backend_example_runs(
        "examples/classes/methods.au",
        "methods-direct",
        "4\n8\n0\n",
    );
}

#[test]
fn build_with_direct_backend_supports_string_example() {
    assert_direct_backend_example_runs(
        "examples/strings/greeting.au",
        "greeting-direct",
        "hello, aurora\n",
    );
}

#[test]
fn build_with_direct_backend_supports_string_methods_example() {
    assert_direct_backend_example_runs(
        "examples/strings/string_methods.au",
        "string-methods-direct",
        "15\ntrue\ntrue\ntrue\naurora repo\n2\naurora\nrepo\naurora lang\naurora repo\nAURORA REPO\nrepo\nnone\naurora\nnone\n11\n",
    );
}

#[test]
fn build_with_auto_backend_falls_back_for_rich_match_example() {
    let fixture = repo_root().join("examples/enums/rich_match.au");
    let output_dir = TempDir::new("aurora-build-auto-rich-match");
    let output_path = output_dir.path().join("rich-match-auto");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("auto")
        .arg("-o")
        .arg(&output_path)
        .arg(&fixture)
        .output()
        .expect("failed to run aura build --backend auto on rich match example");

    assert!(
        build.status.success(),
        "auto backend should succeed for rich match example, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run auto-backend rich match binary");

    assert!(
        run.status.success(),
        "auto-backend rich match binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n30\n0\n");
}

#[test]
fn build_with_direct_backend_supports_indexed_member_chains_and_fstring_indexing() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-index-chain-fstring",
        "def main() -> int32:\n    keys = [\"a\", \"b\"]\n    idx = 1\n    mut counts = {\"key\": 7}\n    match keys.get(idx):\n        case Some(key):\n            print(key)\n        case None:\n            print(\"missing\")\n    print(f\"val: {counts[\"key\"]}\")\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct backend indexed-chain/fstring binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "b\nval: 7\n");
}

#[test]
fn build_with_direct_backend_supports_inferred_enum_match_variants() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-inferred-enum-match",
        "enum Signal:\n    Ready\n    Busy\n\ndef main() -> int32:\n    signal = Signal.Ready\n    match signal:\n        case Ready:\n            print(\"ready\")\n        case Busy:\n            print(\"busy\")\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct backend inferred-enum match binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "ready\n");
}

#[test]
fn build_with_direct_backend_supports_generic_class_field_arithmetic() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-generic-class-fields",
        "class Pair[A]:\n    a: A\n    b: A\n\ndef main() -> int32:\n    pair = Pair[int32](a=3, b=4)\n    inferred = Pair(a=10, b=3)\n    print(pair.a + pair.b)\n    print(inferred.a + inferred.b)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct backend generic-class field arithmetic should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n13\n");
}

#[test]
fn build_with_direct_backend_supports_multi_payload_enum_variants() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-multi-payload-enum",
        "enum Pairing:\n    Pair(int32, int32)\n\ndef main() -> int32:\n    value = Pairing.Pair(2, 3)\n    match value:\n        case Pairing.Pair(a, b):\n            print(a + b)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct backend multi-payload enum binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n");
}

#[test]
fn check_reports_imported_module_syntax_errors_at_the_imported_file() {
    let temp = TempDir::new("aurora-imported-module-syntax");
    let main_path = temp.path().join("main.au");
    let broken_path = temp.path().join("broken.au");
    fs::write(
        &main_path,
        "import broken\n\ndef main() -> int32:\n    return 0\n",
    )
    .expect("failed to write main module");
    fs::write(
        &broken_path,
        "def broken() -> int32:\n    return @@@ syntax error\n",
    )
    .expect("failed to write broken module");

    let output = Command::new(aura_bin())
        .arg("check")
        .arg(&main_path)
        .output()
        .expect("failed to run aura check");

    assert!(
        !output.status.success(),
        "syntax errors in imported modules should fail checking"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&broken_path.display().to_string()),
        "stderr should point at the imported module path, stderr was:\n{}",
        stderr
    );
    assert!(
        stderr.contains("unexpected character `@`"),
        "stderr should preserve the imported parser error, stderr was:\n{}",
        stderr
    );
}

#[test]
fn build_with_direct_backend_supports_numeric_builtins_example() {
    assert_direct_backend_example_runs(
        "examples/numbers/numeric_builtins.au",
        "numeric-builtins-direct",
        "7\n3.5\n2\n12\n9.0\n9.0\n",
    );
}

#[test]
fn build_with_direct_backend_supports_map_basics_example() {
    assert_direct_backend_example_runs(
        "examples/collections/map_basics.au",
        "map-basics-direct",
        "3\ntrue\n1\n1\n5\naurora\n3\n3\n3\n3\ntrue\n",
    );
}

#[test]
fn build_with_direct_backend_supports_set_basics_example() {
    assert_direct_backend_example_runs(
        "examples/collections/set_basics.au",
        "set-basics-direct",
        "3\ntrue\nfalse\ntrue\ntrue\n9\ntrue\ntrue\n1\n",
    );
}

#[test]
fn build_with_direct_backend_supports_string_parsing_and_formatting_example() {
    assert_direct_backend_example_runs(
        "examples/strings/string_parsing_and_formatting.au",
        "string-parsing-formatting-direct",
        "42\n-9000000000\n3.5\ntrue\naurora-lang-tests\ntrue\n12\n4\n9\n3.0\n",
    );
}

#[test]
fn build_with_direct_backend_supports_file_io_example() {
    assert_direct_backend_example_runs(
        "examples/io/read_text_file.au",
        "file-io-direct",
        "true\ntrue\n",
    );
}

#[test]
fn build_with_direct_backend_supports_bytes_file_io_example() {
    assert_direct_backend_example_runs(
        "examples/io/bytes_file_io.au",
        "bytes-file-io-direct",
        "4\n65\n67\n5\n68\n",
    );
}

#[test]
fn build_with_direct_backend_caps_fs_read_to_string_and_read_bytes() {
    let temp = TempDir::new("aurora-direct-file-read-cap");
    let file_path = temp.path().join("huge.txt");
    let file = fs::File::create(&file_path).expect("create oversized file");
    file.set_len((READ_ALL_CAP_BYTES + 1) as u64)
        .expect("size oversized file");
    let source_path = temp.path().join("main.au");
    let source = format!(
        "import fs\n\ndef main() -> int32:\n    match fs.read_to_string(\"{path}\"):\n        case Result.Ok(_):\n            print(\"unexpected-string\")\n        case Result.Err(error):\n            print(error)\n    match fs.read_bytes(\"{path}\"):\n        case Result.Ok(_):\n            print(\"unexpected-bytes\")\n        case Result.Err(error):\n            print(error)\n    return 0\n",
        path = file_path.display()
    );
    fs::write(&source_path, source).expect("write Aurora source");
    let output_path = temp.path().join("out");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build --backend direct");
    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run direct-backend binary");
    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "io.Error.InvalidData\nio.Error.InvalidData\n"
    );
}

#[test]
fn run_caps_fs_read_to_string_and_read_bytes() {
    let temp = TempDir::new("aurora-run-file-read-cap");
    let file_path = temp.path().join("huge.txt");
    let file = fs::File::create(&file_path).expect("create oversized file");
    file.set_len((READ_ALL_CAP_BYTES + 1) as u64)
        .expect("size oversized file");
    let source_path = temp.path().join("main.au");
    let source = format!(
        "import fs\n\ndef main() -> int32:\n    match fs.read_to_string(\"{path}\"):\n        case Result.Ok(_):\n            print(\"unexpected-string\")\n        case Result.Err(error):\n            print(error)\n    match fs.read_bytes(\"{path}\"):\n        case Result.Ok(_):\n            print(\"unexpected-bytes\")\n        case Result.Err(error):\n            print(error)\n    return 0\n",
        path = file_path.display()
    );
    fs::write(&source_path, source).expect("write Aurora source");

    let run = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run");

    assert!(
        run.status.success(),
        "aura run should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "io.Error.InvalidData\nio.Error.InvalidData\n"
    );
}

#[test]
fn run_and_direct_backend_preserve_match_borrow_mut_writebacks_after_dead_branches() {
    let source = r#"enum Opt:
    Some(int32)
    None

def main() -> int32:
    mut x: Opt = Opt.Some(10)
    match borrow mut x:
        case Some(v):
            v = v + 1
            if false:
                x = Opt.Some(100)
        case None:
            pass
    match borrow x:
        case Some(v):
            print(v)
        case None:
            print(-1)
    return 0
"#;

    assert_run_and_direct_source_stdout(
        "aurora-match-borrow-mut-dead-branch-writeback",
        source,
        "11\n",
    );
}

#[test]
fn run_and_direct_backend_preserve_field_match_writeback_across_sibling_mutation() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/match_borrow_mut_field_sibling_write_preserves_writeback.au"
    );
    assert_run_and_direct_source_stdout(
        "aurora-match-borrow-mut-field-sibling-writeback",
        source,
        "9\n11\n",
    );
}

#[test]
fn run_and_direct_backend_match_bare_none_literals_as_option_none() {
    let source = r#"def none_value() -> Option[int32]:
    return None

def main() -> int32:
    a: Option[int32] = None
    match a:
        case Some(value):
            print(value)
        case None:
            print(-1)

    nested: Option[Option[int32]] = Some(None)
    match nested:
        case Some(inner):
            match inner:
                case Some(value):
                    print(value)
                case None:
                    print(-2)
        case None:
            print(-3)

    match none_value():
        case Some(value):
            print(value)
        case None:
            print(-4)

    nested_left: Option[Option[int32]] = Option.Some(None)
    nested_right: Option[Option[int32]] = Option.Some(none_value())
    print(nested_left == nested_right)
    return 0
"#;

    assert_run_and_direct_source_stdout(
        "aurora-bare-none-direct-match",
        source,
        "-1\n-2\n-4\ntrue\n",
    );
}

#[test]
fn mir_and_forced_direct_reject_noncopy_borrowed_return_calls() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/check-fail/borrowed_noncopy_return_call.au"
    );
    let (temp, source_path) = write_temp_source("aurora-borrowed-return-containment", source);
    let expected =
        "produces borrowed non-copy result `String`, which Aurora 0.1 cannot materialize safely";

    let mir = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run forced MIR borrowed-return rejection");
    assert!(!mir.status.success(), "forced MIR should reject the call");
    assert!(
        String::from_utf8_lossy(&mir.stderr).contains(expected),
        "forced MIR diagnostic should explain containment, stderr was:\n{}",
        String::from_utf8_lossy(&mir.stderr)
    );

    let direct = Command::new(aura_bin())
        .args(["build", "--backend", "direct", "-o"])
        .arg(temp.path().join("out"))
        .arg(&source_path)
        .output()
        .expect("failed to run forced direct borrowed-return rejection");
    assert!(
        !direct.status.success(),
        "forced direct should reject the call before code generation"
    );
    assert!(
        String::from_utf8_lossy(&direct.stderr).contains(expected),
        "forced direct diagnostic should explain containment, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
}

#[test]
fn run_and_direct_backend_preserve_bare_none_in_collection_paths_and_nested_options() {
    let source = r#"class Wrap:
    value: Option[Option[int32]]

def print_opt(value: Option[int32]):
    match value:
        case Some(v):
            print(v)
        case None:
            print(-1)

def print_nested(value: Option[Option[int32]]):
    match value:
        case Some(inner):
            match inner:
                case Some(v):
                    print(v)
                case None:
                    print(-2)
        case None:
            print(-3)

def main() -> int32:
    mut pushed = Vec[Option[int32]]()
    pushed.push(None)
    print_opt(pushed[0])

    literal: Vec[Option[int32]] = [None]
    print_opt(literal[0])

    mut values: Vec[Option[int32]] = [Option.Some(7)]
    print_nested(values.set(index=0, value=None))
    print_opt(values[0])

    mut counts: Map[String, Option[int32]] = {"a": Option.Some(1)}
    print_nested(counts.set(key="a", value=None))
    print_opt(counts["a"])

    mut seen: Set[Option[int32]] = Set{}
    seen.insert(None)
    for value in seen:
        print_opt(value)

    jobs = Queue[Option[int32]]()
    jobs.put(None)
    print_opt(jobs.get_or(Option.Some(99)))

    item = Wrap(value=Option.Some(None))
    print_nested(item.value)
    return 0
"#;

    assert_run_and_direct_source_stdout_with_timeout(
        "aurora-bare-none-collections-and-nested-option",
        source,
        std::time::Duration::from_secs(5),
        "-1\n-1\n7\n-1\n1\n-1\n-1\n-1\n-2\n",
    );
}

#[test]
fn check_rejects_match_borrow_mut_binding_use_after_scrutinee_reassign() {
    let source = "enum Opt:\n    Some(int32)\n    None\n\ndef main() -> int32:\n    mut x: Opt = Opt.Some(10)\n    match borrow mut x:\n        case Some(v):\n            x = Opt.Some(v)\n            v = v + 1\n        case None:\n            pass\n    return 0\n";
    let (_temp, source_path) = write_temp_source("aurora-stale-match-binding", source);

    let output = Command::new(aura_bin())
        .arg("check")
        .arg(&source_path)
        .output()
        .expect("failed to run aura check");

    assert!(
        !output.status.success(),
        "stale match-borrow bindings should be rejected"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("cannot use pattern binding `v` after reassigning match scrutinee `x`"),
        "expected stale-binding diagnostic, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn check_and_forced_direct_reject_stale_field_match_binding() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/check-fail/match_borrow_mut_field_binding_use_after_scrutinee_reassign.au"
    );
    let (temp, source_path) = write_temp_source("aurora-stale-field-match-binding", source);
    let expected =
        "cannot use pattern binding `v` after reassigning match scrutinee `holder.state`";

    let checked = Command::new(aura_bin())
        .arg("check")
        .arg(&source_path)
        .output()
        .expect("failed to run aura check");
    assert!(
        !checked.status.success(),
        "aura check should reject stale field bindings"
    );
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains(expected),
        "expected rooted stale-binding diagnostic, stderr was:\n{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let direct = Command::new(aura_bin())
        .args(["build", "--backend", "direct", "-o"])
        .arg(temp.path().join("out"))
        .arg(&source_path)
        .output()
        .expect("failed to run forced direct build");
    assert!(
        !direct.status.success(),
        "forced direct build should reject stale field bindings before code generation"
    );
    assert!(
        String::from_utf8_lossy(&direct.stderr).contains(expected),
        "forced direct diagnostic should retain the rooted field path, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
}

#[test]
fn check_accepts_module_qualified_builtin_io_error_variants() {
    let source =
        "import io\n\ndef main() -> int32:\n    err: io.Error = io.Error.NotFound\n    return 0\n";
    let (_temp, source_path) = write_temp_source("aurora-qualified-io-error", source);

    let output = Command::new(aura_bin())
        .arg("check")
        .arg(&source_path)
        .output()
        .expect("failed to run aura check");

    assert!(
        output.status.success(),
        "qualified io.Error variants should type-check successfully, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_and_direct_backend_preserve_builtin_module_enum_identity() {
    let source = r#"import fs
import io

def main() -> int32:
    print(io.Error.NotFound)
    err: io.Error = io.Error.NotFound
    match err:
        case io.Error.NotFound:
            print(1)
        case _:
            print(2)

    other: io.Error = io.Error.Other(message="miss")
    print(other)
    match other:
        case io.Error.Other(message):
            print(message)
        case _:
            print("nope")

    match fs.read_to_string("/definitely/not/here"):
        case Result.Ok(_):
            print(3)
        case Result.Err(error):
            if error == io.Error.NotFound:
                print(4)
            else:
                print(5)
    return 0
"#;

    assert_run_and_direct_source_stdout(
        "aurora-builtin-module-enum-identity",
        source,
        "io.Error.NotFound\n1\nio.Error.Other(miss)\nmiss\n4\n",
    );
}

#[test]
fn build_with_direct_backend_supports_tcp_echo_example() {
    assert_direct_backend_example_runs("examples/io/tcp_echo.au", "tcp-echo-direct", "echo:ping\n");
}

#[test]
fn build_with_direct_backend_supports_tcp_bytes_example() {
    assert_direct_backend_example_runs("examples/io/tcp_bytes.au", "tcp-bytes-direct", "4\n116\n");
}

#[test]
fn build_with_direct_backend_supports_udp_echo_example() {
    assert_direct_backend_example_runs(
        "examples/io/udp_echo.au",
        "udp-echo-direct",
        "udp:ping\nping\n",
    );
}

#[test]
fn build_with_direct_backend_supports_http_roundtrip_example() {
    assert_direct_backend_example_runs(
        "examples/io/http_roundtrip.au",
        "http-roundtrip-direct",
        "200\nPOST:/hello:body:ok\n",
    );
}

#[test]
fn build_with_direct_backend_supports_websocket_roundtrip_example() {
    assert_direct_backend_example_runs(
        "examples/io/websocket_roundtrip.au",
        "websocket-roundtrip-direct",
        "ws:hi\n",
    );
}

#[cfg(unix)]
#[test]
fn build_with_direct_backend_supports_unix_and_tls_example() {
    assert_direct_backend_example_runs(
        "examples/io/unix_tls_roundtrip.au",
        "unix-tls-roundtrip-direct",
        "unix:ping\n9\n",
    );
}

#[test]
fn build_with_direct_backend_supports_try_and_result_example() {
    assert_direct_backend_example_runs(
        "examples/error_handling/try_result.au",
        "try-result-direct",
        "6\ndivision by zero\n",
    );
}

#[test]
fn build_with_direct_backend_supports_with_cleanup_example() {
    assert_direct_backend_example_runs(
        "examples/resources/with_resource.au",
        "with-direct",
        "demo\nclosed demo\ndone\n",
    );
}

#[test]
fn build_with_direct_backend_supports_trait_dispatch_example() {
    assert_direct_backend_example_runs(
        "examples/traits/greeter.au",
        "greeter-direct",
        "hello aurora\nhello aurora\n",
    );
}

#[test]
fn build_with_direct_backend_supports_multi_type_trait_dispatch_example() {
    assert_direct_backend_example_runs(
        "examples/traits/generic_dispatch_multiple_types.au",
        "multi-trait-dispatch-direct",
        "dog\ncat\n",
    );
}

#[test]
fn build_with_direct_backend_supports_generic_trait_impl_example() {
    assert_direct_backend_example_runs(
        "examples/traits/generic_trait_impl.au",
        "generic-trait-impl-direct",
        "11\n",
    );
}

#[test]
fn build_with_direct_backend_prefers_more_specific_trait_impls() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-trait-specificity",
        "trait Show:\n    def show(borrow self) -> String\n\nclass Box[T]:\n    value: T\n\nimpl[T] Show for Box[T]:\n    def show(borrow self) -> String:\n        return \"generic\"\n\nimpl Show for Box[int32]:\n    def show(borrow self) -> String:\n        return \"int32\"\n\ndef main() -> int32:\n    value = Box(value=7)\n    print(value.show())\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct backend trait specialization should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "int32\n");
}

#[test]
fn build_with_direct_backend_supports_generic_trait_bounds_example() {
    assert_direct_backend_example_runs(
        "examples/traits/generic_trait_bounds.au",
        "generic-trait-bounds-direct",
        "20\n",
    );
}

#[test]
fn build_with_direct_backend_supports_operator_traits_example() {
    assert_direct_backend_example_runs(
        "examples/traits/operator_traits.au",
        "operator-traits-direct",
        "6\n8\n-6\n-8\n",
    );
}

#[test]
fn build_with_direct_backend_supports_ordering_traits_example() {
    assert_direct_backend_example_runs(
        "examples/traits/ordering_traits.au",
        "ordering-traits-direct",
        "true\ntrue\ntrue\ntrue\n2\n",
    );
}

#[test]
fn build_with_direct_backend_supports_generic_data_example() {
    assert_direct_backend_example_runs(
        "examples/generics/box_and_wrapper.au",
        "generic-direct",
        "7\nok\n",
    );
}

#[test]
fn build_with_direct_backend_supports_concurrency_example() {
    assert_direct_backend_example_runs(
        "examples/concurrency/task_group_start.au",
        "queues-direct",
        "2\n4\n6\n",
    );
}

#[test]
fn build_with_direct_backend_supports_queue_timeout_example() {
    assert_direct_backend_example_runs(
        "examples/concurrency/queue_timeout.au",
        "queue-timeout-direct",
        "timeout\n",
    );
}

#[test]
fn build_with_direct_backend_supports_borrow_parameters_example() {
    assert_direct_backend_example_runs(
        "examples/basics/borrow_parameters.au",
        "borrow-params-direct",
        "41\n42\n42\n",
    );
}

#[test]
fn build_with_direct_backend_supports_borrowed_lifetime_labels_example() {
    assert_direct_backend_example_runs(
        "examples/basics/borrowed_lifetime_labels.au",
        "borrowed-lifetime-labels-direct",
        "7\n",
    );
}

#[test]
fn build_with_direct_backend_supports_mutating_methods_example() {
    assert_direct_backend_example_runs(
        "examples/classes/mutating_methods.au",
        "mutating-methods-direct",
        "6\n1\n",
    );
}

#[test]
fn build_with_direct_backend_supports_simple_example() {
    assert_direct_backend_example_runs(
        "examples/basics/simple_example.au",
        "simple-example-direct",
        "Ayoola Olafenwa\n834.6\n",
    );
}

#[test]
fn build_with_direct_backend_supports_generic_constructor_specialization_example() {
    assert_direct_backend_example_runs(
        "examples/generics/generic_constructor_specialization.au",
        "generic-specialization-direct",
        "42\n",
    );
}

#[test]
fn build_with_direct_backend_supports_explicit_builtin_enum_type_args_example() {
    assert_direct_backend_example_runs(
        "examples/enums/explicit_type_args.au",
        "explicit-enum-type-args-direct",
        "7\nbad\n",
    );
}

#[test]
fn build_with_direct_backend_supports_float_return_from_enum_match() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-enum-float-match",
        "enum Value:\n    IntVal(int32)\n    FloatVal(float64)\n\ndef to_float(v: Value) -> float64:\n    match v:\n        case Value.IntVal(i):\n            return 0.0\n        case Value.FloatVal(f):\n            return f\n\ndef main() -> int32:\n    value = Value.FloatVal(2.5)\n    print(to_float(value))\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "2.5\n");
}

#[test]
fn build_with_direct_backend_supports_namespace_import_types_example() {
    assert_direct_backend_example_runs(
        "examples/modules/namespace_import_types.au",
        "namespace-import-types-direct",
        "4\ntrue\n1\n",
    );
}

#[test]
fn build_with_direct_backend_supports_for_range_example() {
    assert_direct_backend_example_runs(
        "examples/control_flow/for_range.au",
        "for-range-direct",
        "7\n",
    );
}

#[test]
fn build_with_direct_backend_supports_literal_match_example() {
    assert_direct_backend_example_runs(
        "examples/control_flow/match_literals.au",
        "match-literals-direct",
        "negative\nzero\nmany\nyes\nno\nrepo\nother\n",
    );
}

#[test]
fn build_with_direct_backend_supports_vec_basics_example() {
    assert_direct_backend_example_runs(
        "examples/collections/vec_basics.au",
        "vec-basics-direct",
        "3\n1\n2\n2\n20\n1\n99\nfalse\n",
    );
}

#[test]
fn build_with_direct_backend_supports_vec_polish_example() {
    assert_direct_backend_example_runs(
        "examples/collections/vec_polish.au",
        "vec-polish-direct",
        "Ada\nGrace\ntrue\n4\n1\n14\n13\n12\n11\ntrue\n100\ntrue\ntrue\n",
    );
}

#[test]
fn build_with_direct_backend_supports_vec_iteration_example() {
    assert_direct_backend_example_runs(
        "examples/collections/vec_iteration.au",
        "vec-iteration-direct",
        "Ada\nGrace\n2\n9\n",
    );
}

#[test]
fn build_with_direct_backend_supports_full_range_uint128_example() {
    assert_direct_backend_example_runs(
        "examples/numbers/uint128_values.au",
        "uint128-direct",
        "340282366920938463463374607431768211455\n340282366920938463463374607431768211455\n",
    );
}

#[test]
fn build_with_direct_backend_supports_bare_none_unit_values() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-none-unit",
        "def noop() -> None:\n    return None\n\ndef main() -> int32:\n    done: None = None\n    noop()\n    print(1)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n");
}

#[test]
fn build_with_direct_backend_supports_vec_literals_and_iteration() {
    let temp = TempDir::new("aurora-build-direct-vec");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "def main() -> int32:\n    mut values = [1, 2]\n    values.push(3)\n    mut total = 0\n    for value in values:\n        total += value\n    print(total)\n    return 0\n",
    )
    .expect("failed to write vec source");
    let output_path = temp.path().join("vec-main");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build --backend direct");

    assert!(
        build.status.success(),
        "direct backend vec build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run vec direct-backend binary");

    assert!(
        run.status.success(),
        "vec direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "6\n");
}

#[test]
fn build_with_direct_backend_supports_vec_methods_and_constructor() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-vec-methods",
        "def print_int_option(value: Option[int32]):\n    match value:\n        case Some(inner):\n            print(inner)\n        case None:\n            print(-1)\n\ndef main() -> int32:\n    values = Vec[int32]()\n    print(values.is_empty())\n    mut items = [1, 2, 3]\n    print(items.len())\n    print_int_option(items.get(1))\n    print_int_option(items.set(index=1, value=20))\n    print_int_option(items.remove(0))\n    items.push(99)\n    print_int_option(items.pop())\n    mut total = 0\n    for value in items:\n        total += value\n    print(total)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "vec direct-backend methods binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true\n3\n2\n2\n1\n99\n23\n"
    );
}

#[test]
fn build_with_direct_backend_supports_string_map_and_numeric_builtins() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-string-map-numbers",
        "def print_int_option(value: Option[int32]):\n    match value:\n        case Some(inner):\n            print(inner)\n        case None:\n            print(-1)\n\ndef main() -> int32:\n    text = \"  aurora repo  \"\n    print(text.len())\n    print(text.contains(\"repo\"))\n    print(text.starts_with(\"  au\"))\n    print(text.ends_with(\"  \"))\n    print(text.trim())\n    print(abs(-7))\n    print(min(9, 2))\n    print(max(4, 12))\n    print(sqrt(81.0))\n    mut counts = {\"aurora\": 1, \"codex\": 2}\n    print(counts.len())\n    print(counts.contains_key(\"aurora\"))\n    print_int_option(counts.get(\"aurora\"))\n    print_int_option(counts.set(key=\"aurora\", value=5))\n    print(counts[\"aurora\"])\n    print(counts.keys().len())\n    print(counts.values().len())\n    print_int_option(counts.remove(\"codex\"))\n    print(counts.is_empty())\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct backend string/map/numbers binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "15\ntrue\ntrue\ntrue\naurora repo\n7\n2\n12\n9.0\n2\ntrue\n1\n1\n5\n2\n2\n2\nfalse\n"
    );
}

#[test]
fn build_with_direct_backend_supports_queue_timeout_matches() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-queue-timeout",
        "def main() -> int32:\n    ch = Queue[int32]()\n    match ch.get(timeout=1ms):\n        case QueueReceive.Item(v):\n            print(v)\n        case QueueReceive.Closed:\n            print(1)\n        case QueueReceive.TimedOut:\n            print(2)\n        case QueueReceive.Cancelled:\n            print(3)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend queue timeout binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n");
}

#[test]
fn built_direct_binaries_render_runtime_errors_with_source_context() {
    let temp = TempDir::new("aurora-build-direct-runtime-diag");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "def main() -> int32:\n    print(1 / 0)\n    return 0\n",
    )
    .expect("failed to write runtime-error source");
    let output_path = temp.path().join("out");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build --backend direct");

    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run direct-backend binary");

    assert!(
        !run.status.success(),
        "direct-backend runtime-error binary should fail"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("error: division by zero"));
    assert!(stderr.contains(&format!("{}:2:11", source_path.display())));
    assert!(stderr.contains("|"));
    assert!(stderr.contains("^"));
}

#[test]
fn default_build_supports_simple_example() {
    assert_default_backend_example_runs(
        "examples/basics/simple_example.au",
        "simple-example-auto",
        "Ayoola Olafenwa\n834.6\n",
    );
}

#[test]
fn default_build_supports_borrowed_lifetime_labels_example() {
    assert_default_backend_example_runs(
        "examples/basics/borrowed_lifetime_labels.au",
        "borrowed-lifetime-labels-auto",
        "7\n",
    );
}

#[test]
fn default_build_supports_literal_match_example() {
    assert_default_backend_example_runs(
        "examples/control_flow/match_literals.au",
        "match-literals-auto",
        "negative\nzero\nmany\nyes\nno\nrepo\nother\n",
    );
}

#[test]
fn default_build_supports_vec_basics_example() {
    assert_default_backend_example_runs(
        "examples/collections/vec_basics.au",
        "vec-basics-auto",
        "3\n1\n2\n2\n20\n1\n99\nfalse\n",
    );
}

#[test]
fn default_build_supports_vec_polish_example() {
    assert_default_backend_example_runs(
        "examples/collections/vec_polish.au",
        "vec-polish-auto",
        "Ada\nGrace\ntrue\n4\n1\n14\n13\n12\n11\ntrue\n100\ntrue\ntrue\n",
    );
}

#[test]
fn default_build_supports_map_basics_example() {
    assert_default_backend_example_runs(
        "examples/collections/map_basics.au",
        "map-basics-auto",
        "3\ntrue\n1\n1\n5\naurora\n3\n3\n3\n3\ntrue\n",
    );
}

#[test]
fn default_build_supports_generic_trait_bounds_example() {
    assert_default_backend_example_runs(
        "examples/traits/generic_trait_bounds.au",
        "generic-trait-bounds-auto",
        "20\n",
    );
}

#[test]
fn default_build_supports_operator_traits_example() {
    assert_default_backend_example_runs(
        "examples/traits/operator_traits.au",
        "operator-traits-auto",
        "6\n8\n-6\n-8\n",
    );
}

#[test]
fn default_build_supports_ordering_traits_example() {
    assert_default_backend_example_runs(
        "examples/traits/ordering_traits.au",
        "ordering-traits-auto",
        "true\ntrue\ntrue\ntrue\n2\n",
    );
}

#[test]
fn default_build_supports_set_basics_example() {
    assert_default_backend_example_runs(
        "examples/collections/set_basics.au",
        "set-basics-auto",
        "3\ntrue\nfalse\ntrue\ntrue\n9\ntrue\ntrue\n1\n",
    );
}

#[test]
fn default_build_supports_vec_iteration_example() {
    assert_default_backend_example_runs(
        "examples/collections/vec_iteration.au",
        "vec-iteration-auto",
        "Ada\nGrace\n2\n9\n",
    );
}

#[test]
fn default_build_supports_generic_constructor_specialization_example() {
    assert_default_backend_example_runs(
        "examples/generics/generic_constructor_specialization.au",
        "generic-specialization-auto",
        "42\n",
    );
}

#[test]
fn default_build_supports_string_methods_example() {
    assert_default_backend_example_runs(
        "examples/strings/string_methods.au",
        "string-methods-auto",
        "15\ntrue\ntrue\ntrue\naurora repo\n2\naurora\nrepo\naurora lang\naurora repo\nAURORA REPO\nrepo\nnone\naurora\nnone\n11\n",
    );
}

#[test]
fn default_build_supports_numeric_builtins_example() {
    assert_default_backend_example_runs(
        "examples/numbers/numeric_builtins.au",
        "numeric-builtins-auto",
        "7\n3.5\n2\n12\n9.0\n9.0\n",
    );
}

#[test]
fn default_build_supports_string_parsing_and_formatting_example() {
    assert_default_backend_example_runs(
        "examples/strings/string_parsing_and_formatting.au",
        "string-parsing-formatting-auto",
        "42\n-9000000000\n3.5\ntrue\naurora-lang-tests\ntrue\n12\n4\n9\n3.0\n",
    );
}

#[test]
fn default_build_supports_file_io_example() {
    assert_default_backend_example_runs(
        "examples/io/read_text_file.au",
        "file-io-auto",
        "true\ntrue\n",
    );
}

#[test]
fn default_build_supports_bytes_file_io_example() {
    assert_default_backend_example_runs(
        "examples/io/bytes_file_io.au",
        "bytes-file-io-auto",
        "4\n65\n67\n5\n68\n",
    );
}

#[test]
fn default_build_supports_tcp_echo_example() {
    assert_default_backend_example_runs("examples/io/tcp_echo.au", "tcp-echo-auto", "echo:ping\n");
}

#[test]
fn default_build_supports_tcp_bytes_example() {
    assert_default_backend_example_runs("examples/io/tcp_bytes.au", "tcp-bytes-auto", "4\n116\n");
}

#[test]
fn default_build_supports_udp_echo_example() {
    assert_default_backend_example_runs(
        "examples/io/udp_echo.au",
        "udp-echo-auto",
        "udp:ping\nping\n",
    );
}

#[test]
fn default_build_supports_http_roundtrip_example() {
    assert_default_backend_example_runs(
        "examples/io/http_roundtrip.au",
        "http-roundtrip-auto",
        "200\nPOST:/hello:body:ok\n",
    );
}

#[test]
fn default_build_supports_websocket_roundtrip_example() {
    assert_default_backend_example_runs(
        "examples/io/websocket_roundtrip.au",
        "websocket-roundtrip-auto",
        "ws:hi\n",
    );
}

#[cfg(unix)]
#[test]
fn default_build_supports_unix_and_tls_example() {
    assert_default_backend_example_runs(
        "examples/io/unix_tls_roundtrip.au",
        "unix-tls-roundtrip-auto",
        "unix:ping\n9\n",
    );
}

#[test]
fn default_build_supports_explicit_builtin_enum_type_args_example() {
    assert_default_backend_example_runs(
        "examples/enums/explicit_type_args.au",
        "explicit-enum-type-args-auto",
        "7\nbad\n",
    );
}

#[test]
fn default_build_supports_float_return_from_enum_match() {
    let (_, run) = build_and_run_default_source(
        "aurora-build-auto-enum-float-match",
        "enum Value:\n    IntVal(int32)\n    FloatVal(float64)\n\ndef to_float(v: Value) -> float64:\n    match v:\n        case Value.IntVal(i):\n            return 0.0\n        case Value.FloatVal(f):\n            return f\n\ndef main() -> int32:\n    value = Value.FloatVal(2.5)\n    print(to_float(value))\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "default-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "2.5\n");
}

#[test]
fn default_build_supports_namespace_import_types_example() {
    assert_default_backend_example_runs(
        "examples/modules/namespace_import_types.au",
        "namespace-import-types-auto",
        "4\ntrue\n1\n",
    );
}

#[test]
fn default_build_supports_generic_trait_impl_example() {
    assert_default_backend_example_runs(
        "examples/traits/generic_trait_impl.au",
        "generic-trait-impl-auto",
        "11\n",
    );
}

#[test]
fn default_build_supports_bare_none_unit_values() {
    let (_, run) = build_and_run_default_source(
        "aurora-build-auto-none-unit",
        "def noop() -> None:\n    return None\n\ndef main() -> int32:\n    done: None = None\n    noop()\n    print(1)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "default-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n");
}

#[test]
fn default_build_supports_queue_timeout_matches() {
    let (_, run) = build_and_run_default_source(
        "aurora-build-auto-queue-timeout",
        "def main() -> int32:\n    ch = Queue[int32]()\n    match ch.get(timeout=1ms):\n        case QueueReceive.Item(v):\n            print(v)\n        case QueueReceive.Closed:\n            print(1)\n        case QueueReceive.TimedOut:\n            print(2)\n        case QueueReceive.Cancelled:\n            print(3)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "default-backend queue timeout binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n");
}

#[test]
fn built_default_binaries_render_runtime_errors_with_source_context() {
    let temp = TempDir::new("aurora-build-auto-runtime-diag");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "def main() -> int32:\n    print(1 / 0)\n    return 0\n",
    )
    .expect("failed to write runtime-error source");
    let output_path = temp.path().join("out");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build");

    assert!(
        build.status.success(),
        "default backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run default-backend binary");

    assert!(
        !run.status.success(),
        "default-backend runtime-error binary should fail"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("error: division by zero"));
    assert!(stderr.contains(&format!("{}:2:11", source_path.display())));
    assert!(stderr.contains("|"));
    assert!(stderr.contains("^"));
}

#[test]
fn build_with_direct_backend_supports_float_comparisons_in_conditions() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-float-cmp",
        "def main() -> int32:\n    x: float64 = 3.0\n    y: float64 = 3.0\n    if x == y:\n        print(\"equal\")\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "equal\n");
}

#[test]
fn build_with_direct_backend_supports_float_modulo() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-float-mod",
        "def main() -> int32:\n    x: float64 = 10.0\n    y: float64 = 3.0\n    print(x % y)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "1.0\n");
}

#[test]
fn build_with_direct_backend_runs_with_cleanup_on_normal_scope_exit() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-with-normal-exit",
        "class Handle:\n    name: String\n\n    def close(borrow mut self):\n        print(\"closing \" + self.name)\n\ndef main() -> int32:\n    with h = Handle(name=\"db\"):\n        print(\"inside with\")\n    print(\"after with\")\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "inside with\nclosing db\nafter with\n"
    );
}

#[test]
fn build_with_direct_backend_preserves_scalar_return_values_through_with_cleanup() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-with-return",
        "class Handle:\n    name: String\n\n    def close(borrow mut self):\n        print(\"closing \" + self.name)\n\ndef process() -> int32:\n    with h = Handle(name=\"file\"):\n        return 42\n    return 0\n\ndef main() -> int32:\n    print(process())\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "closing file\n42\n");
}

#[test]
fn build_with_direct_backend_prints_boolean_values_as_true_and_false() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-print-bool",
        "def main() -> int32:\n    print(1 == 1)\n    print(1 == 2)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true\nfalse\n");
}

#[test]
fn build_with_direct_backend_rejects_narrow_integer_overflow_at_runtime() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-int8-overflow",
        "def main() -> int32:\n    a: int8 = 127\n    b: int8 = 1\n    c = a + b\n    print(c)\n    return 0\n",
    );

    assert!(
        !run.status.success(),
        "direct-backend binary should reject int8 overflow"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("integer value `128` does not fit in `int8`"),
        "direct-backend overflow should explain the failing int8 value, stderr was:\n{}",
        stderr
    );
}

#[test]
fn build_with_direct_backend_supports_trait_impls_on_builtin_types() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-builtin-trait",
        "trait Show:\n    def show(borrow self) -> String\n\nimpl Show for int32:\n    def show(borrow self) -> String:\n        return \"int\"\n\ndef main() -> int32:\n    value: int32 = 7\n    print(value.show())\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "int\n");
}

#[test]
fn build_with_direct_backend_runs_indirect_recursive_example() {
    assert_direct_backend_example_runs(
        "examples/classes/indirect_recursive.au",
        "indirect-recursive-direct",
        "2\n",
    );
}

#[test]
fn build_runs_indirect_recursive_example() {
    assert_default_backend_example_runs(
        "examples/classes/indirect_recursive.au",
        "indirect-recursive-default",
        "2\n",
    );
}

#[test]
fn build_with_direct_backend_supports_task_result_returning_plain_classes() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-task-result-class",
        "class Box:\n    value: int32\n\ndef make_box() -> Box:\n    return Box(value=7)\n\ndef main() -> int32:\n    with TaskGroup() as group:\n        task = group.start(make_box)\n        match task.result():\n            case TaskResult.Ready(box):\n                print(box.value)\n            case TaskResult.Error(_message):\n                print(0)\n            case TaskResult.TimedOut:\n                print(0)\n            case TaskResult.Cancelled:\n                print(0)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend task result binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
}

#[test]
fn build_supports_task_result_returning_plain_classes() {
    let (temp, source_path) = write_temp_source(
        "aurora-build-default-task-result-class",
        "class Box:\n    value: int32\n\ndef make_box() -> Box:\n    return Box(value=7)\n\ndef main() -> int32:\n    with TaskGroup() as group:\n        task = group.start(make_box)\n        match task.result():\n            case TaskResult.Ready(box):\n                print(box.value)\n            case TaskResult.Error(_message):\n                print(0)\n            case TaskResult.TimedOut:\n                print(0)\n            case TaskResult.Cancelled:\n                print(0)\n    return 0\n",
    );
    let output_path = temp.path().join("out");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build");

    assert!(
        build.status.success(),
        "default build should support task result returning plain classes, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run built binary");

    assert!(
        run.status.success(),
        "built binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
}

#[test]
fn build_produces_runnable_concurrency_binary() {
    let fixture = repo_root().join("examples/concurrency/task_group_start.au");
    let output_dir = TempDir::new("aurora-build-concurrency");
    let output_path = output_dir.path().join("task-group-start");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&fixture)
        .output()
        .expect("failed to run aura build for concurrency example");

    assert!(
        build.status.success(),
        "build should succeed for concurrency example, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run built concurrency output");

    assert!(
        run.status.success(),
        "built concurrency binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n4\n6\n");
}

#[test]
fn build_from_stdin_produces_runnable_module_binary() {
    let temp = TempDir::new("aurora-cli-stdin-build-modules");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def triple(value: int32) -> int32:\n    return value * 3\n",
    )
    .expect("failed to write helper module");
    let main_path = temp.path().join("main.au");
    let source = "import helpers.math\n\ndef main() -> int32:\n    print(helpers.math.triple(value=5))\n    return 0\n";
    let output_path = temp.path().join("stdin-built-modules");

    let mut child = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura build for stdin module program");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let build = child
        .wait_with_output()
        .expect("failed to collect stdin build output");

    assert!(
        build.status.success(),
        "stdin build should succeed for module program, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run built stdin module program");

    assert!(
        run.status.success(),
        "built stdin module binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "15\n");
}

#[test]
fn built_binary_runs_after_source_file_is_removed() {
    let temp = TempDir::new("aurora-cli-build-source-removal");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "def main() -> int32:\n    print(value=21 * 2)\n    return 0\n",
    )
    .expect("failed to write source program");
    let output_path = temp.path().join("no-source-needed");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build for source-removal test");

    assert!(
        build.status.success(),
        "build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    fs::remove_file(&source_path).expect("failed to remove source after build");

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run built binary after source removal");

    assert!(
        run.status.success(),
        "built binary should not depend on source files at runtime, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn built_binary_exits_cleanly_when_stdout_pipe_closes() {
    let fixture = repo_root().join("examples/point.au");
    let output_dir = TempDir::new("aurora-build-broken-pipe");
    let output_path = output_dir.path().join("point");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&fixture)
        .output()
        .expect("failed to run aura build");

    assert!(
        build.status.success(),
        "build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let mut child = generated_binary(&output_path)
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn built binary");

    drop(child.stdout.take());

    let status = child
        .wait()
        .expect("failed to wait for built binary after broken pipe");
    assert!(
        status.success(),
        "built binary should exit cleanly when stdout closes early"
    );
}

#[test]
fn run_executes_supported_programs() {
    let fixture = repo_root().join("examples/classes/methods.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run");

    assert!(
        output.status.success(),
        "run should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "4\n8\n0\n");
}

#[test]
fn run_executes_generic_constructor_specialization_example() {
    let fixture = repo_root().join("examples/generics/generic_constructor_specialization.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on generic constructor specialization example");

    assert!(
        output.status.success(),
        "run should succeed for generic constructor specialization example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
}

#[test]
fn run_executes_generic_trait_impl_example() {
    let fixture = repo_root().join("examples/traits/generic_trait_impl.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on generic trait impl example");

    assert!(
        output.status.success(),
        "run should succeed for generic trait impl example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "11\n");
}

#[test]
fn run_executes_try_example() {
    let fixture = repo_root().join("examples/error_handling/try_result.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on try example");

    assert!(
        output.status.success(),
        "run should succeed for try example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "6\ndivision by zero\n"
    );
}

#[test]
fn run_executes_with_example() {
    let fixture = repo_root().join("examples/resources/with_resource.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on with example");

    assert!(
        output.status.success(),
        "run should succeed for with example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "demo\nclosed demo\ndone\n"
    );
}

#[test]
fn run_executes_borrowed_lifetime_labels_example() {
    let fixture = repo_root().join("examples/basics/borrowed_lifetime_labels.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on borrowed lifetime labels example");

    assert!(
        output.status.success(),
        "run should succeed for borrowed lifetime labels example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n");
}

#[test]
fn run_executes_literal_match_example() {
    let fixture = repo_root().join("examples/control_flow/match_literals.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on literal match example");

    assert!(
        output.status.success(),
        "run should succeed for literal match example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "negative\nzero\nmany\nyes\nno\nrepo\nother\n"
    );
}

#[test]
fn run_executes_vec_basics_example() {
    let fixture = repo_root().join("examples/collections/vec_basics.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on vec basics example");

    assert!(
        output.status.success(),
        "run should succeed for vec basics example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "3\n1\n2\n2\n20\n1\n99\nfalse\n"
    );
}

#[test]
fn run_executes_vec_polish_example() {
    let fixture = repo_root().join("examples/collections/vec_polish.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on vec polish example");

    assert!(
        output.status.success(),
        "run should succeed for vec polish example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Ada\nGrace\ntrue\n4\n1\n14\n13\n12\n11\ntrue\n100\ntrue\ntrue\n"
    );
}

#[test]
fn run_executes_vec_iteration_example() {
    let fixture = repo_root().join("examples/collections/vec_iteration.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on vec iteration example");

    assert!(
        output.status.success(),
        "run should succeed for vec iteration example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Ada\nGrace\n2\n9\n"
    );
}

#[test]
fn run_executes_vec_literals_and_iteration() {
    let temp = TempDir::new("aurora-run-vec");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "def main() -> int32:\n    mut values = [1, 2]\n    values.push(3)\n    mut total = 0\n    for value in values:\n        total += value\n    print(total)\n    return 0\n",
    )
    .expect("failed to write vec source");

    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run");

    assert!(
        output.status.success(),
        "run vec execution should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "6\n");
}

#[test]
fn run_executes_vec_methods_and_constructor() {
    let temp = TempDir::new("aurora-run-vec-methods");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "def print_int_option(value: Option[int32]):\n    match value:\n        case Some(inner):\n            print(inner)\n        case None:\n            print(-1)\n\ndef main() -> int32:\n    values = Vec[int32]()\n    print(values.is_empty())\n    mut items = [1, 2, 3]\n    print(items.len())\n    print_int_option(items.get(1))\n    print_int_option(items.set(index=1, value=20))\n    print_int_option(items.remove(0))\n    items.push(99)\n    print_int_option(items.pop())\n    mut total = 0\n    for value in items:\n        total += value\n    print(total)\n    return 0\n",
    )
    .expect("failed to write vec methods source");

    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run");

    assert!(
        output.status.success(),
        "run vec methods execution should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "true\n3\n2\n2\n1\n99\n23\n"
    );
}

#[test]
fn run_executes_map_basics_example() {
    let fixture = repo_root().join("examples/collections/map_basics.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on map basics example");

    assert!(
        output.status.success(),
        "run should succeed for map basics example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "3\ntrue\n1\n1\n5\naurora\n3\n3\n3\n3\ntrue\n"
    );
}

#[test]
fn run_executes_generic_trait_bounds_example() {
    let fixture = repo_root().join("examples/traits/generic_trait_bounds.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on generic trait bounds example");

    assert!(
        output.status.success(),
        "run should succeed for generic trait bounds example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "20\n");
}

#[test]
fn run_executes_operator_traits_example() {
    let fixture = repo_root().join("examples/traits/operator_traits.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on operator traits example");

    assert!(
        output.status.success(),
        "run should succeed for operator traits example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "6\n8\n-6\n-8\n");
}

#[test]
fn run_executes_ordering_traits_example() {
    let fixture = repo_root().join("examples/traits/ordering_traits.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on ordering traits example");

    assert!(
        output.status.success(),
        "run should succeed for ordering traits example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "true\ntrue\ntrue\ntrue\n2\n"
    );
}

#[test]
fn run_executes_set_basics_example() {
    let fixture = repo_root().join("examples/collections/set_basics.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on set basics example");

    assert!(
        output.status.success(),
        "run should succeed for set basics example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "3\ntrue\nfalse\ntrue\ntrue\n9\ntrue\ntrue\n1\n"
    );
}

#[test]
fn run_executes_string_methods_example() {
    let fixture = repo_root().join("examples/strings/string_methods.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on string methods example");

    assert!(
        output.status.success(),
        "run should succeed for string methods example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "15\ntrue\ntrue\ntrue\naurora repo\n2\naurora\nrepo\naurora lang\naurora repo\nAURORA REPO\nrepo\nnone\naurora\nnone\n11\n"
    );
}

#[test]
fn run_executes_numeric_builtins_example() {
    let fixture = repo_root().join("examples/numbers/numeric_builtins.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on numeric builtins example");

    assert!(
        output.status.success(),
        "run should succeed for numeric builtins example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "7\n3.5\n2\n12\n9.0\n9.0\n"
    );
}

#[test]
fn run_executes_string_parsing_and_formatting_example() {
    let fixture = repo_root().join("examples/strings/string_parsing_and_formatting.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on string parsing example");

    assert!(
        output.status.success(),
        "run should succeed for string parsing example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "42\n-9000000000\n3.5\ntrue\naurora-lang-tests\ntrue\n12\n4\n9\n3.0\n"
    );
}

#[test]
fn run_executes_file_io_example() {
    let fixture = repo_root().join("examples/io/read_text_file.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on file io example");

    assert!(
        output.status.success(),
        "run should succeed for file io example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "true\ntrue\n");
}

#[test]
fn run_executes_bytes_file_io_example() {
    let fixture = repo_root().join("examples/io/bytes_file_io.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on bytes file io example");

    assert!(
        output.status.success(),
        "run should succeed for bytes file io example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "4\n65\n67\n5\n68\n"
    );
}

#[test]
fn run_executes_tcp_echo_example() {
    let fixture = repo_root().join("examples/io/tcp_echo.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on tcp echo example");

    assert!(
        output.status.success(),
        "run should succeed for tcp echo example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "echo:ping\n");
}

#[test]
fn run_executes_tcp_bytes_example() {
    let fixture = repo_root().join("examples/io/tcp_bytes.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on tcp bytes example");

    assert!(
        output.status.success(),
        "run should succeed for tcp bytes example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "4\n116\n");
}

#[test]
fn run_executes_udp_echo_example() {
    let fixture = repo_root().join("examples/io/udp_echo.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on udp echo example");

    assert!(
        output.status.success(),
        "run should succeed for udp echo example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "udp:ping\nping\n");
}

#[test]
fn run_executes_http_roundtrip_example() {
    let fixture = repo_root().join("examples/io/http_roundtrip.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on http roundtrip example");

    assert!(
        output.status.success(),
        "run should succeed for http roundtrip example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "200\nPOST:/hello:body:ok\n"
    );
}

#[test]
fn run_executes_websocket_roundtrip_example() {
    let fixture = repo_root().join("examples/io/websocket_roundtrip.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on websocket roundtrip example");

    assert!(
        output.status.success(),
        "run should succeed for websocket roundtrip example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ws:hi\n");
}

#[cfg(unix)]
#[test]
fn run_executes_unix_and_tls_roundtrip_example() {
    let fixture = repo_root().join("examples/io/unix_tls_roundtrip.au");
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run on unix/tls roundtrip example");

    assert!(
        output.status.success(),
        "run should succeed for unix/tls roundtrip example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "unix:ping\n9\n");
}

#[test]
fn run_executes_string_map_and_numeric_builtins() {
    let temp = TempDir::new("aurora-run-string-map-numbers");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "def print_int_option(value: Option[int32]):\n    match value:\n        case Some(inner):\n            print(inner)\n        case None:\n            print(-1)\n\ndef main() -> int32:\n    text = \"  aurora repo  \"\n    print(text.len())\n    print(text.contains(\"repo\"))\n    print(text.starts_with(\"  au\"))\n    print(text.ends_with(\"  \"))\n    print(text.trim())\n    print(abs(-7))\n    print(min(9, 2))\n    print(max(4, 12))\n    print(sqrt(81.0))\n    mut counts = {\"aurora\": 1, \"codex\": 2}\n    print(counts.len())\n    print(counts.contains_key(\"aurora\"))\n    print_int_option(counts.get(\"aurora\"))\n    print_int_option(counts.set(key=\"aurora\", value=5))\n    print(counts[\"aurora\"])\n    print(counts.keys().len())\n    print(counts.values().len())\n    print_int_option(counts.remove(\"codex\"))\n    print(counts.is_empty())\n    return 0\n",
    )
    .expect("failed to write string/map/numbers source");

    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run");

    assert!(
        output.status.success(),
        "run string/map/numbers execution should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "15\ntrue\ntrue\ntrue\naurora repo\n7\n2\n12\n9.0\n2\ntrue\n1\n1\n5\n2\n2\n2\nfalse\n"
    );
}

#[test]
fn run_executes_programs_with_local_modules() {
    let temp = TempDir::new("aurora-cli-modules-run");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def add(left: int32, right: int32) -> int32:\n    return left + right\n",
    )
    .expect("failed to write helper module");
    fs::write(
        temp.path().join("main.au"),
        "from helpers.math import add\n\ndef main() -> int32:\n    print(add(left=3, right=4))\n    return 0\n",
    )
    .expect("failed to write main module");

    let output = Command::new(aura_bin())
        .arg("run")
        .arg(temp.path().join("main.au"))
        .output()
        .expect("failed to run aura on module program");

    assert!(
        output.status.success(),
        "run should succeed for module program, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n");
}

#[test]
fn module_qualified_spawn_target_runs_across_commands() {
    let temp = TempDir::new("aurora-cli-qualified-task-start");
    fs::create_dir_all(temp.path().join("pkg")).expect("failed to create module dir");
    fs::write(
        temp.path().join("pkg/helpers.au"),
        "public def work() -> int32:\n    return 1\n",
    )
    .expect("failed to write helper module");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "import pkg.helpers\n\ndef main() -> int32:\n    with TaskGroup() as group:\n        task = group.start(pkg.helpers.work)\n        match task.result():\n            case TaskResult.Ready(value):\n                print(value)\n            case TaskResult.Error(_message):\n                print(0)\n            case TaskResult.TimedOut:\n                print(0)\n            case TaskResult.Cancelled:\n                print(0)\n    return 0\n",
    )
    .expect("failed to write main module");

    let check = Command::new(aura_bin())
        .arg("check")
        .arg(&source_path)
        .output()
        .expect("failed to run aura check");
    assert!(
        check.status.success(),
        "check should accept module-qualified task start targets, stderr was:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    for command in ["run"] {
        let output = Command::new(aura_bin())
            .arg(command)
            .arg(&source_path)
            .output()
            .expect("failed to run aura command");

        assert!(
            output.status.success(),
            "{} should execute module-qualified task start targets, stderr was:\n{}",
            command,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout) == "1\n",
            "{} should print the spawned result, stdout was:\n{}",
            command,
            String::from_utf8_lossy(&output.stdout)
        );
    }

    for backend in ["auto", "direct"] {
        let output_path = temp.path().join(format!("out-{backend}"));
        let build = Command::new(aura_bin())
            .arg("build")
            .arg("--backend")
            .arg(backend)
            .arg("-o")
            .arg(&output_path)
            .arg(&source_path)
            .output()
            .expect("failed to run aura build");

        assert!(
            build.status.success(),
            "build --backend {backend} should accept module-qualified task start targets, stderr was:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let run = generated_binary(&output_path)
            .output()
            .expect("failed to run built task binary");
        assert!(
            run.status.success(),
            "built binary for backend {backend} should succeed, stderr was:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n");
    }
}

#[test]
fn run_handles_long_binary_expression_chains_quickly() {
    let terms = (1..=24)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(" + ");
    let source = format!(
        "def main() -> int32:\n    result = {}\n    print(result)\n    return 0\n",
        terms
    );
    let (_temp, source_path) = write_temp_source("aurora-cli-long-expr", &source);

    let mut child = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura run on long expression");

    let status = wait_with_timeout(&mut child, std::time::Duration::from_secs(2));
    if status.is_none() {
        child.kill().expect("failed to kill timed out aura run");
    }
    let output = child
        .wait_with_output()
        .expect("failed to collect aura run output for long expression");

    assert!(
        status.is_some(),
        "run should finish quickly for long binary expression chains"
    );
    assert!(
        output.status.success(),
        "run should succeed for long binary expression chains, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "300\n");
}

#[test]
fn build_produces_runnable_binary_for_program_with_local_modules() {
    let temp = TempDir::new("aurora-cli-modules-build");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write helper module");
    fs::write(
        temp.path().join("main.au"),
        "import helpers.math\n\ndef main() -> int32:\n    print(helpers.math.double(value=5))\n    return 0\n",
    )
    .expect("failed to write main module");
    let output_path = temp.path().join("aurora-modules");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(temp.path().join("main.au"))
        .output()
        .expect("failed to build module program");

    assert!(
        build.status.success(),
        "build should succeed for module program, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run built module program");

    assert!(
        run.status.success(),
        "built module binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "10\n");
}

#[test]
fn build_executes_multiple_specialized_trait_impl_dispatch() {
    let source = r#"trait Show:
    def show(borrow self) -> String

class Box[T]:
    value: T

impl Show for Box[int32]:
    def show(borrow self) -> String:
        return f"{self.value}"

impl Show for Box[String]:
    def show(borrow self) -> String:
        return self.value.clone()

def render[T: Show](value: T) -> None:
    print(value.show())

def main() -> int32:
    render(Box(value=7))
    render(Box(value="hi"))
    return 0
"#;
    let (temp, source_path) = write_temp_source("aurora-cli-build-specialized-trait-impls", source);
    let output_path = temp.path().join("specialized-trait-impls");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to build specialized trait impl program");

    assert!(
        build.status.success(),
        "build should succeed for multiple specialized trait impls, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run built specialized trait impl program");

    assert!(
        run.status.success(),
        "built specialized trait impl binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\nhi\n");
}

#[test]
fn build_executes_nested_generic_trait_bound_dispatch() {
    let source = r#"trait Add2[Rhs, Out]:
    def add2(borrow self, rhs: Rhs) -> Out

class Box[T]:
    value: T

impl Add2[int32, int32] for int32:
    def add2(borrow self, rhs: int32) -> int32:
        return self + rhs

impl[T: Add2[T, T]] Add2[Box[T], Box[T]] for Box[T]:
    def add2(borrow self, rhs: Box[T]) -> Box[T]:
        return Box(value=self.value.add2(rhs=rhs.value))

def main() -> int32:
    left: Box[int32] = Box(value=3)
    right: Box[int32] = Box(value=4)
    result: Box[int32] = left.add2(rhs=right)
    print(result.value)
    return 0
"#;
    let (temp, source_path) = write_temp_source(
        "aurora-cli-build-nested-generic-trait-bound-dispatch",
        source,
    );
    let output_path = temp.path().join("nested-generic-trait-bound-dispatch");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to build nested generic trait bound program");

    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run nested generic trait bound program");

    assert!(
        run.status.success(),
        "direct-backend binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
}

#[test]
fn build_executes_trait_impl_associated_methods() {
    let source = r#"trait Factory:
    def make() -> int32

class Widget:
    value: int32

impl Factory for Widget:
    def make() -> int32:
        return 7

def main() -> int32:
    print(Widget.make())
    return 0
"#;
    let (temp, source_path) =
        write_temp_source("aurora-cli-build-trait-associated-methods", source);
    let output_path = temp.path().join("trait-associated-methods");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to build trait impl associated method program");

    assert!(
        build.status.success(),
        "build should succeed for trait impl associated methods, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run built trait impl associated method program");

    assert!(
        run.status.success(),
        "built trait impl associated method binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
}

#[test]
fn direct_backend_build_supports_advanced_io_and_network_surface() {
    let temp = TempDir::new("aurora-cli-direct-advanced-io-net");
    let file_path = temp.path().join("data.bin");
    let source = format!(
        r#"import io
import fs
import net

def serve_udp(socket: net.UdpSocket) -> Result[String, io.Error]:
    with server_socket = socket:
        match try server_socket.recv_from(1024, timeout=1s):
            case Option.Some(packet):
                text = try packet.text()
                try server_socket.send_text(packet.address(), "udp:" + text, timeout=1s)
                return Result.Ok(text)
            case Option.None:
                return Result.Ok("missing")

def serve_http(listener: net.HttpListener) -> Result[None, io.Error]:
    with server_listener = listener:
        exchange = try server_listener.accept(timeout=1s)
        with request = exchange:
            body = try request.body_text()
            headers = request.headers()
            try request.respond_text(200, request.method() + ":" + request.path() + ":" + body + ":" + headers["X-Test"], {{"Content-Type": "text/plain"}})
            return Result.Ok(None)

def serve_http_bytes(listener: net.HttpListener) -> Result[None, io.Error]:
    with server_listener = listener:
        exchange = try server_listener.accept(timeout=1s)
        with request = exchange:
            body = request.body_bytes()
            try request.respond_bytes(202, body, {{"Content-Type": "application/octet-stream"}})
            return Result.Ok(None)

def serve_ws(listener: net.WebSocketListener) -> Result[None, io.Error]:
    with server_listener = listener:
        socket = try server_listener.accept(timeout=1s)
        with server_socket = socket:
            match try server_socket.recv_text(timeout=1s):
                case Option.Some(text):
                    try server_socket.send_text("ws:" + text, timeout=1s)
                    return Result.Ok(None)
                case Option.None:
                    return Result.Ok(None)

def run() -> Result[None, io.Error]:
    bytes: Vec[uint8] = [65 as uint8, 66 as uint8]
    try fs.write_bytes("{path}", bytes)
    try fs.append_bytes("{path}", [67 as uint8, 10 as uint8])
    read_back = try fs.read_bytes("{path}")
    print(read_back.len())
    print(read_back[0])
    print(read_back[2])

    with TaskGroup() as group:
        udp_listener = try net.udp_bind("127.0.0.1:0")
        udp_addr = try udp_listener.local_addr()
        udp_task = group.start(serve_udp, udp_listener)
        udp_client = try net.udp_bind("127.0.0.1:0")
        with client_socket = udp_client:
            try client_socket.send_text(udp_addr, "ping", timeout=1s)
            match try client_socket.recv_from(1024, timeout=1s):
                case Option.Some(packet):
                    print(try packet.text())
                case Option.None:
                    return Result.Ok(None)
        match udp_task.result():
            case TaskResult.Ready(result):
                match result:
                    case Result.Ok(text):
                        print(text)
                    case Result.Err(error):
                        return Result.Err(error)
            case TaskResult.Error(_message):
                return Result.Ok(None)
            case TaskResult.Cancelled:
                return Result.Ok(None)
            case TaskResult.TimedOut:
                return Result.Ok(None)

        http_listener = try net.http_listen("127.0.0.1:0")
        http_addr = try http_listener.local_addr()
        http_task = group.start(serve_http, http_listener)
        headers: Map[String, String] = {{"X-Test": "ok"}}
        response = try net.http_request_text("POST", "http://" + http_addr + "/hello", "body", headers.clone())
        with http_response = response:
            print(http_response.status())
            print(try http_response.text())
        match http_task.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                return Result.Ok(None)
            case TaskResult.Cancelled:
                return Result.Ok(None)
            case TaskResult.TimedOut:
                return Result.Ok(None)

        http_bytes_listener = try net.http_listen("127.0.0.1:0")
        http_bytes_addr = try http_bytes_listener.local_addr()
        http_bytes_task = group.start(serve_http_bytes, http_bytes_listener)
        bytes_response = try net.http_request_bytes("POST", "http://" + http_bytes_addr + "/bytes", [1 as uint8, 2 as uint8], headers)
        with received_bytes = bytes_response:
            print(received_bytes.status())
            print(received_bytes.bytes().len())
        match http_bytes_task.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                return Result.Ok(None)
            case TaskResult.Cancelled:
                return Result.Ok(None)
            case TaskResult.TimedOut:
                return Result.Ok(None)

        ws_listener = try net.websocket_listen("127.0.0.1:0")
        ws_addr = try ws_listener.local_addr()
        ws_task = group.start(serve_ws, ws_listener)
        client = try net.websocket_connect_timeout("ws://" + ws_addr + "/", 1s)
        with ws_client = client:
            try ws_client.send_text("hi", timeout=1s)
            match try ws_client.recv_text(timeout=1s):
                case Option.Some(text):
                    print(text)
                case Option.None:
                    return Result.Ok(None)
        match ws_task.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                return Result.Ok(None)
            case TaskResult.Cancelled:
                return Result.Ok(None)
            case TaskResult.TimedOut:
                return Result.Ok(None)

    return Result.Ok(None)

def main() -> int32:
    match run():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#,
        path = file_path.display()
    );

    let (_build, run) = build_and_run_direct_source("aurora-cli-direct-advanced-io-net", &source);
    assert!(
        run.status.success(),
        "direct backend advanced io/network binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "4\n65\n67\nudp:ping\nping\n200\nPOST:/hello:body:ok\n202\n2\nws:hi\n"
    );
}

#[test]
fn run_and_direct_backend_match_unannotated_get_or_none_and_result_or_none() {
    let source = r#"
def worker() -> int32:
    return 7

def main() -> int32:
    jobs = Queue[int32]()
    jobs.put(5)
    queue_opt = jobs.get_or_none()
    match queue_opt:
        case Some(value):
            print(value)
        case None:
            print(-1)

    with TaskGroup() as group:
        task = group.start(worker)
        task_opt = task.result_or_none(timeout=50ms)
        match task_opt:
            case Some(value):
                print(value)
            case None:
                print(-2)
    return 0
"#;

    assert_run_and_direct_source_stdout(
        "aurora-unannotated-option-match-lowering",
        source,
        "5\n7\n",
    );
}

#[test]
fn run_and_direct_backend_match_bare_none_in_indirect_option_field() {
    let source = r#"
class Node:
    value: int32
    next: indirect Node?

def main() -> int32:
    tail = Node(value=2, next=None)
    match tail.next:
        case Some(next):
            print(next.value)
        case None:
            print(-1)
    return 0
"#;

    assert_run_and_direct_source_stdout("aurora-indirect-option-none-match", source, "-1\n");
}

#[test]
fn run_and_direct_backend_allow_match_expression_value_scrutinee_first_use() {
    let source = r#"
class Box:
    value: int32

def take(b: Box) -> int32:
    return b.value

def main() -> int32:
    b = Box(value=5)
    n = match take(b):
        case 1:
            10
        case _:
            20
    print(n)
    return 0
"#;

    assert_run_and_direct_source_stdout("aurora-match-expr-value-scrutinee", source, "20\n");
}

#[test]
fn run_preserves_buffered_stdout_on_runtime_error() {
    let source = r#"
def main() -> int32:
    print("first")
    print("second")
    values = [1, 2]
    print(values[99])
    return 0
"#;
    let (_temp, source_path) = write_temp_source("aurora-run-buffered-stdout-error", source);

    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run on buffered stdout error source");

    assert!(
        !output.status.success(),
        "run should fail for the runtime error source"
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "first\nsecond\n");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("out of bounds"),
        "runtime error should mention the out-of-bounds access, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn with_task_group_joins_start_soon_before_scope_exit() {
    let source = r#"
def producer(jobs: Queue[int32]) -> None:
    sleep(20ms)
    jobs.put(9)

def main() -> int32:
    jobs = Queue[int32]()
    with TaskGroup() as group:
        group.start_soon(producer, jobs)
        print("scope")
    print(jobs.get_or(-1))
    return 0
"#;

    assert_run_and_direct_source_stdout("aurora-task-group-start-soon-join", source, "scope\n9\n");
}

#[test]
fn task_results_surface_errors_without_aborting_the_program() {
    let source = r#"
def bad() -> int32:
    values = [1, 2]
    return values[7]

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(bad)
        match task.result(timeout=100ms):
            case TaskResult.Ready(value):
                print(value)
            case TaskResult.Error(message):
                print(message.contains("out of bounds"))
            case TaskResult.TimedOut:
                print(false)
            case TaskResult.Cancelled:
                print(false)

        print(task.result_or(-1))

        maybe = task.result_or_none(timeout=100ms)
        match maybe:
            case Some(value):
                print(value)
            case None:
                print(-1)
    print("after")
    return 0
"#;

    assert_run_and_direct_source_stdout(
        "aurora-task-result-error-surface",
        source,
        "true\n-1\n-1\nafter\n",
    );
}

#[test]
fn unread_task_failures_abort_task_group_scope() {
    let source = r#"
def boom() -> int32:
    values = [1, 2]
    return values[7]

def main() -> int32:
    print("before")
    with TaskGroup() as group:
        group.start(boom)
    print("after")
    return 0
"#;
    let (temp, source_path) = write_temp_source("aurora-task-group-unread-failure", source);

    let run = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run on unread task failure source");
    assert!(
        !run.status.success(),
        "run should fail when a task group scope exits with an unread task failure"
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("out of bounds"),
        "run stderr should surface the unread task failure, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let output_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to build unread task failure source");
    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let direct = generated_binary(&output_path)
        .output()
        .expect("failed to run direct unread task failure binary");
    assert!(
        !direct.status.success(),
        "direct binary should fail when a task group scope exits with an unread task failure"
    );
    assert_eq!(String::from_utf8_lossy(&direct.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&direct.stderr).contains("out of bounds"),
        "direct stderr should surface the unread task failure, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
}

#[test]
fn cancelled_yields_for_cpu_bound_lightweight_tasks() {
    let source = r#"
def worker() -> int32:
    mut n = 0
    while n < 1000000:
        if cancelled():
            return 9999
        n += 1
    return n

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(worker)
        sleep(1ms)
        group.cancel()
        match task.result(timeout=10s):
            case TaskResult.Ready(value):
                print(value)
            case TaskResult.Error(_message):
                print(-1)
            case TaskResult.TimedOut:
                print(-2)
            case TaskResult.Cancelled:
                print(-3)
    return 0
"#;

    assert_run_and_direct_source_stdout("aurora-cancelled-yields", source, "9999\n");
}

#[test]
fn self_receiver_method_result_can_bind_to_a_name() {
    let source = r#"
class Box:
    value: int32

    def take(self) -> int32:
        return self.value

def main() -> int32:
    b = Box(value=7)
    x = b.take()
    print(x)
    return 0
"#;

    let (_temp, source_path) = write_temp_source("aurora-value-receiver-binding", source);
    let check = Command::new(aura_bin())
        .arg("check")
        .arg(&source_path)
        .output()
        .expect("failed to run aura check on value receiver binding source");

    assert!(
        check.status.success(),
        "check should accept binding a value-receiver result, stderr was:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    assert_run_and_direct_source_stdout("aurora-value-receiver-binding", source, "7\n");
}

#[test]
fn vec_insert_out_of_bounds_is_a_runtime_error() {
    let source = r#"
def main() -> int32:
    mut values = [1, 2, 3]
    print("before")
    print(values.insert(index=99, value=7))
    print("after")
    return 0
"#;
    let (temp, source_path) = write_temp_source("aurora-vec-insert-oob", source);

    let run = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run on vec insert source");
    assert!(!run.status.success(), "run should fail for vec insert OOB");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("out of bounds"),
        "run stderr should mention the out-of-bounds insert, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let output_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to build direct vec insert source");
    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let direct = generated_binary(&output_path)
        .output()
        .expect("failed to run direct vec insert binary");
    assert!(
        !direct.status.success(),
        "direct binary should fail for vec insert OOB"
    );
    assert_eq!(String::from_utf8_lossy(&direct.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&direct.stderr).contains("out of bounds"),
        "direct stderr should mention the out-of-bounds insert, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
}

#[test]
fn vec_set_out_of_bounds_is_a_runtime_error() {
    let source = r#"
def main() -> int32:
    mut values = [1, 2, 3]
    print("before")
    print(values.set(index=99, value=7))
    print("after")
    return 0
"#;
    let (temp, source_path) = write_temp_source("aurora-vec-set-oob", source);

    let run = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run on vec set source");
    assert!(!run.status.success(), "run should fail for vec set OOB");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("out of bounds"),
        "run stderr should mention the out-of-bounds set, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let output_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to build direct vec set source");
    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let direct = generated_binary(&output_path)
        .output()
        .expect("failed to run direct vec set binary");
    assert!(
        !direct.status.success(),
        "direct binary should fail for vec set OOB"
    );
    assert_eq!(String::from_utf8_lossy(&direct.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&direct.stderr).contains("out of bounds"),
        "direct stderr should mention the out-of-bounds set, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
}

#[test]
fn vec_remove_out_of_bounds_is_a_runtime_error() {
    let source = r#"
def main() -> int32:
    mut values = [1, 2, 3]
    print("before")
    print(values.remove(index=99))
    print("after")
    return 0
"#;
    let (temp, source_path) = write_temp_source("aurora-vec-remove-oob", source);

    let run = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run on vec remove source");
    assert!(!run.status.success(), "run should fail for vec remove OOB");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("out of bounds"),
        "run stderr should mention the out-of-bounds remove, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let output_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to build direct vec remove source");
    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let direct = generated_binary(&output_path)
        .output()
        .expect("failed to run direct vec remove binary");
    assert!(
        !direct.status.success(),
        "direct binary should fail for vec remove OOB"
    );
    assert_eq!(String::from_utf8_lossy(&direct.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&direct.stderr).contains("out of bounds"),
        "direct stderr should mention the out-of-bounds remove, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
}

#[test]
fn vec_swap_out_of_bounds_is_a_runtime_error() {
    let source = r#"
def main() -> int32:
    mut values = [1, 2, 3]
    print("before")
    print(values.swap(first=0, second=99))
    print("after")
    return 0
"#;
    let (temp, source_path) = write_temp_source("aurora-vec-swap-oob", source);

    let run = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run on vec swap source");
    assert!(!run.status.success(), "run should fail for vec swap OOB");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&run.stderr)
            .contains("vector swap indices `0` and `99` are out of bounds for length `3`"),
        "run stderr should mention both out-of-bounds swap indices, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let output_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to build direct vec swap source");
    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let direct = generated_binary(&output_path)
        .output()
        .expect("failed to run direct vec swap binary");
    assert!(
        !direct.status.success(),
        "direct binary should fail for vec swap OOB"
    );
    assert_eq!(String::from_utf8_lossy(&direct.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&direct.stderr)
            .contains("vector swap indices `0` and `99` are out of bounds for length `3`"),
        "direct stderr should mention both out-of-bounds swap indices, stderr was:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
}

#[test]
fn queue_iteration_exits_when_task_group_is_cancelled() {
    let source = r#"
def worker(q: Queue[int32]):
    sleep(10s)

def main() -> int32:
    q: Queue[int32] = Queue[int32]()
    with TaskGroup() as g:
        g.start_soon(worker, q)
        sleep(50ms)
        print("about to cancel")
        g.cancel()
        print("about to iterate")
        for v in q:
            print(v)
        print("loop done")
    print("scope done")
    return 0
"#;

    assert_run_and_direct_source_stdout_with_timeout(
        "aurora-queue-iteration-cancel",
        source,
        std::time::Duration::from_secs(15),
        "about to cancel\nabout to iterate\nloop done\nscope done\n",
    );
}

#[test]
fn queue_iteration_exits_when_a_sibling_task_fails() {
    let source = r#"
def producer(q: Queue[int32]):
    q.put(1)
    values = [1]
    _ = values[99]

def main() -> int32:
    print("before")
    q: Queue[int32] = Queue[int32]()
    with TaskGroup() as g:
        g.start(producer, q)
        for v in q:
            pass
    print("after")
    return 0
"#;

    assert_run_and_direct_source_failure_with_timeout(
        "aurora-queue-iteration-sibling-failure",
        source,
        std::time::Duration::from_secs(15),
        "before\n",
        "out of bounds",
    );
}

#[test]
fn queue_iteration_exits_when_task_group_producers_return_cleanly() {
    let source = r#"
def producer(q: Queue[int32]):
    q.put(1)
    q.put(2)

def main() -> int32:
    q: Queue[int32] = Queue[int32]()
    with TaskGroup() as g:
        g.start_soon(producer, q)
        for v in q:
            print(v)
        print("loop done")
    print("scope done")
    return 0
"#;

    assert_run_and_direct_source_stdout_with_timeout(
        "aurora-queue-iteration-clean-return",
        source,
        std::time::Duration::from_secs(15),
        "1\n2\nloop done\nscope done\n",
    );
}

#[test]
fn direct_backend_unwinds_with_resources_before_runtime_trap() {
    let source = r#"
class Resource:
    name: String

    def close(borrow mut self):
        print("close " + self.name)

def main() -> int32:
    with a = Resource(name="A"):
        with b = Resource(name="B"):
            values: Vec[int32] = []
            print(values[5])
    return 0
"#;

    let (_, run) = build_and_run_direct_source("aurora-direct-with-trap-cleanup", source);
    assert!(
        !run.status.success(),
        "direct binary should fail on vector OOB"
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "close B\nclose A\n");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("vector index `5` is out of bounds"),
        "stderr should include vector OOB diagnostic, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn direct_backend_unwinds_with_resources_when_callee_traps() {
    let source = r#"
class Resource:
    name: String

    def close(borrow mut self):
        print("close " + self.name)

def boom() -> int32:
    values: Vec[int32] = []
    return values[5]

def main() -> int32:
    with a = Resource(name="A"):
        with b = Resource(name="B"):
            with c = Resource(name="C"):
                return boom()
    return 0
"#;

    let (_, run) = build_and_run_direct_source("aurora-direct-with-callee-trap-cleanup", source);
    assert!(
        !run.status.success(),
        "direct binary should fail on vector OOB"
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "close C\nclose B\nclose A\n"
    );
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("vector index `5` is out of bounds"),
        "stderr should include vector OOB diagnostic, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn direct_backend_callee_trap_cleanup_uses_current_resource_state() {
    let source = r#"
class Resource:
    name: String

    def close(borrow mut self):
        print("close " + self.name)

def boom() -> int32:
    values: Vec[int32] = []
    return values[5]

def main() -> int32:
    with resource = Resource(name="old"):
        resource.name = "new"
        return boom()
    return 0
"#;

    let (_, run) = build_and_run_direct_source("aurora-direct-with-current-cleanup", source);
    assert!(
        !run.status.success(),
        "direct binary should fail on vector OOB"
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "close new\n");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("vector index `5` is out of bounds"),
        "stderr should include vector OOB diagnostic, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn direct_backend_preserves_body_trap_when_cleanup_also_traps() {
    let source = r#"
class Resource:
    name: String

    def close(borrow mut self):
        print("close " + self.name)
        print(1 / 0)

def boom() -> int32:
    print("body")
    return 1 / 0

def main() -> int32:
    with resource = Resource(name="A"):
        return boom()
    return 0
"#;

    let (_, run) = build_and_run_direct_source("aurora-direct-primary-trap-diagnostic", source);
    assert!(
        !run.status.success(),
        "direct binary should fail when the body traps"
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "body\nclose A\n");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("return 1 / 0"),
        "direct backend should report the primary body trap, stderr was:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("print(1 / 0)"),
        "cleanup trap should not replace the primary body trap, stderr was:\n{}",
        stderr
    );
}

#[test]
fn direct_backend_recursion_limit_uses_source_diagnostic() {
    let source = r#"
def recurse(value: int32) -> int32:
    return recurse(value + 1)

def main() -> int32:
    return recurse(0)
"#;

    let (_, run) = build_and_run_direct_source("aurora-direct-recursion-diagnostic", source);
    assert!(
        !run.status.success(),
        "direct binary should fail on recursion limit"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("maximum call depth") && stderr.contains("while calling `recurse`"),
        "stderr should describe the Aurora recursion limit, stderr was:\n{}",
        stderr
    );
    assert!(
        stderr.contains("-->") && !stderr.contains("direct backend"),
        "stderr should render with source context and avoid backend-specific wording, stderr was:\n{}",
        stderr
    );
}

#[test]
fn direct_backend_recursion_with_with_frames_matches_run_cleanup_count() {
    let source = r#"
class Resource:
    def close(borrow mut self):
        print("CLOSE_REC")

def recurse(value: int32) -> int32:
    with resource = Resource():
        return recurse(value + 1)

def main() -> int32:
    return recurse(0)
"#;

    let (temp, source_path) = write_temp_source("aurora-recursion-with-cleanup-count", source);
    let run = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run");
    assert!(!run.status.success(), "aura run should fail on recursion");

    let run_stdout = String::from_utf8_lossy(&run.stdout);
    let run_close_count = run_stdout
        .lines()
        .filter(|line| *line == "CLOSE_REC")
        .count();
    assert_eq!(
        run_close_count, 254,
        "aura run should preserve the established cleanup count"
    );

    let output_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build --backend direct");
    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let direct = generated_binary(&output_path)
        .output()
        .expect("failed to run direct binary");
    assert!(
        !direct.status.success(),
        "direct binary should fail on recursion"
    );
    let direct_stdout = String::from_utf8_lossy(&direct.stdout);
    let direct_close_count = direct_stdout
        .lines()
        .filter(|line| *line == "CLOSE_REC")
        .count();
    assert_eq!(
        direct_close_count, run_close_count,
        "direct backend should unwind the same number of with frames as aura run"
    );
}

#[test]
fn direct_backend_unwinds_with_resources_before_recursion_limit() {
    let source = r#"
class Resource:
    name: String

    def close(borrow mut self):
        print("close " + self.name)

def recurse(value: int32) -> int32:
    return recurse(value + 1)

def main() -> int32:
    with resource = Resource(name="A"):
        return recurse(0)
    return 0
"#;

    let (_, run) = build_and_run_direct_source("aurora-direct-recursion-cleanup", source);
    assert!(
        !run.status.success(),
        "direct binary should fail on recursion limit"
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "close A\n");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("maximum call depth"),
        "stderr should describe the Aurora recursion limit, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn run_flushes_stdout_before_sigkill() {
    let source = r#"
def main() -> int32:
    print("before")
    while true:
        sleep(1s)
    return 0
"#;

    let (temp, source_path) = write_temp_source("aurora-run-sigkill-flush", source);
    let mut child = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura run");

    std::thread::sleep(std::time::Duration::from_millis(300));
    child.kill().expect("failed to kill hung aura run");
    let output = child
        .wait_with_output()
        .expect("failed to collect killed aura run output");
    drop(temp);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("before\n"),
        "aura run should flush stdout as prints happen, stdout was:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn process_completed_exposes_binary_stdout_bytes_in_run_and_direct_backend() {
    let source = r#"import process

def run_binary_stdout() -> Result[None, process.Error]:
    completed = try process.run(["/usr/bin/env", "python3", "-c", "import sys; sys.stdout.buffer.write(bytes([255, 0, 65]))"], stdout=process.pipe(), stderr=process.pipe(), timeout=2s, group=true)
    bytes = completed.stdout_bytes()
    print(bytes.len())
    print(bytes[0])
    print(bytes[1])
    print(bytes[2])
    return Result.Ok(None)

def main() -> int32:
    match run_binary_stdout():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#;

    assert_run_and_direct_source_stdout(
        "aurora-process-completed-stdout-bytes",
        source,
        "3\n255\n0\n65\n",
    );
}

#[test]
fn process_completed_stdout_bytes_get_matches_short_option_patterns() {
    let source = r#"import process

def inspect_first_byte() -> Result[None, process.Error]:
    completed = try process.run(["/bin/echo", "hi"], stdout=process.pipe(), stderr=process.pipe(), timeout=2s, group=true)
    opt = completed.stdout_bytes().get(0)
    match opt:
        case Some(byte):
            print("some")
            print(byte)
        case None:
            print("none")
    return Result.Ok(None)

def main() -> int32:
    match inspect_first_byte():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#;

    assert_run_and_direct_source_stdout(
        "aurora-process-stdout-bytes-short-option-match",
        source,
        "some\n104\n",
    );
}

#[test]
fn queue_iteration_without_registered_producers_exits() {
    let source = r#"
def main() -> int32:
    jobs: Queue[int32] = Queue[int32]()
    for job in jobs:
        print(job)
    print("done")
    return 0
"#;

    assert_run_and_direct_source_stdout_with_timeout(
        "aurora-queue-iteration-zero-producers",
        source,
        std::time::Duration::from_secs(5),
        "done\n",
    );
}

#[test]
fn queue_iteration_waits_for_standalone_task_group_producers() {
    let source = r#"
def producer(jobs: Queue[int32]) -> None:
    sleep(1ms)
    jobs.put(7)
    jobs.close()

def main() -> int32:
    jobs: Queue[int32] = Queue[int32]()
    group = TaskGroup()
    group.start(producer, jobs)
    for job in jobs:
        print(job)
    print("done")
    return 0
"#;

    assert_run_and_direct_source_stdout_with_timeout(
        "aurora-queue-iteration-standalone-task-group",
        source,
        std::time::Duration::from_secs(5),
        "7\ndone\n",
    );
}

#[test]
fn wait_any_without_tasks_times_out_immediately() {
    let source = r#"def main() -> int32:
    tasks = Vec[Task[int32]]()
    match wait_any(tasks):
        case WaitAny.Ready(index, value):
            print(index)
            print(value)
        case WaitAny.Error(index, message):
            print(index)
            print(message)
        case WaitAny.TimedOut:
            print("timedout")
        case WaitAny.Cancelled:
            print("cancelled")
    return 0
"#;

    assert_run_and_direct_source_stdout_with_timeout(
        "aurora-wait-any-empty",
        source,
        std::time::Duration::from_secs(5),
        "timedout\n",
    );
}

#[test]
fn queue_get_or_without_timeout_returns_default_immediately() {
    let source = r#"def main() -> int32:
    jobs = Queue[int32]()
    print("before")
    print(jobs.get_or(7))
    print("after")
    return 0
"#;

    assert_run_and_direct_source_stdout_with_timeout(
        "aurora-queue-get-or-no-timeout",
        source,
        std::time::Duration::from_secs(15),
        "before\n7\nafter\n",
    );
}

#[test]
fn task_result_or_without_timeout_returns_fallback_immediately() {
    let source = r#"def slow() -> int32:
    sleep(100ms)
    return 5

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(slow)
        print(task.result_or(-1))
        match task.result_or_none():
            case Some(value):
                print(value)
            case None:
                print(-2)
    return 0
"#;

    assert_run_and_direct_source_stdout_with_timeout(
        "aurora-task-result-or-no-timeout",
        source,
        std::time::Duration::from_secs(15),
        "-1\n-2\n",
    );
}

#[test]
fn fs_write_bytes_accepts_empty_lists_in_run_and_direct_backend() {
    let source = r#"import fs

def main() -> int32:
    match fs.write_bytes("/tmp/aurora-empty-bytes.bin", []):
        case Result.Ok(_):
            print("ok")
        case Result.Err(error):
            print(error)
    return 0
"#;

    assert_run_and_direct_source_stdout("aurora-fs-write-empty-bytes", source, "ok\n");
}

#[test]
fn direct_backend_build_supports_process_module_surface() {
    let temp = TempDir::new("aurora-cli-direct-process");
    let cwd = fs::canonicalize(temp.path())
        .expect("temp path should canonicalize")
        .display()
        .to_string();
    let source = format!(
        r#"import process

def run(cwd: String) -> Result[None, process.Error]:
    env: Map[String, String] = {{"AURORA_PROCESS_VAR": "present"}}
    completed = try process.run(["/usr/bin/printenv", "AURORA_PROCESS_VAR"], env=env, timeout=2s, group=true)
    print(completed.stdout().trim())
    print(completed.stderr().len())
    pwd = try process.run(["/bin/pwd"], cwd=Option.Some(cwd), timeout=2s, group=true)
    print(pwd.stdout().trim())
    print(pwd.stderr().len())
    print(completed.status())
    print(pwd.status())

    with child = try process.start(["/bin/cat"], stdin=process.pipe(), stdout=process.pipe(), stderr=process.null(), group=true):
        match child.stdin():
            case Option.Some(found_pipe):
                stdin_pipe: process.Pipe = found_pipe
                try stdin_pipe.write_all("echo from cat\n", timeout=500ms)
                try stdin_pipe.flush()
                stdin_pipe.close()
            case Option.None:
                return Result.Ok(None)

        match child.stdout():
            case Option.Some(found_pipe):
                stdout_pipe: process.Pipe = found_pipe
                match try stdout_pipe.read_line(timeout=500ms):
                    case Option.Some(text):
                        print(text)
                    case Option.None:
                        return Result.Ok(None)
            case Option.None:
                return Result.Ok(None)

        print(child.wait(timeout=2s))
    with supervisor = process.supervisor():
        try supervisor.start(name="flaky", command=["/usr/bin/false"], restart=process.RestartPolicy.OnFailure, backoff=10ms, max_restarts=1, group=true)
        print(try supervisor.wait_or_none(timeout=500ms))
        print(try supervisor.wait_or_none(timeout=500ms))
        print(supervisor.is_empty())
        try supervisor.start(name="sleeper", command=["/bin/sleep", "1"], restart=process.RestartPolicy.Never, group=true)
        print(supervisor.is_empty())
        try supervisor.stop()
        print(supervisor.is_empty())
    return Result.Ok(None)

def main() -> int32:
    match run("{cwd}"):
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#,
        cwd = cwd,
    );

    let (_build, run) = build_and_run_direct_source("aurora-cli-direct-process", &source);
    assert!(
        run.status.success(),
        "direct backend process binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        format!(
            "present\n0\n{cwd}\n0\nExitStatus.Exited(0)\nExitStatus.Exited(0)\necho from cat\nWait.Exited(ExitStatus.Exited(0))\nOption.Some(SupervisorEvent.Restarted(flaky, ExitStatus.Exited(1), 1))\nOption.Some(SupervisorEvent.Exited(flaky, ExitStatus.Exited(1), 1))\ntrue\nfalse\ntrue\n",
            cwd = cwd,
        )
    );
}

#[cfg(unix)]
#[test]
fn direct_backend_build_supports_unix_and_tls_network_surface() {
    let temp = TempDir::new("aurora-cli-direct-unix-tls");
    let unix_path = PathBuf::from(format!(
        "/tmp/aurora-cli-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos()
    ));

    let certificate = generate_simple_self_signed(vec!["localhost".to_string()])
        .expect("should generate self-signed certificate");
    let cert_pem = certificate.cert.pem();
    let key_pem = certificate.key_pair.serialize_pem();
    let cert_path = temp.path().join("cert.pem");
    let key_path = temp.path().join("key.pem");
    fs::write(&cert_path, cert_pem).expect("should write cert pem");
    fs::write(&key_path, key_pem).expect("should write key pem");

    let source = format!(
        r#"import io
import net

def serve_unix(listener: net.UnixListener) -> Result[None, io.Error]:
    with server_listener = listener:
        stream = try server_listener.accept(timeout=1s)
        with server_stream = stream:
            match try server_stream.read_line(timeout=1s):
                case Option.Some(text):
                    try server_stream.write_all("unix:" + text, timeout=1s)
                    return Result.Ok(None)
                case Option.None:
                    return Result.Ok(None)

def serve_tls(listener: net.TlsListener) -> Result[None, io.Error]:
    with server_listener = listener:
        stream = try server_listener.accept(timeout=2s)
        with server_stream = stream:
            match try server_stream.read_line(timeout=2s):
                case Option.Some(text):
                    try server_stream.write_all("tls:" + text + "\n", timeout=2s)
                    return Result.Ok(None)
                case Option.None:
                    return Result.Ok(None)

def run() -> Result[None, io.Error]:
    with TaskGroup() as group:
        unix_listener = try net.unix_listen("{unix_path}")
        unix_task = group.start(serve_unix, unix_listener)
        client = try net.unix_connect_timeout("{unix_path}", 1s)
        with unix_client = client:
            try unix_client.write_all("ping\n", timeout=1s)
            match try unix_client.read_line(timeout=1s):
                case Option.Some(text):
                    print(text)
                case Option.None:
                    return Result.Ok(None)
        match unix_task.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                return Result.Ok(None)
            case TaskResult.Cancelled:
                return Result.Ok(None)
            case TaskResult.TimedOut:
                return Result.Ok(None)

        tls_listener = try net.tls_listen("127.0.0.1:0", "{cert_path}", "{key_path}")
        tls_addr = try tls_listener.local_addr()
        tls_task = group.start(serve_tls, tls_listener)
        stream = try net.tls_connect_timeout(tls_addr, "localhost", "{cert_path}", 2s)
        with tls_client = stream:
            try tls_client.write_all("ping!\n", timeout=2s)
            match try tls_client.read_line(timeout=2s):
                case Option.Some(text):
                    print(text)
                case Option.None:
                    return Result.Ok(None)
        match tls_task.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                return Result.Ok(None)
            case TaskResult.Cancelled:
                return Result.Ok(None)
            case TaskResult.TimedOut:
                return Result.Ok(None)

    return Result.Ok(None)

def main() -> int32:
    match run():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#,
        unix_path = unix_path.display(),
        cert_path = cert_path.display(),
        key_path = key_path.display()
    );

    let (_build, run) = build_and_run_direct_source("aurora-cli-direct-unix-tls", &source);
    let _ = fs::remove_file(&unix_path);
    assert!(
        run.status.success(),
        "direct backend unix/tls binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "unix:ping\ntls:ping!\n"
    );
}

#[test]
fn zero_sized_udp_reads_return_typed_invalid_input_in_mir_and_direct_backends() {
    let source = r#"import io
import net

def probe() -> Result[None, io.Error]:
    with socket = try net.udp_bind("127.0.0.1:0"):
        print(socket.recv(0, timeout=1ms))
        print(socket.recv_from(0, timeout=1ms))
    return Result.Ok(None)

def main() -> int32:
    match probe():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#;

    assert_run_and_direct_source_stdout(
        "aurora-zero-sized-udp-read",
        source,
        "Result.Err(io.Error.InvalidInput)\nResult.Err(io.Error.InvalidInput)\n",
    );
}

#[test]
fn direct_backend_metrics_int64_overflow_fails_at_runtime() {
    let source = r#"import metrics

def main() -> int32:
    metrics.reset()
    metrics.increment("requests", 9223372036854775807)
    metrics.increment("requests", 1)
    print(metrics.get("requests"))
    return 0
"#;

    let (_build, run) = build_and_run_direct_source("aurora-direct-metrics-overflow", source);
    assert!(!run.status.success(), "metrics overflow should fail");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("metric value overflowed `int64`"),
        "unexpected direct-backend metrics diagnostic: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        run.stdout.is_empty(),
        "overflow should stop before metrics.get"
    );
}
