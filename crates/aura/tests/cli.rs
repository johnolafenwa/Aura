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
fn run_mir_stdin_resolves_local_module_imports() {
    let temp = TempDir::new("aurora-cli-run-mir-modules-stdin");
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
        .arg("run-mir")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura run-mir");

    child
        .stdin
        .take()
        .expect("stdin should be available")
        .write_all(source.as_bytes())
        .expect("failed to write source");

    let output = child
        .wait_with_output()
        .expect("failed to collect aura run-mir output");

    assert!(
        output.status.success(),
        "run-mir should succeed for module-aware stdin buffers, stderr was:\n{}",
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
fn build_with_direct_backend_ignores_closed_recv_when_timeout_arm_exists() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-select-closed-timeout",
        "def main() -> int32:\n    ch: Channel[int32] = channel()\n    ch.close()\n    select:\n        case value = ch.recv():\n            match value:\n                case Option.Some(v):\n                    print(v)\n                case Option.None:\n                    print(1)\n        case after(1ms):\n            print(2)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend select binary should exit successfully, stderr was:\n{}",
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
fn default_build_supports_generic_constructor_specialization_example() {
    assert_default_backend_example_runs(
        "examples/generics/generic_constructor_specialization.au",
        "generic-specialization-auto",
        "42\n",
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
fn default_build_ignores_closed_recv_when_timeout_arm_exists() {
    let (_, run) = build_and_run_default_source(
        "aurora-build-auto-select-closed-timeout",
        "def main() -> int32:\n    ch: Channel[int32] = channel()\n    ch.close()\n    select:\n        case value = ch.recv():\n            match value:\n                case Option.Some(v):\n                    print(v)\n                case Option.None:\n                    print(1)\n        case after(1ms):\n            print(2)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "default-backend select binary should exit successfully, stderr was:\n{}",
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
fn build_with_direct_backend_supports_task_join_returning_plain_classes() {
    let (_, run) = build_and_run_direct_source(
        "aurora-build-direct-task-join-class",
        "class Box:\n    value: int32\n\ndef make_box() -> Box:\n    return Box(value=7)\n\ndef main() -> int32:\n    task = spawn make_box()\n    box = task.join()\n    print(box.value)\n    return 0\n",
    );

    assert!(
        run.status.success(),
        "direct-backend task join binary should exit successfully, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
}

#[test]
fn build_supports_task_join_returning_plain_classes() {
    let (temp, source_path) = write_temp_source(
        "aurora-build-default-task-join-class",
        "class Box:\n    value: int32\n\ndef make_box() -> Box:\n    return Box(value=7)\n\ndef main() -> int32:\n    task = spawn make_box()\n    box = task.join()\n    print(box.value)\n    return 0\n",
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
        "default build should support task join returning plain classes, stderr was:\n{}",
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
fn run_mir_executes_generic_constructor_specialization_example() {
    let fixture = repo_root().join("examples/generics/generic_constructor_specialization.au");
    let output = Command::new(aura_bin())
        .arg("run-mir")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run-mir on generic constructor specialization example");

    assert!(
        output.status.success(),
        "run-mir should succeed for generic constructor specialization example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
}

#[test]
fn run_mir_executes_generic_trait_impl_example() {
    let fixture = repo_root().join("examples/traits/generic_trait_impl.au");
    let output = Command::new(aura_bin())
        .arg("run-mir")
        .arg(&fixture)
        .output()
        .expect("failed to run aura run-mir on generic trait impl example");

    assert!(
        output.status.success(),
        "run-mir should succeed for generic trait impl example, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "11\n");
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
fn module_qualified_spawn_target_reports_a_user_error_across_commands() {
    let temp = TempDir::new("aurora-cli-qualified-spawn");
    fs::create_dir_all(temp.path().join("pkg")).expect("failed to create module dir");
    fs::write(
        temp.path().join("pkg/helpers.au"),
        "public def work() -> int32:\n    return 1\n",
    )
    .expect("failed to write helper module");
    let source_path = temp.path().join("main.au");
    fs::write(
        &source_path,
        "import pkg.helpers\n\ndef main() -> int32:\n    task = spawn pkg.helpers.work()\n    return task.join()\n",
    )
    .expect("failed to write main module");

    for command in ["check", "run", "run-mir"] {
        let output = Command::new(aura_bin())
            .arg(command)
            .arg(&source_path)
            .output()
            .expect("failed to run aura command");

        assert!(
            !output.status.success(),
            "{} should reject module-qualified spawn targets",
            command
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("`spawn` currently supports named function calls only"),
            "{} should report the spawn target diagnostic, stderr was:\n{}",
            command,
            stderr
        );
        assert!(
            !stderr.contains("panicked at"),
            "{} should not panic, stderr was:\n{}",
            command,
            stderr
        );
    }

    let output_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build");

    assert!(
        !build.status.success(),
        "build should reject module-qualified spawn targets"
    );
    let stderr = String::from_utf8_lossy(&build.stderr);
    assert!(
        stderr.contains("`spawn` currently supports named function calls only"),
        "build should report the spawn target diagnostic, stderr was:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("panicked at"),
        "build should not panic, stderr was:\n{}",
        stderr
    );
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
