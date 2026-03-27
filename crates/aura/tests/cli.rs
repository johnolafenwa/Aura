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

    let run = Command::new(&output_path)
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

    let run = Command::new(&output_path)
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

    let run = Command::new(&output_path)
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
        "examples/concurrency/channels_spawn.au",
        "channels-direct",
        "2\n4\n",
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
fn build_with_direct_backend_supports_mutating_methods_example() {
    assert_direct_backend_example_runs(
        "examples/classes/mutating_methods.au",
        "mutating-methods-direct",
        "6\n1\n",
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
fn build_with_direct_backend_supports_full_range_uint128_example() {
    assert_direct_backend_example_runs(
        "examples/numbers/uint128_values.au",
        "uint128-direct",
        "340282366920938463463374607431768211455\n340282366920938463463374607431768211455\n",
    );
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

    let run = Command::new(&output_path)
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

    let run = Command::new(&output_path)
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

    let mut child = Command::new(&output_path)
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
