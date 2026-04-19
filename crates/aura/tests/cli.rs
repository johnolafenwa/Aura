use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use rcgen::generate_simple_self_signed;

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

    let run = Command::new(&output_path)
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

    let run = Command::new(&output_path)
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

    let run = Command::new(&output_path)
        .output()
        .expect("failed to run default-backend binary");

    (build, run)
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
        "from pkg.named import Named\n\npublic class User:\n    public label: String\n\nimpl Named for User:\n    def name(borrow self) -> String:\n        return self.label\n",
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

    let run = Command::new(&output_path)
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
        "def main() -> int32:\n    keys = [\"a\", \"b\"]\n    idx = 1\n    mut counts = {\"key\": 7}\n    print(keys[idx].clone())\n    print(f\"val: {counts[\"key\"]}\")\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct backend indexed-chain/fstring binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "b\nval: 7\n");
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
        "aurora\n",
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
        "Ada\nGrace\ntrue\nfalse\n4\n1\n14\n13\n12\n11\ntrue\n100\ntrue\ntrue\n",
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

    let run = Command::new(&output_path)
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

    let run = Command::new(&output_path)
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
        "aurora\n",
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
        "Ada\nGrace\ntrue\nfalse\n4\n1\n14\n13\n12\n11\ntrue\n100\ntrue\ntrue\n",
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

    let run = Command::new(&output_path)
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
        "class Box:\n    value: int32\n\ndef make_box() -> Box:\n    return Box(value=7)\n\ndef main() -> int32:\n    with TaskGroup() as group:\n        task = group.start(make_box)\n        match task.result():\n            case TaskResult.Ready(box):\n                print(box.value)\n            case TaskResult.TimedOut:\n                print(0)\n            case TaskResult.Cancelled:\n                print(0)\n    return 0\n",
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
        "class Box:\n    value: int32\n\ndef make_box() -> Box:\n    return Box(value=7)\n\ndef main() -> int32:\n    with TaskGroup() as group:\n        task = group.start(make_box)\n        match task.result():\n            case TaskResult.Ready(box):\n                print(box.value)\n            case TaskResult.TimedOut:\n                print(0)\n            case TaskResult.Cancelled:\n                print(0)\n    return 0\n",
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

    let run = Command::new(&output_path)
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

    let run = Command::new(&output_path)
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
    assert_eq!(String::from_utf8_lossy(&output.stdout), "aurora\n");
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
        "Ada\nGrace\ntrue\nfalse\n4\n1\n14\n13\n12\n11\ntrue\n100\ntrue\ntrue\n"
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
        "import pkg.helpers\n\ndef main() -> int32:\n    with TaskGroup() as group:\n        task = group.start(pkg.helpers.work)\n        match task.result():\n            case TaskResult.Ready(value):\n                print(value)\n            case TaskResult.TimedOut:\n                print(0)\n            case TaskResult.Cancelled:\n                print(0)\n    return 0\n",
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

        let run = Command::new(&output_path)
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

    let run = Command::new(&output_path)
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

    let run = Command::new(&output_path)
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
            case TaskResult.Cancelled:
                return Result.Ok(None)
            case TaskResult.TimedOut:
                return Result.Ok(None)

        http_listener = try net.http_listen("127.0.0.1:0")
        http_addr = try http_listener.local_addr()
        http_task = group.start(serve_http, http_listener)
        headers: Map[String, String] = {{"X-Test": "ok"}}
        response = try net.http_request_text_timeout("POST", "http://" + http_addr + "/hello", "body", headers, 1s)
        with http_response = response:
            print(http_response.status())
            print(try http_response.text())
        match http_task.result():
            case TaskResult.Ready(result):
                try result
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
        "4\n65\n67\nudp:ping\nping\n200\nPOST:/hello:body:ok\nws:hi\n"
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
            _payload = try server_stream.read_exact(5, timeout=2s)
            try server_stream.write_all("tls:ping!", timeout=2s)
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
            case TaskResult.Cancelled:
                return Result.Ok(None)
            case TaskResult.TimedOut:
                return Result.Ok(None)

        tls_listener = try net.tls_listen("127.0.0.1:0", "{cert_path}", "{key_path}")
        tls_addr = try tls_listener.local_addr()
        tls_task = group.start(serve_tls, tls_listener)
        stream = try net.tls_connect_timeout(tls_addr, "localhost", "{cert_path}", 2s)
        with tls_client = stream:
            try tls_client.write_all("ping!", timeout=2s)
            reply = try tls_client.read_exact(9, timeout=2s)
            print(reply.len())
        match tls_task.result():
            case TaskResult.Ready(result):
                try result
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
    assert_eq!(String::from_utf8_lossy(&run.stdout), "unix:ping\n9\n");
}
