use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
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
        json["diagnostics"].as_array().expect("diagnostics should be an array").len(),
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
fn complete_stdin_resolves_local_module_member_completions() {
    let temp = TempDir::new("aurora-cli-complete-modules");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write helper module");
    let main_path = temp.path().join("main.au");
    let source =
        "import helpers.math\n\ndef main() -> int32:\n    helpers.math.\n    return 0\n";

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

    let run = Command::new(&output_path)
        .output()
        .expect("failed to run built output");

    assert!(
        run.status.success(),
        "built binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n");
}

#[test]
fn build_produces_runnable_concurrency_binary() {
    let fixture = repo_root().join("examples/concurrency/channels_spawn.au");
    let output_dir = TempDir::new("aurora-build-concurrency");
    let output_path = output_dir.path().join("channels-spawn");

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

    let run = Command::new(&output_path)
        .output()
        .expect("failed to run built concurrency output");

    assert!(
        run.status.success(),
        "built concurrency binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n4\n");
}

#[test]
fn run_mir_executes_supported_programs() {
    let fixture = repo_root().join("examples/classes/methods.au");
    let output = Command::new(aura_bin())
        .arg("run-mir")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run-mir");

    assert!(
        output.status.success(),
        "run-mir should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "4\n8\n0\n");
}

#[test]
fn run_mir_executes_try_example() {
    let fixture = repo_root().join("examples/error_handling/try_result.au");
    let output = Command::new(aura_bin())
        .arg("run-mir")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run-mir on try example");

    assert!(
        output.status.success(),
        "run-mir should succeed for try example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "6\ndivision by zero\n"
    );
}

#[test]
fn run_mir_executes_with_example() {
    let fixture = repo_root().join("examples/resources/with_resource.au");
    let output = Command::new(aura_bin())
        .arg("run-mir")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run-mir on with example");

    assert!(
        output.status.success(),
        "run-mir should succeed for with example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "demo\nclosed demo\ndone\n"
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

    let run = Command::new(&output_path)
        .output()
        .expect("failed to run built module program");

    assert!(
        run.status.success(),
        "built module binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "10\n");
}
