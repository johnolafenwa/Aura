use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use rcgen::generate_simple_self_signed;

const FILESYSTEM_READ_CAP_BYTES: usize = 256 * 1024 * 1024;
const RETIRED_FILESYSTEM_READ_CAP_BYTES: usize = 64 * 1024 * 1024;

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

#[cfg(unix)]
fn command_output_with_timeout(
    mut command: Command,
    timeout: std::time::Duration,
    context: &str,
) -> std::process::Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("{context}: failed to spawn command: {error}"));
    let status = match wait_with_timeout(&mut child, timeout) {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(mut pipe) = child.stdout.take() {
                let _ = pipe.read_to_end(&mut stdout);
            }
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_end(&mut stderr);
            }
            panic!(
                "{context}: command did not finish within {timeout:?}; stdout was:\n{}\nstderr was:\n{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            );
        }
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .expect("captured stdout should exist")
        .read_to_end(&mut stdout)
        .expect("captured stdout should be readable");
    child
        .stderr
        .take()
        .expect("captured stderr should exist")
        .read_to_end(&mut stderr)
        .expect("captured stderr should be readable");
    std::process::Output {
        status,
        stdout,
        stderr,
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

#[test]
fn ast_json_preserves_legacy_named_and_loop_shapes_while_exposing_tuples() {
    let source = [
        "def named(items: Vec[int32]) -> int32:",
        "    for item in items:",
        "        pass",
        "    return 0",
        "",
        "def tupled(items: Vec[(int32, String)]) -> (int32, String):",
        "    for (number, text) in items:",
        "        return (number, text)",
        "    return (0, \"\")",
    ]
    .join("\n");

    let mut child = Command::new(aura_bin())
        .arg("ast-json")
        .arg("--stdin")
        .arg("/virtual/tuple_ast.au")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura ast-json");
    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write tuple AST source");
    let output = child
        .wait_with_output()
        .expect("failed to collect aura ast-json output");
    assert!(
        output.status.success(),
        "ast-json should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("ast-json should return valid JSON");
    let named = &json["items"][0]["Function"];
    let named_type = &named["params"][0]["ty"];
    assert_eq!(named_type["name"], "Vec");
    assert_eq!(named_type["args"][0]["name"], "int32");
    assert!(
        named_type.get("kind").is_none(),
        "named type references must retain the pre-tuple JSON shape"
    );
    let simple_loop = &named["body"][0]["For"];
    assert_eq!(simple_loop["binding"], "item");
    assert!(
        simple_loop.get("target").is_none(),
        "simple loops must retain the pre-tuple `binding` field"
    );

    let tupled = &json["items"][1]["Function"];
    let tuple_parameter = &tupled["params"][0]["ty"]["args"][0];
    assert_eq!(tuple_parameter["elements"][0]["name"], "int32");
    assert_eq!(tuple_parameter["elements"][1]["name"], "String");
    assert_eq!(
        tupled["return_type"]["elements"].as_array().map(Vec::len),
        Some(2)
    );
    let tuple_loop = &tupled["body"][0]["For"];
    assert!(tuple_loop.get("binding").is_none());
    assert_eq!(
        tuple_loop["target"]["Tuple"]["elements"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
}

#[test]
fn generic_tuple_substitution_runs_in_mir_and_direct_backends() {
    let source = [
        "def swap[T, U](pair: own (T, U)) -> (U, T):",
        "    left, right = pair",
        "    return (right, left)",
        "",
        "def main() -> int32:",
        "    result = swap((7, \"seven\"))",
        "    label, number = result",
        "    print(label)",
        "    print(number)",
        "    return 0",
    ]
    .join("\n");

    assert_run_and_direct_source_stdout("aurora-cli-generic-tuples", &source, "seven\n7\n");
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
            "semantic_interface_version": aurora_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION,
            "method": "analyze",
            "path": "/virtual/main.au",
            "source": "def main() -> int32:\n    return 0\n"
        }),
        serde_json::json!({
            "id": 2,
            "semantic_interface_version": aurora_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION,
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
    assert_eq!(
        responses[0]["semantic_interface_version"],
        aurora_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION
    );
    assert!(responses[0]["result"]["diagnostics"].is_array());
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(
        responses[1]["semantic_interface_version"],
        aurora_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION
    );
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
fn fmt_is_idempotent_for_adr_0022_capability_syntax() {
    let temp = TempDir::new("aurora-capability-format");
    let source_path = temp.path().join("capabilities.au");
    fs::write(
        &source_path,
        concat!(
            "class Box:\r\n",
            "    value: String   \r\n",
            "    def read(self) -> String:\r\n",
            "        return self.value.clone()\r\n",
            "    def replace(mut self, value: own String):\r\n",
            "        self.value = value\r\n",
            "\r\n",
            "def inspect(value: String):\r\n",
            "    print(value)\r\n",
            "\r\n",
            "def main():\r\n",
            "    mut boxes = [Box(value=\"one\")]\r\n",
            "    for box in mut boxes:\r\n",
            "        box.replace(\"changed\")\r\n",
            "    match own boxes:\r\n",
            "        case _:\r\n",
            "            pass\t\r\n",
        ),
    )
    .expect("capability source should write");

    let first = Command::new(aura_bin())
        .args(["fmt"])
        .arg(&source_path)
        .output()
        .expect("failed to format ADR-0022 capability syntax");
    assert!(
        first.status.success(),
        "capability syntax should format successfully, stderr was:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let once = fs::read_to_string(&source_path).expect("formatted source should read");
    assert!(once.contains("def replace(mut self, value: own String):"));
    assert!(once.contains("for box in mut boxes:"));
    assert!(once.contains("match own boxes:"));
    assert!(!once.contains('\r'));
    assert!(!once.lines().any(|line| line.ends_with([' ', '\t'])));

    let check = Command::new(aura_bin())
        .args(["fmt", "--check"])
        .arg(&source_path)
        .output()
        .expect("failed to check formatted ADR-0022 capability syntax");
    assert!(
        check.status.success(),
        "a second formatter pass must be idempotent, stderr was:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert_eq!(
        fs::read_to_string(&source_path).expect("idempotent source should read"),
        once
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
    fs::write(&body_path, "x".repeat(2_000_000)).expect("failed to write HTTP response body");
    let source = format!(
        r#"import fs
import io
import net

def serve(listener: own net.HttpListener, path: String) -> Result[None, io.Error]:
    server = listener
    req = try server.accept(timeout=5s)
    body = try fs.read_to_string(path)
    try req.respond_text(200, body, {{}})
    return Result.Ok(None)

def run() -> Result[None, io.Error]:
    with TaskGroup() as group:
        listener = try net.http_listen("127.0.0.1:0")
        address = try listener.local_addr()
        group.start_soon(serve, listener, "{body_path}")
        resp = try net.http_request_text_timeout("GET", "http://" + address + "/big", "x", {{}}, 5s)
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
    assert_run_and_direct_source_stdout(
        "aurora-http-raised-response-cap",
        &source,
        "200\n2000000\n",
    );
}

#[test]
fn http_declared_response_above_fixed_cap_is_typed_on_both_backends() {
    let source = r#"import io
import net

def serve(listener: own net.HttpListener) -> Result[None, io.Error]:
    request = try listener.accept(timeout=5s)
    try request.respond_text(200, "", {"Content-Length": "16777217"})
    return Result.Ok(None)

def run() -> Result[None, io.Error]:
    with TaskGroup() as group:
        listener = try net.http_listen("127.0.0.1:0")
        address = try listener.local_addr()
        group.start_soon(serve, listener)
        response = net.http_request_text_timeout("GET", "http://" + address + "/oversized", "", {}, 5s)
        match response:
            case Result.Err(io.Error.InvalidData):
                print("http-too-large")
            case Result.Err(error):
                print(error)
            case Result.Ok(_):
                print("unexpected-success")
    return Result.Ok(None)

def main() -> int32:
    match run():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#;

    assert_run_and_direct_source_stdout(
        "aurora-http-fixed-response-cap",
        source,
        "http-too-large\n",
    );
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
fn compile_commands_emit_the_shared_structured_diagnostic_schema() {
    let (temp, source_path) = write_temp_source(
        "aurora-structured-diagnostics",
        "def main():\n    print(missing)\n    print(also_missing)\n",
    );
    let output_path = temp.path().join("out");

    let commands = [
        vec![
            "check".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ],
        vec![
            "run".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ],
        vec![
            "build".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "-o".to_string(),
            output_path.display().to_string(),
        ],
    ];

    for mut arguments in commands {
        let command_name = arguments[0].clone();
        arguments.push(source_path.display().to_string());
        let output = Command::new(aura_bin())
            .args(&arguments)
            .output()
            .unwrap_or_else(|error| panic!("failed to run aura {command_name}: {error}"));
        assert!(
            !output.status.success(),
            "{command_name} should reject the source"
        );
        let report: serde_json::Value =
            serde_json::from_slice(&output.stderr).unwrap_or_else(|error| {
                panic!(
                    "{command_name} should emit one JSON document: {error}; stderr was {}",
                    String::from_utf8_lossy(&output.stderr)
                )
            });
        assert_eq!(report["schema_version"], 1, "{command_name}");
        assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
        let diagnostic = &report["diagnostics"][0];
        assert_eq!(diagnostic["code"], "AU2001", "{command_name}");
        assert_eq!(diagnostic["severity"], "error", "{command_name}");
        assert_eq!(diagnostic["message"], "unknown name `missing`");
        assert!(diagnostic["primary_span"]["path"]
            .as_str()
            .unwrap()
            .ends_with("/main.au"));
        assert_eq!(diagnostic["primary_span"]["start"]["line"], 2);
        assert!(diagnostic["secondary_spans"].is_array());
        assert!(diagnostic["notes"].is_array());
        assert!(diagnostic["help"].is_array());
        assert!(diagnostic["edits"].is_array());
    }
}

#[cfg(unix)]
struct NativeCacheFixture {
    cache: TempDir,
    _install: TempDir,
    installed_aura: PathBuf,
    _source: TempDir,
    source_path: PathBuf,
    entry: PathBuf,
}

#[cfg(unix)]
impl NativeCacheFixture {
    fn new(prefix: &str) -> Self {
        let cache = TempDir::new(&format!("{prefix}-cache"));
        let (source, source_path) = write_temp_source(
            &format!("{prefix}-source"),
            "def main() -> int32:\n    print(\"cached\")\n    return 0\n",
        );

        let run = |cache_path: &std::path::Path| {
            Command::new(aura_bin())
                .env("AURORA_CACHE_DIR", cache_path)
                .arg("run")
                .arg("--backend")
                .arg("direct")
                .arg(&source_path)
                .output()
                .expect("failed to populate the native cache")
        };

        let cold = run(cache.path());
        assert!(
            cold.status.success(),
            "native-cache cold run failed, stderr was:\n{}",
            String::from_utf8_lossy(&cold.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&cold.stdout), "cached\n");

        // Timed cache-member checks must measure cache inspection, not
        // unrelated source-checkout tests contending on the shared Cargo
        // runtime lock. Copy a valid runtime plus its stable link arguments
        // into an installed immutable layout, which needs no workspace-runtime
        // lease. Populate the fixture's program entry through that installed
        // binary so concurrent Cargo activity cannot make the entry key refer
        // to different runtime bytes from the later timed checks.
        let install = TempDir::new(&format!("{prefix}-install"));
        let bin_dir = install.path().join("bin");
        let runtime_dir = install.path().join("lib").join("aurora");
        fs::create_dir_all(&bin_dir).expect("installed bin directory should be creatable");
        fs::create_dir_all(&runtime_dir).expect("installed runtime directory should be creatable");
        let installed_aura = bin_dir.join("aura");
        fs::copy(aura_bin(), &installed_aura).expect("aura executable should be installable");
        fs::copy(
            repo_root()
                .join("target")
                .join("debug")
                .join("libaurora_compiler.a"),
            runtime_dir.join("libaurora_compiler.a"),
        )
        .expect("native runtime archive should be installable");
        let runtime_memo = fs::read_to_string(cache.path().join("runtime-identity"))
            .expect("cold run should record native link arguments");
        let native_link_args = runtime_memo
            .lines()
            .nth(2)
            .expect("runtime memo should contain native link arguments");
        fs::write(
            runtime_dir.join("native-link-args.json"),
            format!("{native_link_args}\n"),
        )
        .expect("installed native-link manifest should be writable");
        fs::remove_dir_all(cache.path().join("programs"))
            .expect("workspace bootstrap entry should be removable");
        let installed_cold = Command::new(&installed_aura)
            .env("AURORA_CACHE_DIR", cache.path())
            .arg("run")
            .arg("--backend")
            .arg("direct")
            .arg(&source_path)
            .output()
            .expect("failed to populate the installed native cache");
        assert!(
            installed_cold.status.success(),
            "installed native-cache cold run failed, stderr was:\n{}",
            String::from_utf8_lossy(&installed_cold.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&installed_cold.stdout), "cached\n");

        let mut entries = fs::read_dir(cache.path().join("programs"))
            .expect("installed program cache should exist")
            .map(|entry| entry.expect("cache entry should be readable").path())
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(
            entries.len(),
            1,
            "fixture should publish exactly one installed program entry, found {entries:?}"
        );

        Self {
            cache,
            _install: install,
            installed_aura,
            _source: source,
            source_path,
            entry: entries.remove(0),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.installed_aura);
        command
            .env("AURORA_CACHE_DIR", self.cache.path())
            .arg("run")
            .arg("--backend")
            .arg("direct")
            .arg(&self.source_path);
        command
    }

    fn program(&self) -> PathBuf {
        self.entry.join("program")
    }

    fn digest(&self) -> PathBuf {
        self.entry.join("program.sha256")
    }
}

#[cfg(unix)]
fn replace_file_with_fifo(path: &std::path::Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    fs::remove_file(path).expect("regular cache member should be removable");
    let path = CString::new(path.as_os_str().as_bytes())
        .expect("temporary cache path should not contain a nul byte");
    let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
    assert_eq!(
        result,
        0,
        "failed to create cache-member FIFO: {}",
        std::io::Error::last_os_error()
    );
}

#[cfg(unix)]
#[test]
fn native_cache_creates_every_new_path_component_private_under_permissive_umask() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;

    let root = TempDir::new("aurora-native-cache-private-components");
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))
        .expect("pre-existing cache parent should be private");
    let cache = root.path().join("new-parent").join("new-cache");
    let (_source, source_path) = write_temp_source(
        "aurora-native-cache-private-components-run",
        "def main() -> int32:\n    return 0\n",
    );

    let mut command = Command::new(aura_bin());
    command
        .env("AURORA_CACHE_DIR", &cache)
        .arg("run")
        .arg("--backend")
        .arg("direct")
        .arg(&source_path);
    // Change the mask in the child immediately before exec so this test does
    // not mutate process-global state while other Rust tests are running.
    unsafe {
        command.pre_exec(|| {
            libc::umask(0o000);
            Ok(())
        });
    }
    let output = command
        .output()
        .expect("failed to run aura with a permissive umask");
    assert!(
        output.status.success(),
        "direct run should succeed with a private cache, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    for path in [root.path().join("new-parent"), cache] {
        assert_eq!(
            fs::metadata(&path)
                .unwrap_or_else(|error| panic!("{} should exist: {error}", path.display()))
                .permissions()
                .mode()
                & 0o777,
            0o700,
            "new cache component `{}` must never be group- or world-accessible",
            path.display()
        );
    }
}

#[test]
fn native_run_cache_verifies_artifacts_rebuilds_invalid_entries_and_keys_on_the_program() {
    let cache = TempDir::new("aurora-native-cache");
    let source = "def main() -> int32:\n    print(\"cached\")\n    return 0\n";
    let (_temp, source_path) = write_temp_source("aurora-native-cache-run", source);

    let run = |path: &std::path::Path| {
        Command::new(aura_bin())
            .env("AURORA_CACHE_DIR", cache.path())
            .arg("run")
            .arg("--backend")
            .arg("direct")
            .arg(path.display().to_string())
            .output()
            .expect("failed to run aura run --backend direct")
    };

    let cold = run(&source_path);
    assert!(
        cold.status.success(),
        "cold run failed, stderr was:\n{}",
        String::from_utf8_lossy(&cold.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&cold.stdout), "cached\n");
    assert!(
        String::from_utf8_lossy(&cold.stderr).contains("aura: rebuilding native runtime..."),
        "a cold direct run must explain the native-runtime rebuild, stderr was:\n{}",
        String::from_utf8_lossy(&cold.stderr)
    );

    let entries = |label: &str| {
        let mut found = fs::read_dir(cache.path().join("programs"))
            .unwrap_or_else(|error| panic!("{label}: cache directory should exist: {error}"))
            .map(|entry| entry.expect("cache entry should be readable").file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        found.sort();
        found
    };

    let after_cold = entries("cold");
    assert_eq!(
        after_cold.len(),
        1,
        "one cached binary, found {after_cold:?}"
    );
    // A published entry is never a staged temporary.
    assert!(
        !after_cold[0].starts_with('.'),
        "cache published a staged name: {after_cold:?}"
    );
    let cached_entry = cache.path().join("programs").join(&after_cold[0]);
    let cached_binary = cached_entry.join("program");
    let cached_digest = cached_entry.join("program.sha256");
    assert!(
        cached_binary.is_file(),
        "the cache entry must contain the native program"
    );
    assert!(
        cached_digest.is_file(),
        "the cache entry must record the program's own content hash"
    );

    let cached_contents = fs::read(&cached_binary).expect("cached binary should be readable");
    let expected_digest = aurora_compiler::sha256_hex(&cached_contents);
    assert_eq!(
        fs::read_to_string(&cached_digest)
            .expect("cached digest should be readable")
            .trim(),
        expected_digest,
        "the stored digest must describe the cached program bytes"
    );
    let verified_binary_modified = fs::metadata(&cached_binary)
        .expect("cached binary metadata should be readable")
        .modified()
        .expect("cached binary modification time should be readable");
    let verified_digest = fs::read(&cached_digest).expect("cached digest should be readable");
    std::thread::sleep(std::time::Duration::from_millis(20));
    // A valid hit must return before compiler/linker selection. Poisoning CC
    // makes this a behavioral proof of reuse: an implementation that silently
    // rebuilds on every invocation fails instead of passing because an
    // existing content-addressed entry masks the attempted republish.
    let missing_cc = cache.path().join("missing-cc");
    let warm = Command::new(aura_bin())
        .env("AURORA_CACHE_DIR", cache.path())
        .env("CC", &missing_cc)
        .arg("run")
        .arg("--backend")
        .arg("direct")
        .arg(source_path.display().to_string())
        .output()
        .expect("failed to run a verified native cache hit");
    assert!(
        warm.status.success(),
        "a verified cache hit must not invoke the poisoned compiler, stderr was:\n{}",
        String::from_utf8_lossy(&warm.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&warm.stdout), "cached\n");
    assert_eq!(
        entries("warm"),
        after_cold,
        "a warm run must reuse the cached binary rather than publish another"
    );
    assert_eq!(
        fs::metadata(&cached_binary)
            .expect("verified cached binary should remain readable")
            .modified()
            .expect("verified cached binary modification time should remain readable"),
        verified_binary_modified,
        "a verified cache hit must launch without rebuilding or rewriting"
    );
    assert_eq!(
        fs::read(&cached_digest).expect("verified digest should remain readable"),
        verified_digest,
        "a verified cache hit must not rewrite its digest"
    );

    // A syntactically valid identity that is bound to another content key is
    // still corrupt metadata. The mismatched entry must be removed, rebuilt,
    // and retained as the next warm hit rather than restored after quarantine.
    let cached_entry_id = cached_entry.join("entry-id");
    let wrong_key = if after_cold[0] == "0".repeat(64) {
        "1".repeat(64)
    } else {
        "0".repeat(64)
    };
    fs::write(
        &cached_entry_id,
        format!("{wrong_key}:{}\n", "2".repeat(64)),
    )
    .expect("cached entry identity should be replaceable");
    let after_entry_id_mismatch = run(&source_path);
    assert!(
        after_entry_id_mismatch.status.success(),
        "entry-id mismatch should rebuild, stderr was:\n{}",
        String::from_utf8_lossy(&after_entry_id_mismatch.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&after_entry_id_mismatch.stdout),
        "cached\n"
    );
    assert!(
        fs::read_to_string(&cached_entry_id)
            .expect("entry-id mismatch rebuild should publish an identity")
            .starts_with(&format!("{}:", after_cold[0])),
        "rebuilt entry identity must be bound to its content key"
    );
    let retained_identity = Command::new(aura_bin())
        .env("AURORA_CACHE_DIR", cache.path())
        .env("CC", &missing_cc)
        .arg("run")
        .arg("--backend")
        .arg("direct")
        .arg(source_path.display().to_string())
        .output()
        .expect("failed to run after rebuilding mismatched entry identity");
    assert!(
        retained_identity.status.success(),
        "the rebuilt entry must be retained as a warm hit, stderr was:\n{}",
        String::from_utf8_lossy(&retained_identity.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&retained_identity.stdout),
        "cached\n"
    );

    // A native-shaped artifact with the wrong recorded digest must rebuild.
    // This separately proves that hit verification consults the sidecar rather
    // than accepting the executable header alone.
    fs::write(&cached_digest, format!("{}\n", "0".repeat(64)))
        .expect("cached digest should be replaceable");
    let after_digest_mismatch = run(&source_path);
    assert!(
        after_digest_mismatch.status.success(),
        "digest mismatch should rebuild, stderr was:\n{}",
        String::from_utf8_lossy(&after_digest_mismatch.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&after_digest_mismatch.stdout),
        "cached\n"
    );
    let rebuilt_contents =
        fs::read(&cached_binary).expect("digest-mismatch rebuild should publish a binary");
    assert_eq!(
        fs::read_to_string(&cached_digest)
            .expect("digest-mismatch rebuild should publish a digest")
            .trim(),
        aurora_compiler::sha256_hex(&rebuilt_contents),
        "a digest mismatch must be replaced by a self-verifying entry"
    );

    // A truncated artifact must fail content verification and rebuild. In
    // particular, it must never reach the macOS ENOEXEC shell fallback.
    fs::write(&cached_binary, []).expect("cached binary should be truncatable");
    let after_truncate = run(&source_path);
    assert!(
        after_truncate.status.success(),
        "truncated cache entry should rebuild, stderr was:\n{}",
        String::from_utf8_lossy(&after_truncate.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&after_truncate.stdout), "cached\n");
    assert!(
        fs::metadata(&cached_binary)
            .expect("rebuilt cached binary should exist")
            .len()
            > 0,
        "the truncated artifact must be replaced"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&cached_binary)
            .expect("cached binary permissions should be readable")
            .permissions();
        permissions.set_mode(permissions.mode() & !0o111);
        fs::set_permissions(&cached_binary, permissions)
            .expect("cached execute permissions should be removable");
        let after_unlaunchable = run(&source_path);
        assert!(
            after_unlaunchable.status.success(),
            "unlaunchable verified entry should rebuild, stderr was:\n{}",
            String::from_utf8_lossy(&after_unlaunchable.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&after_unlaunchable.stdout),
            "cached\n"
        );
        assert_ne!(
            fs::metadata(&cached_binary)
                .expect("unlaunchable entry should be replaced")
                .permissions()
                .mode()
                & 0o111,
            0,
            "the rebuilt cache entry must be executable"
        );
    }

    // A digest-matching file with a plausible native magic can still be a
    // malformed executable. It must reach a no-shell-fallback launch probe,
    // fail as cache state, and rebuild rather than becoming a program result.
    let wrong_bytes: &[u8] = if cfg!(target_os = "macos") {
        b"\xcf\xfa\xed\xfeexit 0\n"
    } else if cfg!(target_os = "linux") {
        b"\x7fELFexit 0\n"
    } else {
        b"native-format-invalid\n"
    };
    fs::write(&cached_binary, wrong_bytes).expect("cached binary should be replaceable");
    fs::write(
        &cached_digest,
        format!("{}\n", aurora_compiler::sha256_hex(wrong_bytes)),
    )
    .expect("matching wrong-bytes digest should be writable");
    let after_wrong_bytes = run(&source_path);
    assert!(
        after_wrong_bytes.status.success(),
        "malformed native-shaped cache entry should rebuild, stderr was:\n{}",
        String::from_utf8_lossy(&after_wrong_bytes.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&after_wrong_bytes.stdout),
        "cached\n",
        "digest-matching malformed native bytes must not become a program result"
    );
    assert!(
        fs::metadata(&cached_binary)
            .expect("wrong-shape artifact should be replaced")
            .len()
            > wrong_bytes.len() as u64,
        "the wrong-shape artifact must be replaced by a native binary"
    );

    // Changing the program changes the content key, so the cache gains a
    // second entry rather than launching the stale binary.
    let (_changed_temp, changed_path) = write_temp_source(
        "aurora-native-cache-changed",
        "def main() -> int32:\n    print(\"changed\")\n    return 0\n",
    );
    let changed = run(&changed_path);
    assert!(changed.status.success());
    assert_eq!(String::from_utf8_lossy(&changed.stdout), "changed\n");
    let after_change = entries("changed");
    assert_eq!(
        after_change.len(),
        2,
        "a changed program should key to a new entry, found {after_change:?}"
    );

    // The runtime archive identity is memoized beside the programs, not among
    // them, so its bookkeeping can never be mistaken for a content key.
    assert!(
        cache.path().join("runtime-identity").is_file(),
        "the runtime identity memo should be recorded at the cache root"
    );
}

#[cfg(unix)]
#[test]
fn native_run_cache_serializes_concurrent_cold_runs_into_one_build_and_verified_hits() {
    use std::os::fd::AsRawFd;
    use std::sync::mpsc;

    let cache = TempDir::new("aurora-native-cache-concurrent");
    let (_source, source_path) = write_temp_source(
        "aurora-native-cache-concurrent-run",
        "def main() -> int32:\n    print(\"concurrent\")\n    return 0\n",
    );

    // Bootstrap the exact content key and its lock path, then remove only the
    // program entry. Holding that key gives every child a deterministic cold
    // miss and a real establishment barrier.
    let bootstrap = Command::new(aura_bin())
        .env("AURORA_CACHE_DIR", cache.path())
        .arg("run")
        .arg("--backend")
        .arg("direct")
        .arg(&source_path)
        .output()
        .expect("failed to bootstrap concurrent native cache key");
    assert!(
        bootstrap.status.success(),
        "concurrent-key bootstrap failed, stderr was:\n{}",
        String::from_utf8_lossy(&bootstrap.stderr)
    );
    let mut bootstrapped_entries = fs::read_dir(cache.path().join("programs"))
        .expect("bootstrapped program cache should exist")
        .map(|entry| entry.expect("bootstrap entry should be readable").path())
        .collect::<Vec<_>>();
    assert_eq!(bootstrapped_entries.len(), 1);
    let bootstrapped_entry = bootstrapped_entries.remove(0);
    let key = bootstrapped_entry
        .file_name()
        .expect("bootstrap entry should have a key")
        .to_string_lossy()
        .into_owned();
    fs::remove_dir_all(&bootstrapped_entry)
        .expect("bootstrap program entry should be removable for the cold barrier");
    let lock_path = cache.path().join("locks").join(format!("{key}.lock"));
    let held_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("the exact bootstrapped key lock should exist");
    assert_eq!(
        unsafe { libc::flock(held_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0,
        "the parent should hold the exact cache-key barrier"
    );

    let mut children = (0..4)
        .map(|_| {
            Command::new(aura_bin())
                .env("AURORA_CACHE_DIR", cache.path())
                .arg("run")
                .arg("--backend")
                .arg("direct")
                .arg(&source_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("failed to spawn concurrent direct run")
        })
        .collect::<Vec<_>>();

    // Read each child's first stderr line concurrently. It must be flushed
    // while this process still owns the key lock, before the child can build.
    let mut first_line_receivers = Vec::new();
    let mut stderr_readers = Vec::new();
    for child in &mut children {
        let stderr = child
            .stderr
            .take()
            .expect("concurrent stderr should be captured");
        let (sender, receiver) = mpsc::channel();
        first_line_receivers.push(receiver);
        stderr_readers.push(std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut first_line = String::new();
            let result = reader.read_line(&mut first_line);
            let _ = sender.send((result, first_line));
            let mut rest = Vec::new();
            let _ = reader.read_to_end(&mut rest);
            rest
        }));
    }
    let mut first_lines = Vec::new();
    let mut barrier_error = None;
    for (index, receiver) in first_line_receivers.into_iter().enumerate() {
        match receiver.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok((Ok(_), line)) if line == "aura: waiting for a concurrent build...\n" => {
                first_lines.push(line)
            }
            Ok((Ok(_), line)) => {
                barrier_error = Some(format!(
                    "concurrent direct run {index} reported the wrong pre-block line: {line:?}"
                ));
                break;
            }
            Ok((Err(error), _)) => {
                barrier_error = Some(format!(
                    "concurrent direct run {index} stderr read failed: {error}"
                ));
                break;
            }
            Err(error) => {
                barrier_error = Some(format!(
                    "concurrent direct run {index} did not flush its wait line before blocking: {error}"
                ));
                break;
            }
        }
    }
    drop(held_lock);
    if let Some(error) = barrier_error {
        for child in &mut children {
            let _ = child.kill();
            let _ = child.wait();
        }
        panic!("{error}");
    }

    let mut outputs = Vec::new();
    for (index, (child, stderr_reader)) in children.iter_mut().zip(stderr_readers).enumerate() {
        let status =
            wait_with_timeout(child, std::time::Duration::from_secs(60)).unwrap_or_else(|| {
                let _ = child.kill();
                let _ = child.wait();
                panic!("concurrent direct run {index} did not finish within 60 seconds")
            });
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        child
            .stdout
            .take()
            .expect("concurrent stdout should be captured")
            .read_to_end(&mut stdout)
            .expect("concurrent stdout should be readable");
        stderr.extend_from_slice(first_lines[index].as_bytes());
        stderr.extend(
            stderr_reader
                .join()
                .expect("concurrent stderr reader should finish"),
        );
        outputs.push(std::process::Output {
            status,
            stdout,
            stderr,
        });
    }

    for (index, output) in outputs.iter().enumerate() {
        assert!(
            output.status.success(),
            "concurrent direct run {index} failed; stderr was:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "concurrent\n",
            "concurrent direct run {index} produced the wrong result"
        );
    }

    let rebuilds = outputs
        .iter()
        .filter(|output| {
            String::from_utf8_lossy(&output.stderr).contains("aura: rebuilding native runtime...")
        })
        .count();
    assert_eq!(
        rebuilds,
        1,
        "four concurrent cold runs must perform exactly one build; stderr was:\n{}",
        outputs
            .iter()
            .map(|output| String::from_utf8_lossy(&output.stderr))
            .collect::<Vec<_>>()
            .join("\n---\n")
    );
    for output in &outputs {
        assert_eq!(
            String::from_utf8_lossy(&output.stderr)
                .matches("aura: waiting for a concurrent build...")
                .count(),
            1,
            "each run must deduplicate its wait notice"
        );
    }

    let entries = fs::read_dir(cache.path().join("programs"))
        .expect("concurrent program cache should exist")
        .map(|entry| {
            entry
                .expect("concurrent cache entry should be readable")
                .path()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        entries.len(),
        1,
        "concurrent runs must publish exactly one verified cache entry, found {entries:?}"
    );

    let warm = Command::new(aura_bin())
        .env("AURORA_CACHE_DIR", cache.path())
        .env("CC", cache.path().join("missing-cc"))
        .env("CARGO", cache.path().join("missing-cargo"))
        .arg("run")
        .arg("--backend")
        .arg("direct")
        .arg(&source_path)
        .output()
        .expect("failed to run poisoned-toolchain verified hit");
    assert!(
        warm.status.success(),
        "the established entry must be a verified hit with CC and CARGO unavailable, stderr was:\n{}",
        String::from_utf8_lossy(&warm.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&warm.stdout), "concurrent\n");
    assert!(
        !String::from_utf8_lossy(&warm.stderr).contains("rebuilding native runtime"),
        "a poisoned-toolchain warm hit must not rebuild"
    );
}

#[cfg(unix)]
#[test]
fn native_run_cache_unrelated_warm_hit_does_not_wait_for_another_key() {
    use std::os::fd::AsRawFd;

    let fixture = NativeCacheFixture::new("aurora-native-cache-per-key");
    let cache = &fixture.cache;
    let first_path = &fixture.source_path;
    let (_second_source, second_path) = write_temp_source(
        "aurora-native-cache-per-key-second",
        "def main() -> int32:\n    print(\"second\")\n    return 0\n",
    );
    let run = |path: &std::path::Path| {
        Command::new(&fixture.installed_aura)
            .env("AURORA_CACHE_DIR", cache.path())
            .arg("run")
            .arg("--backend")
            .arg("direct")
            .arg(path)
            .output()
            .expect("failed to populate per-key native cache")
    };
    let first_key = fixture
        .entry
        .file_name()
        .expect("first program should publish a cache entry")
        .to_string_lossy()
        .into_owned();

    let second = run(&second_path);
    assert!(
        second.status.success(),
        "second per-key cold run failed, stderr was:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&second.stdout), "second\n");
    let mut keys = fs::read_dir(cache.path().join("programs"))
        .expect("per-key program cache should exist")
        .map(|entry| {
            entry
                .expect("per-key cache entry should be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    keys.sort();
    assert_eq!(keys.len(), 2, "two programs should produce two cache keys");

    let warm = |label: &str, path: &std::path::Path| {
        let mut warm_command = Command::new(&fixture.installed_aura);
        warm_command
            .env("AURORA_CACHE_DIR", cache.path())
            .env("CC", cache.path().join("missing-cc"))
            .env("CARGO", cache.path().join("missing-cargo"))
            .arg("run")
            .arg("--backend")
            .arg("direct")
            .arg(path);
        command_output_with_timeout(warm_command, std::time::Duration::from_secs(10), label)
    };

    // Installed runtime inputs are immutable and therefore require no
    // target-global runtime lease. Holding one exact program-key writer now
    // isolates the property under test: verified hits for both that same key
    // and an unrelated key must return through the optimistic read path.
    let lock_path = cache.path().join("locks").join(format!("{first_key}.lock"));
    let held_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("the first cache-key lock should exist");
    assert_eq!(
        unsafe { libc::flock(held_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0,
        "the test should hold one otherwise-idle cache-key lock"
    );

    for (label, path, expected) in [
        ("same-key warm hit", first_path.as_path(), "cached\n"),
        ("unrelated warm hit", &second_path, "second\n"),
    ] {
        let warm = warm(label, path);
        assert!(
            warm.status.success(),
            "{label} must not wait or rebuild, stderr was:\n{}",
            String::from_utf8_lossy(&warm.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&warm.stdout), expected);
        let stderr = String::from_utf8_lossy(&warm.stderr);
        assert!(
            !stderr.contains("aura: waiting for a concurrent build..."),
            "{label} must not wait on the held cache-key writer: {stderr}"
        );
        assert!(
            !stderr.contains("aura: rebuilding native runtime..."),
            "{label} must not rebuild through the poisoned toolchain: {stderr}"
        );
    }
    drop(held_lock);
}

#[test]
fn direct_run_json_failure_remains_one_document_when_a_rebuild_is_needed() {
    let cache = TempDir::new("aurora-native-json-rebuild");
    let (_source, source_path) = write_temp_source(
        "aurora-native-json-rebuild-source",
        "def main() -> int32:\n    return 0\n",
    );
    let output = Command::new(aura_bin())
        .env("AURORA_CACHE_DIR", cache.path())
        .env("CC", cache.path().join("missing-cc"))
        .arg("run")
        .arg("--format")
        .arg("json")
        .arg("--backend")
        .arg("direct")
        .arg(&source_path)
        .output()
        .expect("failed to run JSON-mode direct rebuild failure");
    assert!(
        !output.status.success(),
        "a missing linker must fail the forced direct backend"
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stderr).unwrap_or_else(|error| {
            panic!(
                "JSON-mode stderr must remain exactly one document: {error}; stderr was:\n{}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["diagnostics"].as_array().map(Vec::len), Some(1));
    assert!(
        report["diagnostics"][0]["notes"]
            .as_array()
            .is_some_and(|notes| notes
                .iter()
                .any(|note| note == "aura: rebuilding native runtime...")),
        "JSON mode must preserve the exact rebuild notice as structured progress: {report}"
    );
}

#[cfg(unix)]
#[test]
fn direct_run_json_buffers_wait_progress_into_one_document() {
    use std::os::fd::AsRawFd;

    let fixture = NativeCacheFixture::new("aurora-native-json-wait");
    let key = fixture
        .entry
        .file_name()
        .expect("the populated entry should have a content key")
        .to_string_lossy()
        .into_owned();
    fs::remove_dir_all(&fixture.entry)
        .expect("the populated entry should be removable to force a cache miss");

    // Hold the exact content-key lock, not a neighboring or synthetic lock.
    // JSON progress is intentionally buffered to preserve the one-document
    // stderr contract, so the blocked child must not emit a partial document.
    let lock_path = fixture
        .cache
        .path()
        .join("locks")
        .join(format!("{key}.lock"));
    let held_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("the populated entry's exact content-key lock should exist");
    assert_eq!(
        unsafe { libc::flock(held_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0,
        "the test should hold the exact content-key barrier"
    );

    let mut child = Command::new(&fixture.installed_aura)
        .env("AURORA_CACHE_DIR", fixture.cache.path())
        .arg("run")
        .arg("--format")
        .arg("json")
        .arg("--backend")
        .arg("direct")
        .arg(&fixture.source_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn JSON-mode direct run behind the cache-key barrier");

    // The bounded poll is the only observable pre-release assertion available
    // for deliberately buffered JSON output. The final exact wait message
    // below proves that the child reached this held lock during the window.
    if let Some(status) = wait_with_timeout(&mut child, std::time::Duration::from_secs(3)) {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        child
            .stdout
            .take()
            .expect("JSON wait stdout should be captured")
            .read_to_end(&mut stdout)
            .expect("JSON wait stdout should be readable");
        child
            .stderr
            .take()
            .expect("JSON wait stderr should be captured")
            .read_to_end(&mut stderr)
            .expect("JSON wait stderr should be readable");
        panic!(
            "JSON-mode direct run completed before the held content-key lock was released \
             (status {status}); stdout was:\n{}stderr was:\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
    }

    drop(held_lock);
    let status =
        wait_with_timeout(&mut child, std::time::Duration::from_secs(60)).unwrap_or_else(|| {
            let _ = child.kill();
            let _ = child.wait();
            panic!("JSON-mode direct run did not finish after releasing the content-key lock")
        });
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .expect("JSON wait stdout should be captured")
        .read_to_end(&mut stdout)
        .expect("JSON wait stdout should be readable");
    child
        .stderr
        .take()
        .expect("JSON wait stderr should be captured")
        .read_to_end(&mut stderr)
        .expect("JSON wait stderr should be readable");

    assert!(
        status.success(),
        "JSON-mode direct run should succeed after the lock release; stderr was:\n{}",
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(String::from_utf8_lossy(&stdout), "cached\n");
    let stderr_text = String::from_utf8(stderr).expect("JSON-mode stderr should be UTF-8");
    assert_eq!(
        stderr_text.lines().count(),
        1,
        "JSON-mode stderr must contain exactly one JSON document: {stderr_text:?}"
    );
    let report: serde_json::Value = serde_json::from_str(&stderr_text).unwrap_or_else(|error| {
        panic!(
            "JSON-mode wait stderr must be exactly one JSON document: {error}; stderr was:\n\
                 {stderr_text}"
        )
    });
    assert_eq!(report["schema_version"], 1);
    let progress = report["progress"]
        .as_array()
        .unwrap_or_else(|| panic!("JSON wait report should contain progress: {report}"));
    assert_eq!(
        progress
            .iter()
            .filter(|message| *message == "aura: waiting for a concurrent build...")
            .count(),
        1,
        "the buffered report must preserve exactly one exact wait notice: {report}"
    );
}

#[cfg(unix)]
#[test]
fn auto_run_json_fallback_preserves_native_progress_in_one_document() {
    let fixture = NativeCacheFixture::new("aurora-native-json-auto-fallback");
    fs::remove_dir_all(&fixture.entry)
        .expect("the warm entry should be removable to force a direct build");
    let output = Command::new(&fixture.installed_aura)
        .env("AURORA_CACHE_DIR", fixture.cache.path())
        .env("CC", fixture.cache.path().join("missing-cc"))
        .arg("run")
        .arg("--format")
        .arg("json")
        .arg("--backend")
        .arg("auto")
        .arg(&fixture.source_path)
        .output()
        .expect("failed to run JSON-mode automatic backend fallback");
    assert!(
        output.status.success(),
        "the MIR fallback should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "cached\n");
    let report: serde_json::Value =
        serde_json::from_slice(&output.stderr).unwrap_or_else(|error| {
            panic!(
                "JSON-mode fallback stderr must remain exactly one document: {error}; stderr was:\n{}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    assert_eq!(report["schema_version"], 1);
    assert!(
        report["progress"]
            .as_array()
            .is_some_and(|progress| progress
                .iter()
                .any(|message| message == "aura: rebuilding native runtime...")),
        "the automatic fallback must retain the exact direct rebuild notice: {report}"
    );
    assert_eq!(report["fallback"]["from"], "direct");
    assert_eq!(report["fallback"]["to"], "mir");
    assert!(
        report["fallback"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("failed to run native linker")),
        "the structured fallback must retain the direct failure reason: {report}"
    );
}

#[cfg(unix)]
#[test]
fn installed_direct_run_keeps_native_cache_optional_for_build_locking() {
    let bootstrap_cache = TempDir::new("aurora-installed-no-cache-bootstrap");
    let (_source, source_path) = write_temp_source(
        "aurora-installed-no-cache-source",
        "def main() -> int32:\n    print(\"uncached\")\n    return 0\n",
    );
    let bootstrap = Command::new(aura_bin())
        .env("AURORA_CACHE_DIR", bootstrap_cache.path())
        .arg("run")
        .arg("--backend")
        .arg("direct")
        .arg(&source_path)
        .output()
        .expect("failed to establish installable runtime artifacts");
    assert!(
        bootstrap.status.success(),
        "runtime bootstrap failed, stderr was:\n{}",
        String::from_utf8_lossy(&bootstrap.stderr)
    );

    let prefix = TempDir::new("aurora-installed-no-cache-prefix");
    let bin_dir = prefix.path().join("bin");
    let runtime_dir = prefix.path().join("lib").join("aurora");
    fs::create_dir_all(&bin_dir).expect("installed bin directory should be creatable");
    fs::create_dir_all(&runtime_dir).expect("installed runtime directory should be creatable");
    let installed_aura = bin_dir.join("aura");
    fs::copy(aura_bin(), &installed_aura).expect("aura executable should be installable");
    fs::copy(
        repo_root()
            .join("target")
            .join("debug")
            .join("libaurora_compiler.a"),
        runtime_dir.join("libaurora_compiler.a"),
    )
    .expect("native runtime archive should be installable");
    let runtime_memo = fs::read_to_string(bootstrap_cache.path().join("runtime-identity"))
        .expect("bootstrap should record native link arguments");
    let native_link_args = runtime_memo
        .lines()
        .nth(2)
        .expect("runtime memo should contain native link arguments");
    fs::write(
        runtime_dir.join("native-link-args.json"),
        format!("{native_link_args}\n"),
    )
    .expect("installed native-link manifest should be writable");

    let output = Command::new(&installed_aura)
        .env("AURORA_CACHE_DIR", "")
        .env_remove("HOME")
        .arg("run")
        .arg("--backend")
        .arg("direct")
        .arg(&source_path)
        .output()
        .expect("failed to run installed aura without a native cache");
    assert!(
        output.status.success(),
        "installed direct execution must not require a cache merely to lock, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "uncached\n");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("aura: rebuilding native runtime..."),
        "an uncached installed build should still report its long operation"
    );
}

#[cfg(unix)]
#[test]
fn native_run_cache_rejects_symlink_and_fifo_members_without_blocking_or_leaking() {
    use std::os::unix::fs::symlink;

    let fixture = NativeCacheFixture::new("aurora-native-cache-non-regular");
    let timeout = std::time::Duration::from_secs(10);
    let missing_cc = fixture.cache.path().join("missing-cc");
    let launch_temp = fixture.cache.path().join("launch-temp");
    fs::create_dir(&launch_temp).expect("controlled launch temp should be creatable");

    // A verified warm hit is staged privately, and every launch artifact must
    // be removed again after the child exits.
    let mut warm_command = fixture.command();
    warm_command
        .env("CC", &missing_cc)
        .env("TMPDIR", &launch_temp);
    let warm = command_output_with_timeout(warm_command, timeout, "verified native cache hit");
    assert!(
        warm.status.success(),
        "verified hit failed with {:?}; stdout was:\n{}\nstderr was:\n{}",
        warm.status,
        String::from_utf8_lossy(&warm.stdout),
        String::from_utf8_lossy(&warm.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&warm.stdout), "cached\n");
    let leaked_launch_artifacts = fs::read_dir(&launch_temp)
        .expect("controlled launch temp should remain readable")
        .map(|entry| {
            entry
                .expect("launch-temp entry should be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with("aurora-verified-native-"))
        .collect::<Vec<_>>();
    assert!(
        leaked_launch_artifacts.is_empty(),
        "successful verified hit leaked private launch artifacts: {leaked_launch_artifacts:?}"
    );

    // A symlinked program must not be followed even when its target is a real
    // native executable and the sidecar matches that target.
    let external_program = fixture.cache.path().join("external-program");
    fs::copy("/bin/echo", &external_program).expect("external native executable should copy");
    let external_contents =
        fs::read(&external_program).expect("external native executable should be readable");
    fs::remove_file(fixture.program()).expect("cached program should be removable");
    symlink(&external_program, fixture.program()).expect("program symlink should be creatable");
    fs::write(
        fixture.digest(),
        format!("{}\n", aurora_compiler::sha256_hex(&external_contents)),
    )
    .expect("program-symlink digest should be writable");
    let program_symlink = command_output_with_timeout(
        fixture.command(),
        timeout,
        "native cache entry with a program symlink",
    );
    assert!(
        program_symlink.status.success(),
        "program symlink should cause a rebuild, stderr was:\n{}",
        String::from_utf8_lossy(&program_symlink.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&program_symlink.stdout),
        "cached\n",
        "the symlink target must never become the program result"
    );
    assert!(
        fs::symlink_metadata(fixture.program())
            .expect("rebuilt program should exist")
            .file_type()
            .is_file(),
        "the program symlink should be replaced by a regular cached binary"
    );
    assert!(
        external_program.is_file(),
        "rejecting a symlink must not remove its external target"
    );

    // A symlinked digest is invalid cache structure even when it names the
    // correct digest. Rejecting it pins no-follow behavior for both members.
    let external_digest = fixture.cache.path().join("external-digest");
    let current_program =
        fs::read(fixture.program()).expect("rebuilt cached program should be readable");
    fs::write(
        &external_digest,
        format!("{}\n", aurora_compiler::sha256_hex(&current_program)),
    )
    .expect("external digest should be writable");
    fs::remove_file(fixture.digest()).expect("cached digest should be removable");
    symlink(&external_digest, fixture.digest()).expect("digest symlink should be creatable");
    let digest_symlink = command_output_with_timeout(
        fixture.command(),
        timeout,
        "native cache entry with a digest symlink",
    );
    assert!(
        digest_symlink.status.success(),
        "digest symlink should cause a rebuild, stderr was:\n{}",
        String::from_utf8_lossy(&digest_symlink.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&digest_symlink.stdout), "cached\n");
    assert!(
        fs::symlink_metadata(fixture.digest())
            .expect("rebuilt digest should exist")
            .file_type()
            .is_file(),
        "the digest symlink should be replaced by a regular sidecar"
    );
    assert!(
        external_digest.is_file(),
        "rejecting a digest symlink must not remove its external target"
    );

    // FIFOs are especially important: opening either member for an
    // unconditional read blocks forever when there is no writer. Metadata
    // validation must reject the node before any read is attempted.
    for (label, member) in [
        ("program FIFO", fixture.program()),
        ("digest FIFO", fixture.digest()),
    ] {
        replace_file_with_fifo(&member);
        let rebuilt = command_output_with_timeout(fixture.command(), timeout, label);
        assert!(
            rebuilt.status.success(),
            "{label} should cause a rebuild, stderr was:\n{}",
            String::from_utf8_lossy(&rebuilt.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&rebuilt.stdout),
            "cached\n",
            "{label} must never become a program result"
        );
        assert!(
            fs::symlink_metadata(&member)
                .unwrap_or_else(|error| panic!("{label} should be replaced: {error}"))
                .file_type()
                .is_file(),
            "{label} should be replaced by a regular cache member"
        );
    }
}

#[cfg(unix)]
#[test]
fn native_run_cache_preserves_verified_entry_when_private_launch_staging_fails() {
    use std::os::unix::fs::MetadataExt;

    let fixture = NativeCacheFixture::new("aurora-native-cache-launch-environment");
    let program_before =
        fs::read(fixture.program()).expect("cached program should be readable before launch");
    let digest_before =
        fs::read(fixture.digest()).expect("cached digest should be readable before launch");
    let entry_metadata =
        fs::metadata(&fixture.entry).expect("cache entry metadata should be readable");
    let program_metadata =
        fs::metadata(fixture.program()).expect("cached program metadata should be readable");
    let digest_metadata =
        fs::metadata(fixture.digest()).expect("cached digest metadata should be readable");

    // Rust's Unix temp-dir selection honors TMPDIR. Pointing it at a regular
    // file makes private launch staging fail for environmental reasons after
    // the shared cache bytes have already verified. That must not be confused
    // with evidence that the valid cache entry itself is corrupt.
    let unusable_tmp = fixture.cache.path().join("tmp-is-a-file");
    fs::write(&unusable_tmp, "not a directory").expect("unusable TMPDIR marker should be writable");
    let mut command = fixture.command();
    command
        .env("TMPDIR", &unusable_tmp)
        .env("CC", fixture.cache.path().join("missing-cc"));
    let output = command_output_with_timeout(
        command,
        std::time::Duration::from_secs(10),
        "verified launch with unusable TMPDIR",
    );
    assert!(
        !output.status.success(),
        "a regular-file TMPDIR should exercise the private-staging failure path; stdout was:\n{}\nstderr was:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("failed to create private verified-native directory"),
        "expected an environmental private-staging diagnostic, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        fs::read(fixture.program()).expect("environmental failure must preserve cached program"),
        program_before,
        "environmental launch failure must not rewrite cached program bytes"
    );
    assert_eq!(
        fs::read(fixture.digest()).expect("environmental failure must preserve cached digest"),
        digest_before,
        "environmental launch failure must not rewrite the verified sidecar"
    );
    let entry_after =
        fs::metadata(&fixture.entry).expect("environmental failure must preserve cache entry");
    let program_after =
        fs::metadata(fixture.program()).expect("environmental failure must preserve program");
    let digest_after =
        fs::metadata(fixture.digest()).expect("environmental failure must preserve digest");
    assert_eq!(
        (entry_after.dev(), entry_after.ino()),
        (entry_metadata.dev(), entry_metadata.ino()),
        "environmental launch failure must not replace the cache entry"
    );
    assert_eq!(
        (program_after.dev(), program_after.ino()),
        (program_metadata.dev(), program_metadata.ino()),
        "environmental launch failure must not replace the cached program"
    );
    assert_eq!(
        (digest_after.dev(), digest_after.ino()),
        (digest_metadata.dev(), digest_metadata.ino()),
        "environmental launch failure must not replace the digest sidecar"
    );
}

#[test]
fn run_backend_selector_matches_across_mir_direct_and_auto() {
    let source = "import sys\n\ndef main() -> int32:\n    print(\"selector\")\n    for arg in sys.args():\n        print(arg)\n    return 3\n";
    let (_temp, source_path) = write_temp_source("aurora-run-backend-selector", source);
    let expected = "selector\nalpha\nbeta\n";

    for backend in ["mir", "direct", "auto"] {
        let output = Command::new(aura_bin())
            .arg("run")
            .arg("--backend")
            .arg(backend)
            .arg(source_path.display().to_string())
            .arg("--")
            .arg("alpha")
            .arg("beta")
            .output()
            .unwrap_or_else(|error| panic!("failed to run aura run --backend {backend}: {error}"));

        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{backend} stdout, stderr was:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.status.code(), Some(3), "{backend} exit code");
    }

    // The default is still the MIR runtime, and it agrees with every explicit
    // selector.
    let default_run = Command::new(aura_bin())
        .arg("run")
        .arg(source_path.display().to_string())
        .arg("--")
        .arg("alpha")
        .arg("beta")
        .output()
        .expect("failed to run aura run with the default backend");
    assert_eq!(String::from_utf8_lossy(&default_run.stdout), expected);
    assert_eq!(default_run.status.code(), Some(3));

    let rejected = Command::new(aura_bin())
        .arg("run")
        .arg("--backend")
        .arg("interpreter")
        .arg(source_path.display().to_string())
        .output()
        .expect("failed to run aura run with an unknown backend");
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("--backend mir|direct|auto"));
}

#[test]
fn compile_commands_accept_membership_and_comparison_chains() {
    let (temp, source_path) = write_temp_source(
        "aurora-membership-and-chains",
        "def main():\n    ports = [80, 443]\n    if 443 in ports and 1 <= 80 < 1024:\n        print(\"ok\")\n",
    );
    let output_path = temp.path().join("out");
    let commands = [
        vec![
            "check".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ],
        vec!["run".to_string()],
        vec![
            "build".to_string(),
            "--backend".to_string(),
            "direct".to_string(),
            "-o".to_string(),
            output_path.display().to_string(),
        ],
    ];

    for mut arguments in commands {
        let command_name = arguments[0].clone();
        arguments.push(source_path.display().to_string());
        let output = Command::new(aura_bin())
            .args(&arguments)
            .output()
            .unwrap_or_else(|error| panic!("failed to run aura {command_name}: {error}"));
        assert!(
            output.status.success(),
            "{command_name} should accept membership and comparison chains, stderr was:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let direct = Command::new(&output_path)
        .output()
        .expect("the direct binary should run");
    assert_eq!(String::from_utf8_lossy(&direct.stdout), "ok\n");

    let (_reject_temp, reject_path) = write_temp_source(
        "aurora-membership-rejection",
        "def main():\n    print(1 in 5)\n",
    );
    let rejected = Command::new(aura_bin())
        .args(["check", "--format", "json"])
        .arg(reject_path.display().to_string())
        .output()
        .expect("failed to run aura check");
    assert!(!rejected.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&rejected.stderr).expect("check should emit JSON");
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["code"], "AU2003");
    assert_eq!(
        diagnostic["message"],
        "`in` requires a `Vec[T]`, `Set[T]`, `Map[K, V]`, or `String` container, found `int64`"
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
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("usage: aura"),
            "help path {:?} should print usage",
            args
        );
        assert!(
            !stdout.contains("run-mir"),
            "help path {:?} should no longer advertise `run-mir`, stdout was:\n{}",
            args,
            stdout
        );
        assert!(
            stdout.contains("or: aura build -o <output>"),
            "help path {:?} should show that `build -o` is required, stdout was:\n{}",
            args,
            stdout
        );
        assert!(
            !stdout.contains("aura build [-o <output>]"),
            "help path {:?} must not show the required output option as optional, stdout was:\n{}",
            args,
            stdout
        );
        assert!(
            stdout
                .lines()
                .filter(|line| line.contains("aura build"))
                .all(|line| line.contains("-o <output>")),
            "every advertised build form must include required `-o <output>`, stdout was:\n{}",
            stdout
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
        "public trait Named:\n    def name(self) -> String\n",
    )
    .expect("failed to write trait module");
    fs::write(
        temp.path().join("pkg/user.au"),
        "from pkg.named import Named\n\npublic class User:\n    public label: String\n\nimpl Named for User:\n    def name(self) -> String:\n        return self.label.clone()\n",
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
        "def main() -> int32:\n    keys = [\"a\", \"b\"]\n    idx: int32 = 1\n    mut counts = {\"key\": 7}\n    match keys.get(idx):\n        case Some(key):\n            print(key)\n        case None:\n            print(\"missing\")\n    print(f\"val: {counts[\"key\"]}\")\n    return 0\n",
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
fn run_and_direct_backends_preserve_false_fs_exists_results() {
    let missing_name = format!(
        "aurora-fs-exists-false-{}-{}.missing",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos()
    );
    let missing_path = PathBuf::from(&missing_name);
    assert!(
        !missing_path.exists(),
        "the fs.exists false-result probe must start absent"
    );
    let source = format!(
        "import fs\n\ndef main() -> int32:\n    print(fs.exists(\"{}\"))\n    return 0\n",
        missing_name
    );

    assert_run_and_direct_source_stdout("aurora-fs-exists-false", &source, "false\n");
}

#[test]
fn run_and_direct_backends_preserve_the_dynamic_json_surface() {
    let source =
        include_str!("../../aurora-compiler/tests/fixtures/run-pass/json_dynamic_values.au");
    let expected =
        include_str!("../../aurora-compiler/tests/fixtures/run-pass/json_dynamic_values.stdout");

    assert_run_and_direct_source_stdout("aurora-dynamic-json-parity", source, expected);
}

#[test]
fn run_and_direct_backends_clone_json_task_results_and_clean_up_unobserved_values() {
    let source =
        include_str!("../../aurora-compiler/tests/fixtures/run-pass/task_json_result_cleanup.au");
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/task_json_result_cleanup.stdout"
    );

    assert_run_and_direct_source_stdout("aurora-json-task-result-parity", source, expected);
}

#[test]
fn run_and_direct_backends_move_deep_fields_without_consuming_siblings() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/deep_projected_move_preserves_siblings.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/deep_projected_move_preserves_siblings.stdout"
    );

    assert_run_and_direct_source_stdout("aurora-deep-projected-move-parity", source, expected);
}

#[test]
fn run_and_direct_backends_backtrack_before_moving_match_expression_payloads() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/consuming_nested_noncopy_match_expression.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/consuming_nested_noncopy_match_expression.stdout"
    );

    assert_run_and_direct_source_stdout(
        "aurora-consuming-match-expression-parity",
        source,
        expected,
    );
}

#[test]
fn run_and_direct_backends_discover_queues_nested_in_task_arguments() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/task_nested_queue_capture_lifecycle.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/task_nested_queue_capture_lifecycle.stdout"
    );

    assert_run_and_direct_source_stdout("aurora-nested-task-queue-parity", source, expected);
}

#[test]
fn run_and_direct_backends_move_noncopy_try_errors_through_from_conversion() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/try_noncopy_error_conversion.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/try_noncopy_error_conversion.stdout"
    );

    assert_run_and_direct_source_stdout("aurora-noncopy-try-from-parity", source, expected);
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
    file.set_len((FILESYSTEM_READ_CAP_BYTES + 1) as u64)
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
    file.set_len((FILESYSTEM_READ_CAP_BYTES + 1) as u64)
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
fn run_and_direct_filesystem_read_to_string_accepts_above_retired_cap() {
    let temp = TempDir::new("aurora-raised-file-read-cap");
    let file_path = temp.path().join("above-retired-cap.txt");
    let file = fs::File::create(&file_path).expect("create sparse file above retired cap");
    file.set_len((RETIRED_FILESYSTEM_READ_CAP_BYTES + 1) as u64)
        .expect("size sparse file above retired cap");
    let source = format!(
        "import fs\n\ndef main() -> int32:\n    match fs.read_to_string(\"{path}\"):\n        case Result.Ok(text):\n            print(text.byte_len())\n            return 0\n        case Result.Err(error):\n            print(error)\n            return 1\n",
        path = file_path.display()
    );

    assert_run_and_direct_source_stdout("aurora-raised-file-read-cap", &source, "67108865\n");
}

#[test]
fn run_and_direct_backend_preserve_match_borrow_mut_writebacks_after_dead_branches() {
    let source = r#"enum Opt:
    Some(int32)
    None

def main() -> int32:
    mut x: Opt = Opt.Some(10)
    match mut x:
        case Some(v):
            v = v + 1
            if false:
                x = Opt.Some(100)
        case None:
            pass
    match x:
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
fn run_and_direct_backends_preserve_int64_defaulting_boundaries_aliases_and_casts() {
    let source =
        include_str!("../../aurora-compiler/tests/fixtures/run-pass/default_integer_is_int64.au");
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/default_integer_is_int64.stdout"
    );
    assert_run_and_direct_source_stdout("aurora-int64-defaulting", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_floor_division_and_modulo() {
    let source =
        include_str!("../../aurora-compiler/tests/fixtures/run-pass/floor_division_and_modulo.au");
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/floor_division_and_modulo.stdout"
    );
    assert_run_and_direct_source_stdout("aurora-floor-division-modulo", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_floor_division_across_integer_widths_and_places() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/floor_division_integer_widths_and_places.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/floor_division_integer_widths_and_places.stdout"
    );
    assert_run_and_direct_source_stdout("aurora-floor-division-widths-places", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_integer_to_float_rounding() {
    let source =
        include_str!("../../aurora-compiler/tests/fixtures/run-pass/integer_to_float_rounding.au");
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/integer_to_float_rounding.stdout"
    );
    assert_run_and_direct_source_stdout("aurora-integer-to-float-rounding", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_integer_to_float_expression_contexts() {
    let source =
        include_str!("../../aurora-compiler/tests/fixtures/run-pass/integer_to_float_contexts.au");
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/integer_to_float_contexts.stdout"
    );
    assert_run_and_direct_source_stdout("aurora-integer-to-float-contexts", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_float_context_integer_literals() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/float_context_integer_literals.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/float_context_integer_literals.stdout"
    );
    assert_run_and_direct_source_stdout("aurora-float-context-integer-literals", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_shortest_roundtrip_float_printing() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/float_shortest_roundtrip_printing.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/float_shortest_roundtrip_printing.stdout"
    );
    assert_run_and_direct_source_stdout(
        "aurora-shortest-roundtrip-float-printing",
        source,
        expected,
    );
}

#[test]
fn run_and_direct_backends_preserve_the_numbers_example() {
    let source = include_str!("../../../examples/basics/numbers.au");
    assert_run_and_direct_source_stdout(
        "aurora-numbers-example",
        source,
        "2\n-3\n2\n-3\n-2\n3.5\n2.0\ntrue\ntrue\n42.0\n9007199254740992.0\n",
    );
}

#[test]
fn run_and_direct_backends_trap_float_floor_division_by_zero() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-fail/float_floor_division_by_zero.au"
    );
    assert_run_and_direct_source_failure_with_timeout(
        "aurora-float-floor-division-zero",
        source,
        std::time::Duration::from_secs(15),
        "",
        "division by zero",
    );
}

#[test]
fn run_and_direct_backends_trap_signed_floor_division_overflow() {
    let source =
        include_str!("../../aurora-compiler/tests/fixtures/run-fail/int64_division_overflow.au");
    assert_run_and_direct_source_failure_with_timeout(
        "aurora-int64-floor-division-overflow",
        source,
        std::time::Duration::from_secs(15),
        "",
        "integer value `9223372036854775808` does not fit in `int64`",
    );
}

#[test]
fn run_and_direct_backends_trap_boxed_int128_floor_division_overflow() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-fail/int128_floor_division_overflow.au"
    );
    assert_run_and_direct_source_failure_with_timeout(
        "aurora-int128-floor-division-overflow",
        source,
        std::time::Duration::from_secs(15),
        "0\n",
        "integer value `170141183460469231731687303715884105728` does not fit in `int128`",
    );
}

#[test]
fn run_and_direct_backends_distinguish_exact_cast_from_rounding_conversion() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-fail/int64_to_float64_cast_inexact_boundary.au"
    );
    assert_run_and_direct_source_failure_with_timeout(
        "aurora-int64-exact-float-cast-boundary",
        source,
        std::time::Duration::from_secs(15),
        "",
        "integer value `9007199254740993` cannot be represented exactly as `float64`",
    );
}

#[test]
fn run_and_direct_backends_preserve_contextual_int32_literal_inference() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/contextual_int32_literals_remain_int32.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/contextual_int32_literals_remain_int32.stdout"
    );
    assert_run_and_direct_source_stdout("aurora-contextual-int32-inference", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_default_integer_generic_dispatch() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/default_integer_generic_dispatch.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/default_integer_generic_dispatch.stdout"
    );
    assert_run_and_direct_source_stdout("aurora-default-int64-generic-dispatch", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_generic_numeric_receiver_dispatch() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/generic_numeric_receiver_dispatch.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/generic_numeric_receiver_dispatch.stdout"
    );
    assert_run_and_direct_source_stdout("generic-numeric-receiver-dispatch", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_nested_numeric_generic_dispatch() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/nested_numeric_generic_dispatch.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/nested_numeric_generic_dispatch.stdout"
    );
    assert_run_and_direct_source_stdout("nested-numeric-generic-dispatch", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_try_error_conversion_width() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/try_numeric_error_conversion_width.au"
    );
    let expected = include_str!(
        "../../aurora-compiler/tests/fixtures/run-pass/try_numeric_error_conversion_width.stdout"
    );
    assert_run_and_direct_source_stdout("try-numeric-error-conversion-width", source, expected);
}

#[test]
fn run_and_direct_backends_preserve_default_int64_to_uint64_negation_failure() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/run-fail/uint64_unary_negation_underflow.au"
    );
    assert_run_and_direct_source_failure_with_timeout(
        "aurora-default-int64-uint64-negation",
        source,
        std::time::Duration::from_secs(15),
        "",
        "integer value `-1` does not fit in `uint64`",
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
fn mir_and_forced_direct_reject_noncopy_internal_exposure() {
    let source = include_str!(
        "../../aurora-compiler/tests/fixtures/check-fail/borrowed_noncopy_return_call.au"
    );
    let (temp, source_path) = write_temp_source("aurora-borrowed-return-containment", source);
    let expected = "cannot move non-copy field `name` out of borrowed value `user`";

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
    let source = "enum Opt:\n    Some(int32)\n    None\n\ndef main() -> int32:\n    mut x: Opt = Opt.Some(10)\n    match mut x:\n        case Some(v):\n            x = Opt.Some(v)\n            v = v + 1\n        case None:\n            pass\n    return 0\n";
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
        "trait Show:\n    def show(self) -> String\n\nclass Box[T]:\n    value: T\n\nimpl[T] Show for Box[T]:\n    def show(self) -> String:\n        return \"generic\"\n\nimpl Show for Box[int32]:\n    def show(self) -> String:\n        return \"int32\"\n\ndef main() -> int32:\n    value = Box[int32](value=7)\n    print(value.show())\n    return 0\n",
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
        "def print_int_option(value: Option[int32]):\n    match value:\n        case Some(inner):\n            print(inner)\n        case None:\n            print(-1)\n\ndef main() -> int32:\n    values = Vec[int32]()\n    print(values.is_empty())\n    mut items: Vec[int32] = [1, 2, 3]\n    print(items.len())\n    print_int_option(items.get(1))\n    print_int_option(items.set(index=1, value=20))\n    print_int_option(items.remove(0))\n    items.push(99)\n    print_int_option(items.pop())\n    mut total: int32 = 0\n    for value in items:\n        total += value\n    print(total)\n    return 0\n",
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
        "def print_int_option(value: Option[int32]):\n    match value:\n        case Some(inner):\n            print(inner)\n        case None:\n            print(-1)\n\ndef main() -> int32:\n    text = \"  aurora repo  \"\n    print(text.len())\n    print(text.contains(\"repo\"))\n    print(text.starts_with(\"  au\"))\n    print(text.ends_with(\"  \"))\n    print(text.trim())\n    print(abs(-7))\n    print(min(9, 2))\n    print(max(4, 12))\n    print(sqrt(81.0))\n    mut counts: Map[String, int32] = {\"aurora\": 1, \"codex\": 2}\n    print(counts.len())\n    print(counts.contains_key(\"aurora\"))\n    print_int_option(counts.get(\"aurora\"))\n    print_int_option(counts.set(key=\"aurora\", value=5))\n    print(counts[\"aurora\"])\n    print(counts.keys().len())\n    print(counts.values().len())\n    print_int_option(counts.remove(\"codex\"))\n    print(counts.is_empty())\n    return 0\n",
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
fn string_lengths_and_negative_vec_indices_match_run_and_direct_backends() {
    let source = r#"
def print_int_option(value: Option[int32]):
    match value:
        case Some(inner):
            print(inner)
        case None:
            print(-999)

def main() -> int32:
    text = "é🎉é"
    print(text.len())
    print(text.byte_len())

    mut values: Vec[int32] = [10, 20, 30, 40]
    print(values[-1])
    values[-2] = 35
    print(values[-2])
    print_int_option(values.get(-4))
    print_int_option(values.get(-5))
    print_int_option(values.set(index=-4, value=11))
    print_int_option(values.remove(-2))
    print(values.swap(first=-1, second=-3))
    print(values.insert(index=-1, value=99))
    end_index: int32 = values.len() as int32
    print(values.insert(index=end_index, value=77))
    for value in values:
        print(value)
    return 0
"#;

    assert_run_and_direct_source_stdout(
        "aurora-string-lengths-negative-vec-indices",
        source,
        "4\n9\n40\n35\n10\n-999\n10\n35\ntrue\ntrue\ntrue\n40\n20\n99\n11\n77\n",
    );
}

#[test]
fn too_negative_vec_index_traps_on_run_and_direct_backends() {
    let source = r#"
def main() -> int32:
    values: Vec[int32] = [10, 20, 30]
    print(values[-4])
    return 0
"#;

    assert_run_and_direct_source_failure_with_timeout(
        "aurora-too-negative-vec-index",
        source,
        std::time::Duration::from_secs(20),
        "",
        "vector index `-4` is out of bounds for length `3`",
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
        "def main() -> int32:\n    print(1 // 0)\n    return 0\n",
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
    assert!(stderr.contains("error[AU4004]: division by zero"));
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
        "def main() -> int32:\n    print(1 // 0)\n    return 0\n",
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
    assert!(stderr.contains("error[AU4004]: division by zero"));
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
        "class Handle:\n    name: String\n\n    def close(mut self):\n        print(\"closing \" + self.name)\n\ndef main() -> int32:\n    with h = Handle(name=\"db\"):\n        print(\"inside with\")\n    print(\"after with\")\n    return 0\n",
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
        "class Handle:\n    name: String\n\n    def close(mut self):\n        print(\"closing \" + self.name)\n\ndef process() -> int32:\n    with h = Handle(name=\"file\"):\n        return 42\n    return 0\n\ndef main() -> int32:\n    print(process())\n    return 0\n",
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
        "trait Show:\n    def show(self) -> String\n\nimpl Show for int32:\n    def show(self) -> String:\n        return \"int\"\n\ndef main() -> int32:\n    value: int32 = 7\n    print(value.show())\n    return 0\n",
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
        "def print_int_option(value: Option[int32]):\n    match value:\n        case Some(inner):\n            print(inner)\n        case None:\n            print(-1)\n\ndef main() -> int32:\n    values = Vec[int32]()\n    print(values.is_empty())\n    mut items: Vec[int32] = [1, 2, 3]\n    print(items.len())\n    print_int_option(items.get(1))\n    print_int_option(items.set(index=1, value=20))\n    print_int_option(items.remove(0))\n    items.push(99)\n    print_int_option(items.pop())\n    mut total: int32 = 0\n    for value in items:\n        total += value\n    print(total)\n    return 0\n",
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
        "def print_int_option(value: Option[int32]):\n    match value:\n        case Some(inner):\n            print(inner)\n        case None:\n            print(-1)\n\ndef main() -> int32:\n    text = \"  aurora repo  \"\n    print(text.len())\n    print(text.contains(\"repo\"))\n    print(text.starts_with(\"  au\"))\n    print(text.ends_with(\"  \"))\n    print(text.trim())\n    print(abs(-7))\n    print(min(9, 2))\n    print(max(4, 12))\n    print(sqrt(81.0))\n    mut counts: Map[String, int32] = {\"aurora\": 1, \"codex\": 2}\n    print(counts.len())\n    print(counts.contains_key(\"aurora\"))\n    print_int_option(counts.get(\"aurora\"))\n    print_int_option(counts.set(key=\"aurora\", value=5))\n    print(counts[\"aurora\"])\n    print(counts.keys().len())\n    print(counts.values().len())\n    print_int_option(counts.remove(\"codex\"))\n    print(counts.is_empty())\n    return 0\n",
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
    def show(self) -> String

class Box[T]:
    value: T

impl Show for Box[int32]:
    def show(self) -> String:
        return f"{self.value}"

impl Show for Box[String]:
    def show(self) -> String:
        return self.value.clone()

def render[T: Show](value: T) -> None:
    print(value.show())

def main() -> int32:
    render(Box[int32](value=7))
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
    def add2(self, rhs: own Rhs) -> Out

class Box[T]:
    value: T

impl Add2[int32, int32] for int32:
    def add2(self, rhs: own int32) -> int32:
        return self + rhs

impl[T: Add2[T, T]] Add2[Box[T], Box[T]] for Box[T]:
    def add2(self, rhs: own Box[T]) -> Box[T]:
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

def serve_udp(socket: own net.UdpSocket) -> Result[String, io.Error]:
    with server_socket = socket:
        match own try server_socket.recv_from(1024, timeout=1s):
            case Option.Some(packet):
                text = try packet.text()
                try server_socket.send_text(packet.address(), "udp:" + text, timeout=1s)
                return Result.Ok(text)
            case Option.None:
                return Result.Ok("missing")

def serve_http(listener: own net.HttpListener) -> Result[None, io.Error]:
    with server_listener = listener:
        exchange = try server_listener.accept(timeout=1s)
        with request = exchange:
            method = request.method()
            path = request.path()
            body = try request.body_text()
            headers = request.headers()
            match own headers.get("X-Test"):
                case Option.Some(test_header):
                    try request.respond_text(200, method + ":" + path + ":" + body + ":" + test_header, {{"Content-Type": "text/plain"}})
                    return Result.Ok(None)
                case Option.None:
                    try request.respond_text(400, "missing X-Test", {{"Content-Type": "text/plain"}})
                    return Result.Ok(None)

def serve_http_bytes(listener: own net.HttpListener) -> Result[None, io.Error]:
    with server_listener = listener:
        exchange = try server_listener.accept(timeout=1s)
        with request = exchange:
            body = request.body_bytes()
            try request.respond_bytes(202, body, {{"Content-Type": "application/octet-stream"}})
            return Result.Ok(None)

def serve_ws(listener: own net.WebSocketListener) -> Result[None, io.Error]:
    with server_listener = listener:
        socket = try server_listener.accept(timeout=1s)
        with server_socket = socket:
            match own try server_socket.recv_text(timeout=1s):
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
            match own try client_socket.recv_from(1024, timeout=1s):
                case Option.Some(packet):
                    print(try packet.text())
                case Option.None:
                    return Result.Ok(None)
        match own udp_task.result():
            case TaskResult.Ready(result):
                match own result:
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
        match own http_task.result():
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
        match own http_bytes_task.result():
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
            match own try ws_client.recv_text(timeout=1s):
                case Option.Some(text):
                    print(text)
                case Option.None:
                    return Result.Ok(None)
        match own ws_task.result():
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
    match own run():
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
    values: Vec[int32] = [1, 2]
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
    values: Vec[int32] = [1, 2]
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
    mut n: int32 = 0
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
    value: String

    def take(own self) -> String:
        return self.value

def main() -> int32:
    b = Box(value="held")
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

    assert_run_and_direct_source_stdout("aurora-value-receiver-binding", source, "held\n");
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

    def close(mut self):
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

    def close(mut self):
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

    def close(mut self):
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

    def close(mut self):
        print("close " + self.name)
        print(1 // 0)

def boom() -> int32:
    print("body")
    return 1 // 0

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
        stderr.contains("return 1 // 0"),
        "direct backend should report the primary body trap, stderr was:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("print(1 // 0)"),
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
    def close(mut self):
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

    def close(mut self):
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
fn retrying_network_worker_runs_with_computed_backoff_on_both_backends() {
    let example = repo_root().join("examples/agents/retrying_network_worker.au");
    let source = fs::read_to_string(&example)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", example.display()));

    let expected = concat!(
        "recover request 1\n",
        "recover retry 4ms\n",
        "recover request 2\n",
        "recover result 200\n",
        "rate request 1\n",
        "rate retry 6ms\n",
        "rate request 2\n",
        "rate result 429\n",
        "exhaust request 1\n",
        "exhaust retry 3ms\n",
        "exhaust request 2\n",
        "exhaust retry 5ms\n",
        "exhaust request 3\n",
        "exhaust result 503\n",
        "requests 7\n",
    );

    assert_run_and_direct_source_stdout_with_timeout(
        "aurora-retrying-network-worker",
        &source,
        std::time::Duration::from_secs(20),
        expected,
    );

    let retry_body = source
        .split_once("def request_with_retry")
        .and_then(|(_, rest)| rest.split_once("\ndef work"))
        .map(|(body, _)| body)
        .expect("example should define request_with_retry before work");

    let marker = |needle: &str| {
        retry_body
            .find(needle)
            .unwrap_or_else(|| panic!("retry worker should contain `{needle}`"))
    };
    let retryable_guard = marker("if status != 503:");
    let final_attempt_guard = marker("if attempt == max_attempts:");
    let jitter = marker("jitter = rng.next_int(0, 4) * 1ms");
    let delay = marker("delay = backoff + jitter");
    let retry_log = marker("print(f\"{name} retry {delay}\")");
    let sleep = marker("sleep(delay)");
    let double = marker("backoff = backoff * 2");

    assert!(
        retryable_guard < final_attempt_guard
            && final_attempt_guard < jitter
            && jitter < delay
            && delay < retry_log
            && retry_log < sleep
            && sleep < double,
        "the final-attempt guard must precede jitter, logging, sleep, and doubling"
    );
    assert_eq!(retry_body.matches("rng.next_int(0, 4)").count(), 1);
    assert_eq!(retry_body.matches("sleep(delay)").count(), 1);
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

def run(cwd: own String) -> Result[None, process.Error]:
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

def serve_unix(listener: own net.UnixListener) -> Result[None, io.Error]:
    with server_listener = listener:
        stream = try server_listener.accept(timeout=1s)
        with server_stream = stream:
            match own try server_stream.read_line(timeout=1s):
                case Option.Some(text):
                    try server_stream.write_all("unix:" + text, timeout=1s)
                    return Result.Ok(None)
                case Option.None:
                    return Result.Ok(None)

def serve_tls(listener: own net.TlsListener) -> Result[None, io.Error]:
    with server_listener = listener:
        stream = try server_listener.accept(timeout=2s)
        with server_stream = stream:
            match own try server_stream.read_line(timeout=2s):
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
            match own try unix_client.read_line(timeout=1s):
                case Option.Some(text):
                    print(text)
                case Option.None:
                    return Result.Ok(None)
        match own unix_task.result():
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
            match own try tls_client.read_line(timeout=2s):
                case Option.Some(text):
                    print(text)
                case Option.None:
                    return Result.Ok(None)
        match own tls_task.result():
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
    match own run():
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

#[test]
fn run_and_direct_backend_match_d6_parameter_loop_and_task_defaults() {
    let source = r#"class Message:
    text: String

def read(message: Message) -> int32:
    return message.text.len() as int32

def consume(value: own String):
    print(value)

def task_read(value: String) -> int32:
    return value.len() as int32

def main() -> int32:
    message = Message(text="shared")
    print(read(message))
    print(message.text)

    names = ["Ada", "Grace"]
    for name in names:
        print(name)
    print(names.len())

    owned = ["moved"]
    for value in own owned:
        consume(value)

    captured = "capture"
    with TaskGroup() as group:
        task = group.start(task_read, captured)
        match task.result():
            case TaskResult.Ready(value):
                print(value)
            case TaskResult.Error(_message):
                print(-1)
            case TaskResult.Cancelled:
                print(-2)
            case TaskResult.TimedOut:
                print(-3)
    return 0
"#;

    assert_run_and_direct_source_stdout(
        "aurora-d6-defaults",
        source,
        "6\nshared\nAda\nGrace\n2\nmoved\n7\n",
    );
}

#[test]
fn check_and_direct_backend_preserve_d6_own_parameter_guidance() {
    let source = r#"def take(value: String) -> String:
    return value
"#;
    let (temp, source_path) = write_temp_source("aurora-d6-own-guidance", source);
    let expected = "parameter `value` is borrowed; declare it as `own String` to take ownership, or clone the value before consuming it";

    let checked = Command::new(aura_bin())
        .arg("check")
        .arg(&source_path)
        .output()
        .expect("failed to run D6 ownership check");
    assert!(
        !checked.status.success(),
        "borrowed parameter move should fail"
    );
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains(expected),
        "unexpected D6 check diagnostic:\n{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let direct = Command::new(aura_bin())
        .args(["build", "--backend", "direct", "-o"])
        .arg(temp.path().join("out"))
        .arg(&source_path)
        .output()
        .expect("failed to run forced-direct D6 ownership check");
    assert!(
        !direct.status.success(),
        "forced direct should reject a borrowed parameter move before code generation"
    );
    assert!(
        String::from_utf8_lossy(&direct.stderr).contains(expected),
        "unexpected forced-direct D6 diagnostic:\n{}",
        String::from_utf8_lossy(&direct.stderr)
    );
}

#[test]
fn check_and_direct_backend_reject_queue_iteration_modifiers() {
    let expected = "Queue iteration receives values; each received item is already owned by the loop binding, and the Queue handle is a copy value, so ownership modifiers have nothing to modify; use the bare form `for item in queue:`";

    for (name, modifier) in [("own", "own "), ("mut", "mut ")] {
        let source = format!(
            "def main() -> int32:\n    queue = Queue[int64]()\n    for item in {modifier}queue:\n        print(item)\n    return 0\n"
        );
        let (temp, source_path) = write_temp_source(&format!("aurora-d6-queue-{name}"), &source);

        let checked = Command::new(aura_bin())
            .arg("check")
            .arg(&source_path)
            .output()
            .expect("failed to check a Queue iteration modifier");
        assert!(
            !checked.status.success(),
            "Queue iteration modifier `{name}` should fail"
        );
        assert!(
            String::from_utf8_lossy(&checked.stderr).contains(expected),
            "unexpected Queue `{name}` diagnostic:\n{}",
            String::from_utf8_lossy(&checked.stderr)
        );

        let direct = Command::new(aura_bin())
            .args(["build", "--backend", "direct", "-o"])
            .arg(temp.path().join("out"))
            .arg(&source_path)
            .output()
            .expect("failed to run forced-direct Queue modifier check");
        assert!(
            !direct.status.success(),
            "forced direct should reject Queue iteration modifier `{name}`"
        );
        assert!(
            String::from_utf8_lossy(&direct.stderr).contains(expected),
            "unexpected forced-direct Queue `{name}` diagnostic:\n{}",
            String::from_utf8_lossy(&direct.stderr)
        );
    }
}

#[test]
fn check_and_direct_backend_reject_range_iteration_modifiers() {
    let expected = "Range iteration yields copy `int32` values, so ownership modifiers have nothing to modify or transfer; use the bare form `for item in range(...):`";

    for (name, modifier) in [("own", "own "), ("mut", "mut ")] {
        let source = format!(
            "def main() -> int32:\n    for item in {modifier}range(0, 3):\n        print(item)\n    return 0\n"
        );
        let (temp, source_path) = write_temp_source(&format!("aurora-d6-range-{name}"), &source);

        let checked = Command::new(aura_bin())
            .arg("check")
            .arg(&source_path)
            .output()
            .expect("failed to check a Range iteration modifier");
        assert!(
            !checked.status.success(),
            "Range iteration modifier `{name}` should fail"
        );
        assert!(
            String::from_utf8_lossy(&checked.stderr).contains(expected),
            "unexpected Range `{name}` diagnostic:\n{}",
            String::from_utf8_lossy(&checked.stderr)
        );

        let direct = Command::new(aura_bin())
            .args(["build", "--backend", "direct", "-o"])
            .arg(temp.path().join("out"))
            .arg(&source_path)
            .output()
            .expect("failed to run forced-direct Range modifier check");
        assert!(
            !direct.status.success(),
            "forced direct should reject Range iteration modifier `{name}`"
        );
        assert!(
            String::from_utf8_lossy(&direct.stderr).contains(expected),
            "unexpected forced-direct Range `{name}` diagnostic:\n{}",
            String::from_utf8_lossy(&direct.stderr)
        );
    }
}

fn run_and_direct_failure_outputs(prefix: &str, source: &str) -> [std::process::Output; 2] {
    let (temp, source_path) = write_temp_source(prefix, source);
    let run = Command::new(aura_bin())
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("failed to run assertion source");
    assert!(
        !run.status.success(),
        "aura run should fail for assertion source"
    );

    let output_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .args(["build", "--backend", "direct", "-o"])
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to build assertion source with the direct backend");
    assert!(
        build.status.success(),
        "direct assertion build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let direct = generated_binary(&output_path)
        .output()
        .expect("failed to run direct assertion binary");
    assert!(
        !direct.status.success(),
        "direct assertion binary should fail"
    );

    [run, direct]
}

#[test]
fn assertions_preserve_exact_messages_in_run_and_direct_backends() {
    for (name, suffix, expected_first_line) in [
        ("default", "", "error[AU4001]: assertion failed"),
        (
            "custom",
            ", \"custom assertion\"",
            "error[AU4001]: custom assertion",
        ),
        ("empty", ", \"\"", "error[AU4001]: "),
        ("whitespace", ", \"   \"", "error[AU4001]:    "),
    ] {
        let source = format!("def main():\n    assert false{suffix}\n");
        for output in
            run_and_direct_failure_outputs(&format!("aurora-assert-message-{name}"), &source)
        {
            assert!(output.stdout.is_empty(), "{name} should not print");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr).lines().next(),
                Some(expected_first_line),
                "{name} assertion message must be preserved exactly"
            );
        }
    }
}

#[test]
fn assertions_evaluate_condition_once_and_message_only_on_failure() {
    let passing = r#"def lazy_message() -> String:
    print("unexpected message")
    return "unused"

def main():
    print("before")
    assert true, lazy_message()
    print("after")
"#;
    assert_run_and_direct_source_stdout(
        "aurora-assert-lazy-passing-message",
        passing,
        "before\nafter\n",
    );

    let failing = r#"class Probe:
    condition_calls: int32
    message_calls: int32

    def condition(mut self) -> bool:
        self.condition_calls += 1
        print(f"condition {self.condition_calls}")
        return false

    def message(mut self) -> String:
        self.message_calls += 1
        print(f"message {self.message_calls}")
        return "evaluated once"

def main():
    mut probe = Probe(condition_calls=0, message_calls=0)
    assert probe.condition(), probe.message()
"#;
    for output in run_and_direct_failure_outputs("aurora-assert-order", failing) {
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "condition 1\nmessage 1\n"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).lines().next(),
            Some("error[AU4001]: evaluated once")
        );
    }
}

#[test]
fn assertion_operand_traps_precede_assertion_failure() {
    let condition_trap = r#"def condition() -> bool:
    print("condition")
    values: Vec[bool] = [true]
    return values[5]

def message() -> String:
    print("message")
    return "assertion should not run"

def main():
    assert condition(), message()
"#;
    for output in run_and_direct_failure_outputs("aurora-assert-condition-trap", condition_trap) {
        assert_eq!(String::from_utf8_lossy(&output.stdout), "condition\n");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("vector index `5` is out of bounds"));
        assert!(!stderr.contains("assertion should not run"));
    }

    let message_trap = r#"def message() -> String:
    print("message")
    values: Vec[int32] = [1]
    print(values[5])
    return "assertion should not run"

def main():
    print("condition")
    assert false, message()
"#;
    for output in run_and_direct_failure_outputs("aurora-assert-message-trap", message_trap) {
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "condition\nmessage\n"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("vector index `5` is out of bounds"));
        assert!(!stderr.contains("assertion failed"));
    }
}

#[test]
fn assertion_failure_remains_primary_when_cleanup_also_traps() {
    let source = r#"class Resource:
    def close(mut self):
        print("close")
        print(1 // 0)

def main():
    with resource = Resource():
        print("body")
        assert false, "body assertion"
"#;

    for output in run_and_direct_failure_outputs("aurora-assert-cleanup-primary", source) {
        assert_eq!(String::from_utf8_lossy(&output.stdout), "body\nclose\n");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).lines().next(),
            Some("error[AU4001]: body assertion")
        );
    }
}

#[test]
fn aura_test_discovers_test_functions_and_keeps_main_files_working() {
    let temp = TempDir::new("aurora-test-discovery");
    let tests = temp.path().join("tests");
    fs::create_dir_all(&tests).expect("test directory should create");

    fs::write(
        tests.join("functions.au"),
        "def test_adds():\n    assert 1 + 1 == 2\n\ndef test_membership():\n    values = [1, 2]\n    assert 2 in values\n\ndef test_reports_failure():\n    assert 1 == 2, \"one is not two\"\n\ndef helper() -> int32:\n    return 1\n",
    )
    .expect("function test source should write");
    fs::write(
        tests.join("legacy.au"),
        "def main() -> int32:\n    print(\"legacy\")\n    return 0\n",
    )
    .expect("legacy test source should write");

    let run = Command::new(aura_bin())
        .current_dir(temp.path())
        .arg("test")
        .output()
        .expect("failed to run aura test");

    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);

    // Each `def test_*()` is its own result, named by file and function.
    assert!(
        stdout.contains("::test_adds"),
        "expected a per-function result, stdout was:\n{stdout}"
    );
    assert!(stdout.contains("::test_membership"), "{stdout}");
    // A file without any `def test_*()` still reports one result for the file.
    assert!(stdout.contains("legacy.au"), "{stdout}");
    assert!(
        !stdout.contains("::helper") && !stderr.contains("::helper"),
        "a non-test function must not be discovered"
    );

    // A failing assertion reports its message and span, not just a count.
    assert!(
        stderr.contains("::test_reports_failure"),
        "expected the failing function to be named, stderr was:\n{stderr}"
    );
    assert!(stderr.contains("one is not two"), "{stderr}");
    assert!(stderr.contains("functions.au:9:5"), "{stderr}");

    assert!(stdout.contains("3 passed; 1 failed"), "{stdout}");
    assert!(!run.status.success(), "a failing test must fail the run");
}

#[test]
fn aura_test_treats_file_level_assertions_as_test_results() {
    let temp = TempDir::new("aurora-file-assert-tests");
    let passing_path = temp.path().join("passing.au");
    fs::write(
        &passing_path,
        "def main():\n    assert true, \"passing assertion\"\n",
    )
    .expect("passing assertion test should write");
    let passing = Command::new(aura_bin())
        .args(["test"])
        .arg(&passing_path)
        .output()
        .expect("failed to run passing file-level assertion test");
    assert!(
        passing.status.success(),
        "passing assertion test should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&passing.stderr)
    );
    assert!(String::from_utf8_lossy(&passing.stdout).contains("1 passed; 0 failed"));

    let failing_path = temp.path().join("failing.au");
    fs::write(
        &failing_path,
        "def main():\n    assert false, \"file-level assertion\"\n",
    )
    .expect("failing assertion test should write");
    let failing = Command::new(aura_bin())
        .args(["test"])
        .arg(&failing_path)
        .output()
        .expect("failed to run failing file-level assertion test");
    assert!(
        !failing.status.success(),
        "failing assertion test should fail"
    );
    assert!(String::from_utf8_lossy(&failing.stdout).contains("0 passed; 1 failed"));
    let stderr = String::from_utf8_lossy(&failing.stderr);
    assert!(stderr.contains("FAILED"));
    assert!(stderr.contains("error[AU4001]: file-level assertion"));
    assert!(stderr.contains("assert false"));
}

#[test]
fn native_cache_format_is_bumped_past_the_capability_migration() {
    // ADR-0022 Q9 requires every Phase-4 artifact built from the old grammar
    // to be invalidated, so the cache format string must have moved past the
    // `v3` that pre-migration builds keyed on.
    let main = include_str!("../src/main.rs");
    assert!(
        main.contains(r#"const NATIVE_CACHE_FORMAT: &str = "aurora-native-cache-v4";"#),
        "native cache format must be v4 so pre-migration artifacts cannot be reused"
    );
    assert!(
        !main.contains("aurora-native-cache-v3"),
        "the retired v3 cache format must not linger in the key material"
    );
}
