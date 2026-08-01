use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

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

    fn write(&self, relative: &str, source: &str) -> PathBuf {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dirs");
        }
        fs::write(&path, source).expect("failed to write fixture file");
        path
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

fn copy_dir_recursive(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("failed to create destination directory");
    for entry in fs::read_dir(source).expect("failed to read source directory") {
        let entry = entry.expect("failed to read directory entry");
        let entry_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("failed to read entry file type");
        if file_type.is_dir() {
            copy_dir_recursive(&entry_path, &destination_path);
        } else if file_type.is_file() {
            fs::copy(&entry_path, &destination_path).expect("failed to copy fixture file");
        } else {
            panic!(
                "unsupported maintained package example entry `{}`",
                entry_path.display()
            );
        }
    }
}

struct GitRepo {
    path: PathBuf,
}

impl GitRepo {
    fn init(temp: &TempDir, relative: &str, package_name: &str, files: &[(&str, &str)]) -> Self {
        let path = temp.path().join(relative);
        fs::create_dir_all(path.join("src")).expect("failed to create git package src");
        fs::write(
            path.join("Aura.toml"),
            format!(
                "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
                package_name
            ),
        )
        .expect("failed to write git package manifest");
        for (relative_path, source) in files {
            let file_path = path.join(relative_path);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).expect("failed to create git package parent dirs");
            }
            fs::write(&file_path, source).expect("failed to write git package file");
        }
        let repo = Self { path };
        repo.git(&["init", "-b", "main"]);
        repo.git(&["config", "user.name", "Aura Tests"]);
        repo.git(&["config", "user.email", "aura-tests@example.com"]);
        repo.git(&["add", "."]);
        repo.git(&["commit", "-m", "initial"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("failed to run git command");
        assert!(
            output.status.success(),
            "git {:?} failed in `{}`\nstdout:\n{}\nstderr:\n{}",
            args,
            self.path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git output should be utf8")
            .trim()
            .to_string()
    }

    fn commit_all(&self, message: &str) -> String {
        self.git(&["add", "."]);
        self.git(&["commit", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }
}

fn write_manifest_package_fixture(prefix: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new(prefix);
    temp.write(
        "app/Aura.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { path = "../util" }
"#,
    );
    let main_path = temp.write(
        "app/src/main.au",
        r#"import util.math
import helpers.math

def main() -> int32:
    print(util.math.double(value=helpers.math.triple(value=2)))
    return 0
"#,
    );
    temp.write(
        "app/src/helpers/math.au",
        r#"public def triple(value: int32) -> int32:
    return value * 3
"#,
    );
    temp.write(
        "util/Aura.toml",
        r#"[package]
name = "util"
version = "0.1.0"
edition = "2026"
"#,
    );
    temp.write(
        "util/src/math.au",
        r#"public def double(value: int32) -> int32:
    return value * 2
"#,
    );
    (temp, main_path)
}

#[test]
fn manifest_aware_cli_commands_support_path_dependencies() {
    let (temp, main_path) = write_manifest_package_fixture("aura-cli-packages");

    let check = Command::new(aura_bin())
        .arg("check")
        .arg(&main_path)
        .output()
        .expect("failed to run aura check");
    assert!(
        check.status.success(),
        "check should succeed for manifest-aware packages, stderr was:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&check.stdout), "ok\n");

    for command in ["run"] {
        let output = Command::new(aura_bin())
            .arg(command)
            .arg(&main_path)
            .output()
            .expect("failed to run aura execution command");
        assert!(
            output.status.success(),
            "{} should succeed for manifest-aware packages, stderr was:\n{}",
            command,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "12\n");
    }

    let analysis = Command::new(aura_bin())
        .arg("analyze")
        .arg(&main_path)
        .output()
        .expect("failed to run aura analyze");
    assert!(
        analysis.status.success(),
        "analyze should succeed for manifest-aware packages, stderr was:\n{}",
        String::from_utf8_lossy(&analysis.stderr)
    );
    let analysis_json: serde_json::Value =
        serde_json::from_slice(&analysis.stdout).expect("analyze should return valid JSON");
    assert_eq!(
        analysis_json["diagnostics"]
            .as_array()
            .expect("diagnostics should be an array")
            .len(),
        0,
        "analyze should not report diagnostics for a valid manifest-aware package"
    );

    let build_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&build_path)
        .arg(&main_path)
        .output()
        .expect("failed to run aura build");
    assert!(
        build.status.success(),
        "build should succeed for manifest-aware packages, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let built = generated_binary(&build_path)
        .output()
        .expect("failed to run built package binary");
    assert!(
        built.status.success(),
        "built binary should run successfully, stderr was:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&built.stdout), "12\n");
}

#[test]
fn manifest_aware_stdin_analysis_and_completion_support_path_dependencies() {
    let (_temp, main_path) = write_manifest_package_fixture("aura-cli-packages-stdin");
    let completion_source = [
        "import util.math",
        "import helpers.math",
        "",
        "def main() -> int32:",
        "    util.math.",
        "    return helpers.math.triple(value=2)",
    ]
    .join("\n");

    let mut analyze_child = Command::new(aura_bin())
        .arg("analyze")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura analyze");
    analyze_child
        .stdin
        .take()
        .expect("analyze stdin should exist")
        .write_all(
            [
                "import util.math",
                "import helpers.math",
                "",
                "def main() -> int32:",
                "    print(util.math.double(value=helpers.math.triple(value=2)))",
                "    return 0",
            ]
            .join("\n")
            .as_bytes(),
        )
        .expect("failed to write analyze source");
    let analyze_output = analyze_child
        .wait_with_output()
        .expect("failed to collect analyze output");
    assert!(
        analyze_output.status.success(),
        "stdin analyze should succeed for manifest-aware packages, stderr was:\n{}",
        String::from_utf8_lossy(&analyze_output.stderr)
    );
    let analyze_json: serde_json::Value = serde_json::from_slice(&analyze_output.stdout)
        .expect("stdin analyze should return valid JSON");
    assert_eq!(
        analyze_json["diagnostics"]
            .as_array()
            .expect("diagnostics should be an array")
            .len(),
        0,
        "stdin analyze should not report diagnostics"
    );

    let mut complete_child = Command::new(aura_bin())
        .arg("complete")
        .arg("--line")
        .arg("4")
        .arg("--character")
        .arg("14")
        .arg("--trigger")
        .arg(".")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura complete");
    complete_child
        .stdin
        .take()
        .expect("complete stdin should exist")
        .write_all(completion_source.as_bytes())
        .expect("failed to write completion source");
    let complete_output = complete_child
        .wait_with_output()
        .expect("failed to collect complete output");
    assert!(
        complete_output.status.success(),
        "stdin complete should succeed for manifest-aware packages, stderr was:\n{}",
        String::from_utf8_lossy(&complete_output.stderr)
    );
    let completions: serde_json::Value =
        serde_json::from_slice(&complete_output.stdout).expect("complete should return valid JSON");
    assert!(
        completions
            .as_array()
            .expect("completions should be an array")
            .iter()
            .any(|entry| entry["name"] == "double"),
        "dependency module completions should include exported members"
    );
}

#[test]
fn maintained_package_examples_run_through_cli_commands() {
    let repo = repo_root();
    let temp = TempDir::new("aura-cli-maintained-package-examples");
    let local_path_dependencies = temp.path().join("local_path_dependencies");
    let workspace = temp.path().join("workspace");
    copy_dir_recursive(
        &repo.join("examples/packages/local_path_dependencies"),
        &local_path_dependencies,
    );
    copy_dir_recursive(&repo.join("examples/packages/workspace"), &workspace);

    let package_examples = [
        (local_path_dependencies.join("app/src/main.au"), "12\n"),
        (workspace.join("app/src/main.au"), "8\n"),
    ];

    for (main_path, expected_stdout) in package_examples {
        for command in ["check", "run"] {
            let output = Command::new(aura_bin())
                .arg(command)
                .arg(&main_path)
                .output()
                .expect("failed to run package example command");
            assert!(
                output.status.success(),
                "{} should succeed for maintained package example `{}`, stderr was:\n{}",
                command,
                main_path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
            if command == "check" {
                assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n");
            } else {
                assert_eq!(String::from_utf8_lossy(&output.stdout), expected_stdout);
            }
        }

        let output_dir = TempDir::new("aura-cli-package-examples");
        let output_path = output_dir.path().join("out");
        let build = Command::new(aura_bin())
            .arg("build")
            .arg("-o")
            .arg(&output_path)
            .arg(&main_path)
            .output()
            .expect("failed to build maintained package example");
        assert!(
            build.status.success(),
            "build should succeed for maintained package example `{}`, stderr was:\n{}",
            main_path.display(),
            String::from_utf8_lossy(&build.stderr)
        );
        let built = generated_binary(&output_path)
            .output()
            .expect("failed to run built package example");
        assert!(
            built.status.success(),
            "built maintained package example should run successfully, stderr was:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&built.stdout), expected_stdout);
    }
}

#[test]
fn manifest_aware_cli_commands_support_git_dependencies() {
    let temp = TempDir::new("aura-cli-packages-git");
    let repo = GitRepo::init(
        &temp,
        "util-repo",
        "util",
        &[(
            "src/math.au",
            r#"public def double(value: int32) -> int32:
    return value * 2
"#,
        )],
    );
    let resolved_rev = repo.git(&["rev-parse", "HEAD"]);

    temp.write(
        "app/Aura.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { git = "../util-repo" }
"#,
    );
    let main_path = temp.write(
        "app/src/main.au",
        r#"import util.math

def main() -> int32:
    print(util.math.double(value=3))
    return 0
"#,
    );

    let check = Command::new(aura_bin())
        .arg("check")
        .arg(&main_path)
        .output()
        .expect("failed to run aura check");
    assert!(
        check.status.success(),
        "check should succeed for manifest-aware git packages, stderr was:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&check.stdout), "ok\n");

    for command in ["run"] {
        let output = Command::new(aura_bin())
            .arg(command)
            .arg(&main_path)
            .output()
            .expect("failed to run aura execution command");
        assert!(
            output.status.success(),
            "{} should succeed for manifest-aware git packages, stderr was:\n{}",
            command,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "6\n");
    }

    let analysis = Command::new(aura_bin())
        .arg("analyze")
        .arg(&main_path)
        .output()
        .expect("failed to run aura analyze");
    assert!(
        analysis.status.success(),
        "analyze should succeed for manifest-aware git packages, stderr was:\n{}",
        String::from_utf8_lossy(&analysis.stderr)
    );
    let analysis_json: serde_json::Value =
        serde_json::from_slice(&analysis.stdout).expect("analyze should return valid JSON");
    assert_eq!(
        analysis_json["diagnostics"]
            .as_array()
            .expect("diagnostics should be an array")
            .len(),
        0
    );

    let build_path = temp.path().join("out");
    let build = Command::new(aura_bin())
        .arg("build")
        .arg("-o")
        .arg(&build_path)
        .arg(&main_path)
        .output()
        .expect("failed to run aura build");
    assert!(
        build.status.success(),
        "build should succeed for manifest-aware git packages, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let built = generated_binary(&build_path)
        .output()
        .expect("failed to run built package binary");
    assert!(built.status.success());
    assert_eq!(String::from_utf8_lossy(&built.stdout), "6\n");

    let lockfile =
        fs::read_to_string(temp.path().join("app/Aura.lock")).expect("lockfile should exist");
    assert!(lockfile.contains("source = \"git\""));
    assert!(lockfile.contains("branch = \"main\""));
    assert!(lockfile.contains(&format!("rev = \"{}\"", resolved_rev)));
}

#[test]
fn manifest_aware_stdin_analysis_and_completion_support_git_dependencies() {
    let temp = TempDir::new("aura-cli-packages-git-stdin");
    GitRepo::init(
        &temp,
        "util-repo",
        "util",
        &[(
            "src/math.au",
            r#"public def double(value: int32) -> int32:
    return value * 2
"#,
        )],
    );
    temp.write(
        "app/Aura.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { git = "../util-repo" }
"#,
    );
    let main_path = temp.write(
        "app/src/main.au",
        r#"import util.math

def main() -> int32:
    print(util.math.double(value=3))
    return 0
"#,
    );

    let mut analyze_child = Command::new(aura_bin())
        .arg("analyze")
        .arg("--stdin")
        .arg(&main_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura analyze");
    analyze_child
        .stdin
        .take()
        .expect("analyze stdin should exist")
        .write_all(
            [
                "import util.math",
                "",
                "def main() -> int32:",
                "    print(util.math.double(value=3))",
                "    return 0",
            ]
            .join("\n")
            .as_bytes(),
        )
        .expect("failed to write analyze source");
    let analyze_output = analyze_child
        .wait_with_output()
        .expect("failed to collect analyze output");
    assert!(analyze_output.status.success());

    let completion_source = [
        "import util.math",
        "",
        "def main() -> int32:",
        "    util.math.",
        "    return 0",
    ]
    .join("\n");
    let mut complete_child = Command::new(aura_bin())
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
        .spawn()
        .expect("failed to spawn aura complete");
    complete_child
        .stdin
        .take()
        .expect("complete stdin should exist")
        .write_all(completion_source.as_bytes())
        .expect("failed to write completion source");
    let complete_output = complete_child
        .wait_with_output()
        .expect("failed to collect complete output");
    assert!(complete_output.status.success());
    let completions: serde_json::Value =
        serde_json::from_slice(&complete_output.stdout).expect("complete should return valid JSON");
    assert!(completions
        .as_array()
        .expect("completions should be an array")
        .iter()
        .any(|entry| entry["name"] == "double"));
}

#[test]
fn deps_update_refreshes_a_specific_git_dependency_only() {
    let temp = TempDir::new("aura-cli-deps-update-one");
    let util_repo = GitRepo::init(
        &temp,
        "util-repo",
        "util",
        &[(
            "src/math.au",
            r#"public def value() -> int32:
    return 2
"#,
        )],
    );
    let jsonx_repo = GitRepo::init(
        &temp,
        "jsonx-repo",
        "jsonx",
        &[(
            "src/parse.au",
            r#"public def value() -> int32:
    return 10
"#,
        )],
    );

    temp.write(
        "app/Aura.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { git = "../util-repo", branch = "main" }
jsonx = { git = "../jsonx-repo", branch = "main" }
"#,
    );
    let main_path = temp.write(
        "app/src/main.au",
        r#"import util.math
import jsonx.parse

def main() -> int32:
    print(util.math.value() + jsonx.parse.value())
    return 0
"#,
    );

    let initial = Command::new(aura_bin())
        .arg("run")
        .arg(&main_path)
        .output()
        .expect("failed to run initial package");
    assert!(initial.status.success());
    assert_eq!(String::from_utf8_lossy(&initial.stdout), "12\n");

    temp.write(
        "util-repo/src/math.au",
        r#"public def value() -> int32:
    return 4
"#,
    );
    let new_util_rev = util_repo.commit_all("advance util");
    temp.write(
        "jsonx-repo/src/parse.au",
        r#"public def value() -> int32:
    return 20
"#,
    );
    let new_jsonx_rev = jsonx_repo.commit_all("advance jsonx");

    let update = Command::new(aura_bin())
        .arg("deps")
        .arg("update")
        .arg("util")
        .current_dir(temp.path().join("app"))
        .output()
        .expect("failed to run deps update util");
    assert!(
        update.status.success(),
        "deps update util should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&update.stderr)
    );
    assert!(
        String::from_utf8_lossy(&update.stdout).contains("updated util"),
        "specific package update should report the refreshed package"
    );

    let after_update = Command::new(aura_bin())
        .arg("run")
        .arg("src/main.au")
        .current_dir(temp.path().join("app"))
        .output()
        .expect("failed to run updated package");
    assert!(after_update.status.success());
    assert_eq!(String::from_utf8_lossy(&after_update.stdout), "14\n");

    let lockfile =
        fs::read_to_string(temp.path().join("app/Aura.lock")).expect("lockfile should exist");
    assert!(lockfile.contains(&new_util_rev));
    assert!(
        !lockfile.contains(&new_jsonx_rev),
        "targeted update should not refresh unrelated git dependencies"
    );
}

#[test]
fn deps_update_preserves_the_compiler_diagnostic_code() {
    let temp = TempDir::new("aura-cli-deps-update-diagnostic-code");

    let output = Command::new(aura_bin())
        .arg("deps")
        .arg("update")
        .current_dir(temp.path())
        .output()
        .expect("failed to run deps update outside a package");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error[AU2999]: could not find an enclosing Aura package"),
        "deps update must retain the compiler-owned diagnostic code, stderr was:\n{stderr}"
    );
    assert!(stderr.contains(&temp.path().display().to_string()));
}

#[test]
fn deps_update_refreshes_all_git_dependencies_in_the_current_package() {
    let temp = TempDir::new("aura-cli-deps-update-all");
    let util_repo = GitRepo::init(
        &temp,
        "util-repo",
        "util",
        &[(
            "src/math.au",
            r#"public def value() -> int32:
    return 2
"#,
        )],
    );
    let jsonx_repo = GitRepo::init(
        &temp,
        "jsonx-repo",
        "jsonx",
        &[(
            "src/parse.au",
            r#"public def value() -> int32:
    return 10
"#,
        )],
    );

    temp.write(
        "app/Aura.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { git = "../util-repo", branch = "main" }
jsonx = { git = "../jsonx-repo", branch = "main" }
"#,
    );
    let main_path = temp.write(
        "app/src/main.au",
        r#"import util.math
import jsonx.parse

def main() -> int32:
    print(util.math.value() + jsonx.parse.value())
    return 0
"#,
    );

    let initial = Command::new(aura_bin())
        .arg("run")
        .arg(&main_path)
        .output()
        .expect("failed to run initial package");
    assert!(initial.status.success());
    assert_eq!(String::from_utf8_lossy(&initial.stdout), "12\n");

    temp.write(
        "util-repo/src/math.au",
        r#"public def value() -> int32:
    return 4
"#,
    );
    let new_util_rev = util_repo.commit_all("advance util");
    temp.write(
        "jsonx-repo/src/parse.au",
        r#"public def value() -> int32:
    return 20
"#,
    );
    let new_jsonx_rev = jsonx_repo.commit_all("advance jsonx");

    let update = Command::new(aura_bin())
        .arg("deps")
        .arg("update")
        .current_dir(temp.path().join("app"))
        .output()
        .expect("failed to run deps update");
    assert!(
        update.status.success(),
        "deps update should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&update.stderr)
    );
    let stdout = String::from_utf8_lossy(&update.stdout);
    assert!(stdout.contains("updated util"));
    assert!(stdout.contains("updated jsonx"));

    let after_update = Command::new(aura_bin())
        .arg("run")
        .arg("src/main.au")
        .current_dir(temp.path().join("app"))
        .output()
        .expect("failed to run package after deps update");
    assert!(after_update.status.success());
    assert_eq!(String::from_utf8_lossy(&after_update.stdout), "24\n");

    let lockfile =
        fs::read_to_string(temp.path().join("app/Aura.lock")).expect("lockfile should exist");
    assert!(lockfile.contains(&new_util_rev));
    assert!(lockfile.contains(&new_jsonx_rev));
}
