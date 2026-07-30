use super::*;
use std::fs;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        ));
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
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn expect_diag<T>(result: Result<T>, context: &str) -> Diagnostic {
    match result {
        Ok(_) => panic!("{}", context),
        Err(error) => error,
    }
}

fn write_package(temp: &TempDir, dir: &str, name: &str, dependencies: &str) -> PathBuf {
    temp.write(
        &format!("{}/Aurora.toml", dir),
        &format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n{}",
            name, dependencies
        ),
    );
    temp.write(&format!("{}/src/main.au", dir), "def main():\n    pass\n");
    temp.path.join(dir)
}

fn canonical_manifest_path(temp: &TempDir, package_dir: &str) -> PathBuf {
    fs::canonicalize(temp.path.join(package_dir).join(MANIFEST_NAME))
        .expect("fixture manifest should canonicalize")
}

#[test]
fn ffi_manifest_opt_in_defaults_off_and_allows_direct_root_use_when_enabled() {
    let temp = TempDir::new("aurora-packages-ffi-root-opt-in");
    let main_path = temp.write("app/src/main.au", "def main():\n    pass\n");
    temp.write(
        "app/Aurora.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );

    let graph = PackageGraph::discover_for_entry(&main_path)
        .expect("legacy manifests should remain valid")
        .expect("package graph should exist");
    let denied = graph
        .ensure_ffi_allowed_for_path(&main_path)
        .expect_err("FFI must require an explicit package opt-in");
    assert_eq!(denied.code, "AU2999");
    assert_eq!(
        denied.message,
        format!(
            "package `app` uses FFI but its manifest `{}` does not opt in; add `allow_ffi = true` to `[package]`",
            canonical_manifest_path(&temp, "app").display()
        )
    );

    temp.write(
        "app/Aurora.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    );
    let graph = PackageGraph::discover_for_entry(&main_path)
        .expect("an opted-in root package should resolve")
        .expect("package graph should exist");
    graph
        .ensure_ffi_allowed_for_path(&main_path)
        .expect("the opted-in root package should be allowed to use FFI");
}

#[test]
fn extern_declarations_enforce_manifest_authorization_during_module_loading() {
    let temp = TempDir::new("aurora-packages-ffi-module-authorization");
    let standalone = temp.write(
        "standalone.au",
        "public extern \"C\" opaque class Handle\npublic extern \"C\" def acquire() -> Handle\n\ndef main():\n    pass\n",
    );
    let standalone_error =
        crate::check_path(&standalone).expect_err("standalone files must not declare FFI");
    assert_eq!(standalone_error.code, "AU2999");
    assert_eq!(
        standalone_error.message,
        format!(
            "FFI declarations in `{}` require an Aurora package manifest; add `Aurora.toml` with `[package] allow_ffi = true`",
            fs::canonicalize(&standalone)
                .expect("standalone fixture should canonicalize")
                .display()
        )
    );

    let package_main = temp.write(
        "app/src/main.au",
        "public extern \"C\" opaque class Handle\npublic extern \"C\" def acquire() -> Handle\n\ndef main():\n    pass\n",
    );
    temp.write(
        "app/Aurora.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    let package_error =
        crate::check_path(&package_main).expect_err("a package must opt in before declaring FFI");
    assert_eq!(package_error.code, "AU2999");
    assert_eq!(
        package_error.message,
        format!(
            "package `app` uses FFI but its manifest `{}` does not opt in; add `allow_ffi = true` to `[package]`",
            canonical_manifest_path(&temp, "app").display()
        )
    );

    temp.write(
        "app/Aurora.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    );
    crate::check_path(&package_main).expect("an opted-in package should be allowed to declare FFI");
}

#[test]
fn ffi_authorization_rejects_paths_outside_the_discovered_package_graph() {
    let temp = TempDir::new("aurora-packages-ffi-unknown-source");
    let main_path = temp.write("app/src/main.au", "def main():\n    pass\n");
    temp.write(
        "app/Aurora.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    );
    let outside_path = temp.write("outside.au", "public extern \"C\" def getpid() -> int32\n");

    let graph = PackageGraph::discover_for_entry(&main_path)
        .expect("the opted-in package should resolve")
        .expect("package graph should exist");
    let diagnostic = graph
        .ensure_ffi_allowed_for_path(&outside_path)
        .expect_err("an opt-in cannot authorize source outside its package graph");
    assert_eq!(diagnostic.code, "AU2999");
    assert_eq!(
        diagnostic.message,
        format!(
            "could not determine the Aurora package for FFI declaration in `{}`",
            fs::canonicalize(&outside_path)
                .expect("outside fixture should canonicalize")
                .display()
        )
    );
}

#[test]
fn ffi_dependency_report_accepts_listed_direct_and_transitive_dependencies() {
    let temp = TempDir::new("aurora-packages-ffi-dependency-report");
    let main_path = temp.write(
        "app/src/main.au",
        "import util.bridge\n\ndef main():\n    pass\n",
    );
    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[dependencies]
util = { path = "../util" }

[ffi]
dependencies = ["native"]
"#,
    );
    temp.write(
        "util/src/bridge.au",
        "import native.sys\n\npublic def ready() -> bool:\n    return true\n",
    );
    temp.write(
        "util/Aurora.toml",
        r#"[package]
name = "util"
version = "0.1.0"
edition = "2026"

[dependencies]
native = { path = "../native" }
"#,
    );
    let native_path = temp.write(
        "native/src/sys.au",
        "public extern \"C\" def getpid() -> int32\n",
    );
    temp.write(
        "native/Aurora.toml",
        "[package]\nname = \"native\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    );

    let graph = PackageGraph::discover_for_entry(&main_path)
        .expect("a complete transitive FFI report should resolve")
        .expect("package graph should exist");
    graph
        .ensure_ffi_allowed_for_path(&native_path)
        .expect("the reported, opted-in transitive dependency should use FFI");
    crate::check_path(&main_path)
        .expect("module loading should authorize the reported transitive FFI declaration");

    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[dependencies]
native = { path = "../native" }

[ffi]
dependencies = ["native"]
"#,
    );
    PackageGraph::discover_for_entry(&main_path)
        .expect("a complete direct FFI report should resolve")
        .expect("package graph should exist");
}

#[test]
fn ffi_dependency_report_names_the_missing_transitive_dependency_path() {
    let temp = TempDir::new("aurora-packages-ffi-missing-report");
    let main_path = temp.write("app/src/main.au", "def main():\n    pass\n");
    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[dependencies]
util = { path = "../util" }
"#,
    );
    temp.write("util/src/main.au", "def main():\n    pass\n");
    temp.write(
        "util/Aurora.toml",
        r#"[package]
name = "util"
version = "0.1.0"
edition = "2026"

[dependencies]
native = { path = "../native" }
"#,
    );
    temp.write("native/src/main.au", "def main():\n    pass\n");
    temp.write(
        "native/Aurora.toml",
        "[package]\nname = \"native\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    );

    let error = PackageGraph::discover_for_entry(&main_path)
        .expect_err("an unreported FFI dependency must be rejected");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        format!(
            "dependency package `native` enables FFI but is missing from root package `app`'s `[ffi] dependencies` report; add `\"native\"` to `[ffi] dependencies` in `{}` (dependency path: app -> util -> native)",
            canonical_manifest_path(&temp, "app").display()
        )
    );
}

#[test]
fn ffi_dependency_report_requires_the_root_package_opt_in_too() {
    let temp = TempDir::new("aurora-packages-ffi-root-dependency-opt-in");
    let main_path = temp.write("app/src/main.au", "def main():\n    pass\n");
    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
native = { path = "../native" }

[ffi]
dependencies = ["native"]
"#,
    );
    temp.write("native/src/main.au", "def main():\n    pass\n");
    temp.write(
        "native/Aurora.toml",
        "[package]\nname = \"native\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    );

    let error = PackageGraph::discover_for_entry(&main_path)
        .expect_err("a root package must opt in to dependency FFI");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        format!(
            "root package `app` includes FFI-enabled dependency `native` but does not opt in; add `allow_ffi = true` to `[package]` in `{}` and keep `\"native\"` listed in `[ffi] dependencies` (dependency path: app -> native)",
            canonical_manifest_path(&temp, "app").display()
        )
    );
}

#[test]
fn ffi_dependency_report_teaches_unopted_root_to_add_the_missing_report_entry() {
    let temp = TempDir::new("aurora-packages-ffi-root-missing-report");
    let main_path = temp.write("app/src/main.au", "def main():\n    pass\n");
    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
native = { path = "../native" }
"#,
    );
    temp.write("native/src/main.au", "def main():\n    pass\n");
    temp.write(
        "native/Aurora.toml",
        "[package]\nname = \"native\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    );

    let error = PackageGraph::discover_for_entry(&main_path)
        .expect_err("root opt-in and dependency report are both required");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        format!(
            "root package `app` includes FFI-enabled dependency `native` but does not opt in; add `allow_ffi = true` to `[package]` in `{}` and also add `\"native\"` to `[ffi] dependencies` (dependency path: app -> native)",
            canonical_manifest_path(&temp, "app").display()
        )
    );
}

#[test]
fn ffi_dependency_missing_own_opt_in_names_its_dependency_path() {
    let temp = TempDir::new("aurora-packages-ffi-dependency-own-opt-in");
    let main_path = temp.write(
        "app/src/main.au",
        "import native.sys\n\ndef main():\n    pass\n",
    );
    let native_path = temp.write(
        "native/src/sys.au",
        "public extern \"C\" def getpid() -> int32\n",
    );
    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[dependencies]
native = { path = "../native" }
"#,
    );
    temp.write(
        "native/Aurora.toml",
        "[package]\nname = \"native\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );

    let graph = PackageGraph::discover_for_entry(&main_path)
        .expect("a dependency without FFI should not need a report entry")
        .expect("package graph should exist");
    let error = graph
        .ensure_ffi_allowed_for_path(&native_path)
        .expect_err("a dependency that declares FFI must opt in itself");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        format!(
            "dependency package `native` uses FFI but its manifest `{}` does not opt in; add `allow_ffi = true` to `[package]` (dependency path: app -> native)",
            canonical_manifest_path(&temp, "native").display()
        )
    );
    let module_error = crate::check_path(&main_path)
        .expect_err("module loading must enforce the dependency's own FFI opt-in");
    assert_eq!(module_error.code, error.code);
    assert_eq!(module_error.message, error.message);
}

#[test]
fn ffi_dependency_report_rejects_unknown_and_non_ffi_entries() {
    let temp = TempDir::new("aurora-packages-ffi-stale-report");
    let main_path = temp.write("app/src/main.au", "def main():\n    pass\n");
    temp.write("util/src/main.au", "def main():\n    pass\n");
    temp.write(
        "util/Aurora.toml",
        "[package]\nname = \"util\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[dependencies]
util = { path = "../util" }

[ffi]
dependencies = ["missing"]
"#,
    );
    let unknown = PackageGraph::discover_for_entry(&main_path)
        .expect_err("unknown report entries must be rejected");
    assert_eq!(unknown.code, "AU2999");
    assert_eq!(
        unknown.message,
        format!(
            "root package `app` lists `missing` in `[ffi] dependencies`, but no such dependency is reachable from `app`; remove the stale entry or add the dependency in `{}`",
            canonical_manifest_path(&temp, "app").display()
        )
    );

    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[dependencies]
util = { path = "../util" }

[ffi]
dependencies = ["util"]
"#,
    );
    let non_ffi = PackageGraph::discover_for_entry(&main_path)
        .expect_err("non-FFI report entries must be rejected");
    assert_eq!(non_ffi.code, "AU2999");
    assert_eq!(
        non_ffi.message,
        format!(
            "root package `app` lists `util` in `[ffi] dependencies`, but dependency `util` does not enable FFI; remove the stale entry or add `allow_ffi = true` to `[package]` in `{}` (dependency path: app -> util)",
            canonical_manifest_path(&temp, "util").display()
        )
    );
}

#[test]
fn ffi_dependency_report_rejects_duplicate_invalid_and_self_entries() {
    let temp = TempDir::new("aurora-packages-ffi-report-entry-validation");
    let main_path = temp.write("app/src/main.au", "def main():\n    pass\n");
    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[ffi]
dependencies = ["native", "native"]
"#,
    );
    let duplicate = PackageGraph::discover_for_entry(&main_path)
        .expect_err("duplicate FFI report entries must be rejected");
    assert_eq!(
        duplicate.message,
        format!(
            "manifest `{}` lists dependency `native` more than once in `[ffi] dependencies`",
            canonical_manifest_path(&temp, "app").display()
        )
    );

    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[ffi]
dependencies = ["not-a-package"]
"#,
    );
    let invalid = PackageGraph::discover_for_entry(&main_path)
        .expect_err("invalid FFI report package names must be rejected");
    assert_eq!(
        invalid.message,
        format!(
            "manifest `{}` has invalid FFI dependency report entry `not-a-package`; entries in `[ffi] dependencies` must be package names matching `[A-Za-z_][A-Za-z0-9_]*`",
            canonical_manifest_path(&temp, "app").display()
        )
    );

    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[ffi]
dependencies = ["app"]
"#,
    );
    let self_entry = PackageGraph::discover_for_entry(&main_path)
        .expect_err("the root package must not report itself as a dependency");
    assert_eq!(
        self_entry.message,
        format!(
            "root package `app` lists itself in `[ffi] dependencies`; remove the stale entry from `{}` because the root package's own FFI use is authorized by `[package] allow_ffi = true`",
            canonical_manifest_path(&temp, "app").display()
        )
    );
}

struct EnvVarGuard {
    name: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, original }
    }

    fn remove(name: &'static str) -> Self {
        let original = std::env::var_os(name);
        std::env::remove_var(name);
        Self { name, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

fn run_git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("failed to run git command");
    assert!(
        output.status.success(),
        "git {:?} failed in `{}`\nstdout:\n{}\nstderr:\n{}",
        args,
        repo.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output should be utf8")
        .trim()
        .to_string()
}

fn init_git_package_repo(temp: &TempDir, relative: &str, package_name: &str) -> PathBuf {
    let repo = temp.path.join(relative);
    fs::create_dir_all(repo.join("src")).expect("failed to create package repo");
    fs::write(
        repo.join(MANIFEST_NAME),
        format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
            package_name
        ),
    )
    .expect("failed to write manifest");
    fs::write(
        repo.join("src/lib.au"),
        "public def value() -> int32:\n    return 1\n",
    )
    .expect("failed to write source");
    run_git(&repo, &["init", "-b", "main"]);
    run_git(&repo, &["config", "user.name", "Aurora Tests"]);
    run_git(&repo, &["config", "user.email", "aurora-tests@example.com"]);
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "initial"]);
    repo
}

#[test]
fn configured_git_command_disables_interactive_prompts() {
    let args = vec!["status".to_string()];
    let command = configured_git_command(None, &args);
    let envs = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().to_string(),
                value.map(|value| value.to_string_lossy().to_string()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        envs.get("GIT_TERMINAL_PROMPT"),
        Some(&Some("0".to_string()))
    );
    assert_eq!(envs.get("GIT_ASKPASS"), Some(&Some(String::new())));
    assert_eq!(envs.get("SSH_ASKPASS"), Some(&Some(String::new())));
}

#[cfg(unix)]
#[test]
fn command_timeout_terminates_hung_git_helpers() {
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 5"]);
    let started = Instant::now();
    let error = run_command_with_timeout(command, "git test-timeout", StdDuration::from_millis(50))
        .expect_err("hung commands should time out");
    assert!(error.message.contains("timed out"));
    assert!(
        started.elapsed() < StdDuration::from_secs(2),
        "timeout helper should not wait for the child sleep to finish"
    );
}

#[cfg(unix)]
#[test]
fn reject_symlinks_in_tree_reports_symlinked_entries() {
    let temp = TempDir::new("aurora-package-symlink-tree");
    let root = temp.path.join("checkout");
    fs::create_dir_all(root.join("src")).expect("failed to create checkout root");
    fs::write(root.join("Aurora.toml"), "[package]\nname = \"pkg\"\n").expect("manifest");
    std::os::unix::fs::symlink("/tmp", root.join("src").join("escape"))
        .expect("failed to create symlink");

    let error = reject_symlinks_in_tree(&root).expect_err("symlinked content should fail");
    assert!(error.message.contains("contains symlinked content"));
}

#[cfg(unix)]
#[test]
fn read_cached_git_revision_rejects_symlinked_markers() {
    let temp = TempDir::new("aurora-package-symlink-marker");
    let root = temp.path.join("checkout");
    fs::create_dir_all(&root).expect("failed to create checkout root");
    fs::write(root.join("Aurora.toml"), "[package]\nname = \"pkg\"\n").expect("manifest");
    let target = temp.path.join("outside.rev");
    fs::write(&target, "1234567").expect("failed to write outside marker");
    std::os::unix::fs::symlink(&target, root.join(".aurora-cache-rev"))
        .expect("failed to create symlinked marker");

    let error = read_cached_git_revision(&root).expect_err("symlinked revision marker should fail");
    assert!(error
        .message
        .contains("failed to inspect git checkout marker"));
}

#[test]
fn package_git_selector_and_refresh_helpers_cover_validation_edges() {
    let multiple = GitSelector::from_manifest(
        "util",
        Some("abcdef0".to_string()),
        Some("v1".to_string()),
        None,
    )
    .expect_err("multiple selectors should fail");
    assert!(multiple.message.contains("at most one git selector"));

    let empty_rev = GitSelector::from_manifest("util", Some(String::new()), None, None)
        .expect_err("empty revisions should fail");
    assert!(empty_rev.message.contains("empty git revision"));
    let empty_tag = GitSelector::from_manifest("util", None, Some(String::new()), None)
        .expect_err("empty tags should fail");
    assert!(empty_tag.message.contains("empty git tag"));
    let empty_branch = GitSelector::from_manifest("util", None, None, Some(String::new()))
        .expect_err("empty branches should fail");
    assert!(empty_branch.message.contains("empty git branch"));

    let invalid_rev = GitSelector::from_manifest("util", Some("not-a-rev".to_string()), None, None)
        .expect_err("non-hex revisions should fail");
    assert!(invalid_rev.message.contains("invalid git revision"));

    let rev = GitSelector::from_manifest("util", Some("abcdef0".to_string()), None, None)
        .expect("valid revision selector should load");
    let tag = GitSelector::from_manifest("util", None, Some("v1.0.0".to_string()), None)
        .expect("valid tag selector should load");
    let branch = GitSelector::from_manifest("util", None, None, Some("release".to_string()))
        .expect("valid branch selector should load");
    let default_branch =
        GitSelector::from_manifest("util", None, None, None).expect("default branch selector");
    assert!(matches!(rev, GitSelector::Rev(_)));
    assert!(matches!(tag, GitSelector::Tag(_)));
    assert!(matches!(branch, GitSelector::Branch(_)));
    assert!(matches!(default_branch, GitSelector::Branch(ref name) if name == "main"));

    let mut fields = String::new();
    rev.write_lockfile_fields(&mut fields);
    assert_eq!(fields, "");
    tag.write_lockfile_fields(&mut fields);
    branch.write_lockfile_fields(&mut fields);
    assert!(fields.contains("tag = \"v1.0.0\""));
    assert!(fields.contains("branch = \"release\""));

    let lockfile_multiple = GitSelector::from_lockfile(
        "util",
        "abcdef0",
        Some("v1".to_string()),
        Some("main".to_string()),
    )
    .expect_err("lockfile entries cannot have tag and branch");
    assert!(lockfile_multiple.message.contains("multiple git selectors"));
    assert!(
        GitSelector::from_lockfile("util", "abcdef0", Some(String::new()), None)
            .expect_err("empty lockfile tags should fail")
            .message
            .contains("empty git tag")
    );
    assert!(
        GitSelector::from_lockfile("util", "abcdef0", None, Some("../main".to_string()))
            .expect_err("invalid lockfile branches should fail")
            .message
            .contains("invalid git branch")
    );
    assert!(GitSelector::from_lockfile("util", "not-a-rev", None, None)
        .expect_err("invalid lockfile revisions should fail")
        .message
        .contains("invalid git revision"));

    assert!(matches!(
        GitSelector::from_lockfile("util", "abcdef0", Some("v1".to_string()), None)
            .expect("tag selector should load"),
        GitSelector::Tag(_)
    ));
    assert!(matches!(
        GitSelector::from_lockfile("util", "abcdef0", None, Some("main".to_string()))
            .expect("branch selector should load"),
        GitSelector::Branch(_)
    ));
    assert!(matches!(
        GitSelector::from_lockfile("util", "abcdef0", None, None)
            .expect("revision selector should load"),
        GitSelector::Rev(_)
    ));

    let locked = LockedPackage {
        source: LockedPackageSource::Git {
            source: "repo".to_string(),
            rev: "abcdef0".to_string(),
            selector: branch.clone(),
        },
    };
    assert_eq!(
        locked.git_locked_rev("repo", &branch),
        Some("abcdef0".to_string())
    );
    assert_eq!(locked.git_locked_rev("other", &branch), None);
    assert_eq!(locked.git_locked_rev("repo", &tag), None);
    assert_eq!(
        LockedPackage {
            source: LockedPackageSource::Path,
        }
        .git_locked_rev("repo", &branch),
        None
    );

    assert!(!DependencyRefreshPolicy::None.should_refresh("util", &branch));
    assert!(DependencyRefreshPolicy::AllGit.should_refresh("util", &branch));
    assert!(DependencyRefreshPolicy::Selected("util".to_string()).should_refresh("util", &tag));
    assert!(!DependencyRefreshPolicy::Selected("other".to_string()).should_refresh("util", &tag));
    assert!(!DependencyRefreshPolicy::AllGit.should_refresh("util", &rev));

    assert_eq!(
        parse_ls_remote_revision("repo", "refs/heads/main", "abcdef0\trefs/heads/main\n")
            .expect("ls-remote output should parse"),
        "abcdef0"
    );
    assert!(parse_ls_remote_revision("repo", "refs/heads/main", "").is_err());
    assert!(
        parse_ls_remote_revision("repo", "refs/heads/main", "not-a-rev\trefs/heads/main").is_err()
    );
    assert_eq!(
        resolve_git_revision("repo", &GitSelector::Rev("abcdef0".to_string()))
            .expect("revision selectors should not invoke git"),
        "abcdef0"
    );
}

#[test]
fn package_manifest_and_lockfile_helpers_report_current_diagnostics() {
    let temp = TempDir::new("aurora-package-manifest-lockfile-helpers");

    temp.write("missing_package/Aurora.toml", "[dependencies]\n");
    assert!(expect_diag(
        load_package_manifest(&temp.path.join("missing_package")),
        "missing package section should fail"
    )
    .message
    .contains("missing a [package] section"));

    let manifest_cases = [
        (
            "empty_name",
            "[package]\nname = \"\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
            "empty package name",
        ),
        (
            "invalid_name",
            "[package]\nname = \"my-util\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
            "invalid package name",
        ),
        (
            "empty_version",
            "[package]\nname = \"pkg\"\nversion = \"\"\nedition = \"2026\"\n",
            "empty package version",
        ),
        (
            "invalid_version",
            "[package]\nname = \"pkg\"\nversion = \"beta\"\nedition = \"2026\"\n",
            "invalid package version",
        ),
        (
            "empty_edition",
            "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"\"\n",
            "empty package edition",
        ),
        (
            "unsupported_edition",
            "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"2025\"\n",
            "unsupported package edition",
        ),
    ];
    for (name, source, expected) in manifest_cases {
        temp.write(&format!("{}/Aurora.toml", name), source);
        let error = expect_diag(
            load_package_manifest(&temp.path.join(name)),
            "manifest case should fail",
        );
        assert!(
            error.message.contains(expected),
            "expected `{}` in `{}`",
            expected,
            error.message
        );
    }

    temp.write("parse_error/Aurora.toml", "[package");
    assert!(expect_diag(
        load_raw_manifest(&temp.path.join("parse_error/Aurora.toml")),
        "invalid TOML should fail"
    )
    .message
    .contains("failed to parse manifest"));

    assert!(load_lockfile(&temp.path.join("no_lockfile"))
        .expect("missing lockfiles should be accepted")
        .is_empty());

    let lockfile_cases = [
        ("bad_version", "version = 99\n", "unsupported lockfile version"),
        ("bad_parse", "version = [\n", "failed to parse lockfile"),
        (
            "missing_path",
            "version = 1\n\n[[package]]\nname = \"pkg\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
            "missing `path`",
        ),
        (
            "missing_git",
            "version = 1\n\n[[package]]\nname = \"pkg\"\nversion = \"0.1.0\"\nsource = \"git\"\nrev = \"abcdef0\"\n",
            "missing `git`",
        ),
        (
            "missing_rev",
            "version = 1\n\n[[package]]\nname = \"pkg\"\nversion = \"0.1.0\"\nsource = \"git\"\ngit = \"repo\"\n",
            "missing `rev`",
        ),
        (
            "multiple_selectors",
            "version = 1\n\n[[package]]\nname = \"pkg\"\nversion = \"0.1.0\"\nsource = \"git\"\ngit = \"repo\"\nrev = \"abcdef0\"\ntag = \"v1\"\nbranch = \"main\"\n",
            "multiple git selectors",
        ),
        (
            "bad_tag",
            "version = 1\n\n[[package]]\nname = \"pkg\"\nversion = \"0.1.0\"\nsource = \"git\"\ngit = \"repo\"\nrev = \"abcdef0\"\ntag = \"bad..tag\"\n",
            "invalid git tag",
        ),
        (
            "bad_branch",
            "version = 1\n\n[[package]]\nname = \"pkg\"\nversion = \"0.1.0\"\nsource = \"git\"\ngit = \"repo\"\nrev = \"abcdef0\"\nbranch = \"bad branch\"\n",
            "invalid git branch",
        ),
        (
            "bad_rev",
            "version = 1\n\n[[package]]\nname = \"pkg\"\nversion = \"0.1.0\"\nsource = \"git\"\ngit = \"repo\"\nrev = \"not-a-rev\"\n",
            "invalid git revision",
        ),
        (
            "bad_source",
            "version = 1\n\n[[package]]\nname = \"pkg\"\nversion = \"0.1.0\"\nsource = \"registry\"\n",
            "unsupported source",
        ),
    ];
    for (name, source, expected) in lockfile_cases {
        temp.write(&format!("{}/Aurora.lock", name), source);
        let error = load_lockfile(&temp.path.join(name)).expect_err("lockfile case should fail");
        assert!(
            error.message.contains(expected),
            "expected `{}` in `{}`",
            expected,
            error.message
        );
    }
}

#[test]
fn package_update_and_workspace_helpers_cover_edge_paths() {
    let temp = TempDir::new("aurora-package-update-helpers");
    temp.write(
        "app/Aurora.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[dependencies]\nutil = { path = \"../util\" }\n",
    );
    temp.write("app/src/main.au", "def main():\n    pass\n");
    temp.write(
        "util/Aurora.toml",
        "[package]\nname = \"util\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    temp.write(
        "util/src/math.au",
        "public def value() -> int32:\n    return 1\n",
    );

    let missing = update_git_dependencies_in_working_dir(&temp.path.join("app"), Some("missing"))
        .expect_err("missing selected package should fail");
    assert!(missing
        .message
        .contains("not part of the current package graph"));

    let path_dependency =
        update_git_dependencies_in_working_dir(&temp.path.join("app"), Some("util"))
            .expect_err("path dependencies cannot be refreshed as git dependencies");
    assert!(path_dependency.message.contains("not a git dependency"));

    let missing_start = update_git_dependencies_in_working_dir(&temp.path.join("missing"), None)
        .expect_err("missing update roots should report package path errors");
    assert!(missing_start
        .message
        .contains("failed to resolve package path"));

    let isolated = temp.path.join("isolated");
    fs::create_dir_all(&isolated).expect("failed to create isolated dir");
    let no_workspace = update_git_dependencies_in_working_dir(&isolated, None)
        .expect_err("directories outside packages should fail");
    assert!(no_workspace.message.contains("could not find an enclosing"));

    temp.write("empty_workspace/Aurora.toml", "[workspace]\nmembers = []\n");
    let empty_workspace =
        update_git_dependencies_in_working_dir(&temp.path.join("empty_workspace"), None)
            .expect_err("empty workspaces should fail");
    assert!(empty_workspace
        .message
        .contains("does not declare any members"));

    temp.write(
        "workspace/Aurora.toml",
        "[workspace]\nmembers = [\"app\", \"lib\"]\n",
    );
    temp.write(
        "workspace/app/Aurora.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    temp.write("workspace/app/src/main.au", "def main():\n    pass\n");
    temp.write(
        "workspace/lib/Aurora.toml",
        "[package]\nname = \"lib\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    temp.write("workspace/lib/src/main.au", "def main():\n    pass\n");
    let update = update_git_dependencies_in_working_dir(&temp.path.join("workspace"), None)
        .expect("workspace updates should resolve member graph");
    assert!(update.updated_packages.is_empty());
    assert_eq!(
        update.lockfile_root,
        fs::canonicalize(temp.path.join("workspace")).expect("workspace should canonicalize")
    );
    let lockfile = fs::read_to_string(temp.path.join("workspace/Aurora.lock"))
        .expect("workspace update should write lockfile");
    assert!(lockfile.contains("name = \"app\""));
    assert!(lockfile.contains("name = \"lib\""));
}

#[test]
fn package_resolver_reports_graph_limit_and_workspace_lookup_edges() {
    let temp = TempDir::new("aurora-package-graph-limits");
    let manifest_dir = write_package(&temp, "overflow", "overflow", "");
    let mut resolver = PackageResolver::new(BTreeMap::new(), DependencyRefreshPolicy::None);
    for index in 0..MAX_PACKAGES_IN_GRAPH {
        let name = format!("pkg_{}", index);
        let package_dir = temp.path.join(format!("existing_{}", index));
        let source_root = package_dir.join("src");
        resolver.packages.insert(
            name.clone(),
            PackageSource {
                name,
                version: "0.1.0".to_string(),
                allow_ffi: false,
                manifest_dir: package_dir,
                source_root: source_root.clone(),
                canonical_source_root: source_root,
                external_prefix: None,
                dependencies: BTreeMap::new(),
                ffi_dependencies: BTreeSet::new(),
                origin: PackageOrigin::Path,
            },
        );
    }
    let graph_limit = resolver
        .resolve_package(&manifest_dir, None, PackageOrigin::Path)
        .expect_err("resolver should reject graphs at the package limit");
    assert!(graph_limit.message.contains("package graph exceeds"));

    temp.write(
        "workspace/Aurora.toml",
        "[workspace]\nmembers = [\"other\"]\n",
    );
    let app_dir = write_package(&temp, "workspace/app", "app", "");
    assert!(find_workspace_root(&app_dir)
        .expect("workspace lookup should tolerate non-member workspaces")
        .is_none());
    let solo_dir = write_package(&temp, "solo", "solo", "");
    assert!(
        find_enclosing_workspace_root_from_dir(&solo_dir.join("src"))
            .expect("workspace lookup should continue past package manifests")
            .is_none()
    );
    temp.write("workspace_only/Aurora.toml", "[workspace]\nmembers = []\n");
    temp.write("workspace_only/src/main.au", "def main():\n    pass\n");
    assert!(
        find_enclosing_package_manifest_dir(&temp.path.join("workspace_only/src/main.au"))
            .expect("package lookup should skip workspace-only manifests")
            .is_none()
    );
    assert_eq!(
        find_enclosing_package_manifest_dir_from_dir(&app_dir.join("src"))
            .expect("directory package lookup should find the app package"),
        Some(app_dir.clone())
    );
    assert!(
        find_enclosing_package_manifest_dir_from_dir(&temp.path.join("workspace_only/src"))
            .expect("directory package lookup should skip workspace-only manifests")
            .is_none()
    );
    assert_eq!(
        find_enclosing_workspace_root_from_dir(&temp.path.join("workspace_only/src"))
            .expect("workspace lookup should find workspace-only manifests"),
        Some(temp.path.join("workspace_only"))
    );

    temp.write(
        "valid_workspace/Aurora.toml",
        "[workspace]\nmembers = [\"app\"]\n",
    );
    let valid_member_dir = write_package(&temp, "valid_workspace/app", "app", "");
    assert_eq!(
        load_workspace_member_dirs(&temp.path.join("valid_workspace"))
            .expect("valid workspace members should load"),
        vec![fs::canonicalize(valid_member_dir).expect("member dir should canonicalize")]
    );

    temp.write(
        "missing_member/Aurora.toml",
        "[workspace]\nmembers = [\"ghost\"]\n",
    );
    let missing_member = load_workspace_member_dirs(&temp.path.join("missing_member"))
        .expect_err("missing workspace members should report package path errors");
    assert!(missing_member
        .message
        .contains("failed to resolve package path"));
}

#[test]
fn package_lookup_helpers_propagate_manifest_parse_errors() {
    let temp = TempDir::new("aurora-package-lookup-parse-errors");

    temp.write("bad_package/Aurora.toml", "[package");
    temp.write("bad_package/src/main.au", "def main():\n    pass\n");
    let bad_package = PackageGraph::discover_for_entry(&temp.path.join("bad_package/src/main.au"))
        .expect_err("malformed package manifests should fail package discovery");
    assert!(bad_package.message.contains("failed to parse manifest"));

    temp.write("bad_workspace/Aurora.toml", "[workspace");
    temp.write(
        "bad_workspace/app/Aurora.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    temp.write("bad_workspace/app/src/main.au", "def main():\n    pass\n");
    let bad_workspace =
        PackageGraph::discover_for_entry(&temp.path.join("bad_workspace/app/src/main.au"))
            .expect_err("malformed workspace manifests should fail package discovery");
    assert!(bad_workspace.message.contains("failed to parse manifest"));

    assert!(
        find_enclosing_package_manifest_dir_from_dir(&temp.path.join("bad_package/src"))
            .expect_err("directory package lookup should propagate parse errors")
            .message
            .contains("failed to parse manifest")
    );
    assert!(
        find_enclosing_workspace_root_from_dir(&temp.path.join("bad_workspace/app/src"))
            .expect_err("directory workspace lookup should propagate parse errors")
            .message
            .contains("failed to parse manifest")
    );
    assert!(load_workspace_member_dirs(&temp.path.join("bad_workspace"))
        .expect_err("workspace member loading should propagate parse errors")
        .message
        .contains("failed to parse manifest"));
}

#[test]
fn package_resolver_reports_missing_source_roots_and_dependency_paths() {
    let temp = TempDir::new("aurora-package-missing-source-roots");

    temp.write(
        "no_src/Aurora.toml",
        "[package]\nname = \"no_src\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    let missing_source_root = PackageResolver::new(BTreeMap::new(), DependencyRefreshPolicy::None)
        .resolve_package(&temp.path.join("no_src"), None, PackageOrigin::Path)
        .expect_err("packages without src directories should fail");
    assert!(missing_source_root
        .message
        .contains("failed to resolve package path"));

    write_package(
        &temp,
        "missing_dep_app",
        "missing_dep_app",
        "\n[dependencies]\nmissing = { path = \"../missing\" }\n",
    );
    let missing_dependency = PackageResolver::new(BTreeMap::new(), DependencyRefreshPolicy::None)
        .resolve_package(
            &temp.path.join("missing_dep_app"),
            None,
            PackageOrigin::Path,
        )
        .expect_err("missing path dependencies should fail");
    assert!(missing_dependency
        .message
        .contains("failed to resolve package path"));
}

#[test]
fn package_workspace_update_marks_non_member_dependencies_external() {
    let temp = TempDir::new("aurora-package-workspace-external-deps");
    temp.write(
        "workspace/Aurora.toml",
        "[workspace]\nmembers = [\"app\"]\n",
    );
    write_package(
        &temp,
        "workspace/app",
        "app",
        "\n[dependencies]\nutil = { path = \"../util\" }\n",
    );
    write_package(&temp, "workspace/util", "util", "");

    let (graph, updated_packages) = resolve_package_graph_for_update(
        &temp.path.join("workspace"),
        DependencyRefreshPolicy::AllGit,
    )
    .expect("workspace graph should resolve non-member path dependencies");
    assert!(updated_packages.is_empty());
    assert_eq!(
        graph
            .packages
            .get("app")
            .expect("workspace member should be present")
            .external_prefix,
        None
    );
    assert_eq!(
        graph
            .packages
            .get("util")
            .expect("path dependency should be present")
            .external_prefix
            .as_deref(),
        Some("util")
    );
}

#[test]
fn package_path_cache_and_validation_helpers_cover_remaining_edges() {
    let temp = TempDir::new("aurora-package-path-cache-helpers");

    assert!(is_valid_package_name("pkg_1"));
    assert!(!is_valid_package_name(""));
    assert!(!is_valid_package_name("1pkg"));
    assert!(!is_valid_package_name("pkg-name"));
    assert!(is_valid_package_version("1.0.0-alpha+1"));
    assert!(!is_valid_package_version(""));
    assert!(!is_valid_package_version("beta"));
    assert!(!is_valid_package_version("1.0@bad"));
    assert!(is_valid_package_version("1-alpha+2"));
    assert_eq!(toml_string("a\"b"), "\"a\\\"b\"");
    assert_eq!(normalize_member_path(""), ".");
    assert_eq!(normalize_member_path("./app/"), "app");
    assert_eq!(normalize_relative_path(Path::new(".")), ".");
    assert_eq!(
        normalize_relative_path(Path::new("./app/../util")),
        "app/../util"
    );
    assert_eq!(
        relative_path_from(Path::new("/tmp/workspace"), Path::new("/tmp/workspace/app")),
        PathBuf::from("app")
    );
    assert_eq!(
        relative_path_from(Path::new("/tmp/workspace"), Path::new("/tmp/workspace")),
        PathBuf::from(".")
    );

    fs::create_dir_all(temp.path.join("root/src")).expect("failed to create source root");
    let missing_source = temp.path.join("root/src/nested/missing.au");
    let canonical_missing =
        canonicalize_if_exists(&missing_source).expect("missing suffix should canonicalize");
    assert!(canonical_missing.ends_with("nested/missing.au"));

    let source_root = temp.path.join("root/src");
    let canonical_source_root = fs::canonicalize(&source_root).expect("source root");
    assert_eq!(
        canonicalize_if_exists(&source_root).expect("existing paths should canonicalize"),
        canonical_source_root
    );
    let checked =
        checked_source_file_path(&source_root, &canonical_source_root, &["main".to_string()])
            .expect("normal imports should stay under source root");
    assert!(checked.ends_with("main.au"));
    let escaped = checked_source_file_path(
        &source_root,
        &canonical_source_root,
        &["..".to_string(), "outside".to_string()],
    )
    .expect_err("imports that escape the source root should fail");
    assert!(escaped.message.contains("escapes package source root"));

    assert_eq!(hash_source_key("repo").len(), 64);
    assert_eq!(
        normalize_git_source(&temp.path, "https://example.com/repo.git")
            .expect("explicit URLs should pass"),
        "https://example.com/repo.git"
    );
    fs::create_dir_all(temp.path.join("local_repo")).expect("failed to create local repo dir");
    assert!(normalize_git_source(&temp.path, "local_repo")
        .expect("existing relative git paths should canonicalize")
        .contains("local_repo"));
    assert!(normalize_git_source(
        &temp.path,
        &temp.path.join("local_repo").display().to_string()
    )
    .expect("existing absolute git paths should canonicalize")
    .contains("local_repo"));
    assert!(normalize_git_source(&temp.path, "missing_repo").is_err());
    assert!(validate_git_source_literal("util", "").is_err());
    assert!(validate_git_source_literal("util", "-bad").is_err());
    assert!(validate_git_source_literal("util", "bad\nsource").is_err());
    assert!(validate_git_selector_literal("util", "branch", "").is_err());
    assert!(validate_git_selector_literal("util", "branch", "-main").is_err());
    assert!(validate_git_selector_literal("util", "branch", "bad branch").is_err());
    assert!(validate_git_selector_literal("util", "branch", "../main").is_err());
    assert!(validate_git_revision_literal("util", "not-a-rev").is_err());
    assert!(unsupported_version_dependency("util", Some("1.0.0"))
        .message
        .contains("version-only dependencies"));
    assert!(unsupported_version_dependency("util", None)
        .message
        .contains("<unspecified>"));
    assert!(
        validate_dependency_shape("util", None, Some("1.0.0"), None, None, None, None)
            .expect_err("version fields should fail")
            .message
            .contains("version-only dependencies")
    );
    assert!(
        validate_dependency_shape("util", None, None, None, None, None, None)
            .expect_err("dependencies need exactly one source")
            .message
            .contains("exactly one dependency source")
    );
    assert!(validate_dependency_shape(
        "util",
        Some("../util"),
        None,
        None,
        Some("abcdef0"),
        None,
        None
    )
    .expect_err("selectors without git should fail")
    .message
    .contains("without `git"));

    let temp_path = unique_temp_path(&temp.path, "file").expect("temp path should be unique");
    assert!(temp_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .contains("file"));

    let atomic_path = temp.path.join("atomic/out.txt");
    write_atomic_file(&atomic_path, b"hello", "test file", "`out.txt`")
        .expect("atomic writes should succeed");
    assert_eq!(
        fs::read_to_string(&atomic_path).expect("atomic file should be readable"),
        "hello"
    );

    let checkout = temp.path.join("checkout");
    fs::create_dir_all(&checkout).expect("failed to create checkout");
    assert!(!git_checkout_contains_required_files(&checkout)
        .expect("missing manifests should not match"));
    assert!(!cached_git_checkout_matches_rev(&checkout, "abcdef0")
        .expect("incomplete checkouts should not match"));
    fs::write(checkout.join(MANIFEST_NAME), "[package]\nname = \"pkg\"\n")
        .expect("failed to write manifest");
    assert!(git_checkout_contains_required_files(&checkout).expect("manifest files should match"));
    assert!(!cached_git_checkout_matches_rev(&checkout, "abcdef0")
        .expect("missing cache revisions should not match"));
    write_cached_git_revision(&checkout, "abcdef0").expect("revision marker should write");
    assert_eq!(
        read_cached_git_revision(&checkout)
            .expect("revision marker should read")
            .as_deref(),
        Some("abcdef0")
    );
    assert!(cached_git_checkout_matches_rev(&checkout, "abcdef0")
        .expect("matching cache revisions should match"));
}

#[test]
fn package_git_dependency_resolution_uses_cached_revision_selectors() {
    let temp = TempDir::new("aurora-package-rev-selector-cache");
    fs::create_dir_all(temp.path.join("local_repo")).expect("failed to create local repo dir");
    let source = fs::canonicalize(temp.path.join("local_repo"))
        .expect("local repo path should canonicalize")
        .to_string_lossy()
        .to_string();

    let original_cache_home = std::env::var_os("XDG_CACHE_HOME");
    std::env::set_var("XDG_CACHE_HOME", temp.path.join("cache"));
    let checkout = git_cache_root()
        .join(hash_source_key(&source))
        .join("abcdef0");
    fs::create_dir_all(&checkout).expect("failed to create cached checkout");
    fs::write(
        checkout.join(MANIFEST_NAME),
        "[package]\nname = \"util\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    )
    .expect("failed to write cached manifest");
    write_cached_git_revision(&checkout, "abcdef0").expect("failed to write cached rev");

    let resolved = resolve_git_dependency(
        &temp.path,
        "util",
        source.clone(),
        &GitSelector::Rev("abcdef0".to_string()),
        None,
    )
    .expect("explicit revision selectors should use matching cache entries");
    assert_eq!(resolved.checkout_dir, checkout);
    assert_eq!(resolved.normalized_source, source);
    assert_eq!(resolved.resolved_rev, "abcdef0");
    assert!(matches!(resolved.selector, GitSelector::Rev(_)));

    match original_cache_home {
        Some(value) => std::env::set_var("XDG_CACHE_HOME", value),
        None => std::env::remove_var("XDG_CACHE_HOME"),
    }
}

#[test]
fn package_resolver_reports_git_dependency_resolution_and_package_name_errors() {
    let temp = TempDir::new("aurora-package-git-resolver-errors");

    let missing_source_app = write_package(
        &temp,
        "missing-source-app",
        "missing_source_app",
        "\n[dependencies]\nutil = { git = \"missing-repo\", rev = \"abcdef0\" }\n",
    );
    let mut missing_source_resolver =
        PackageResolver::new(BTreeMap::new(), DependencyRefreshPolicy::None);
    let missing_source = missing_source_resolver
        .resolve_package(&missing_source_app, None, PackageOrigin::Path)
        .expect_err("missing git sources should be reported through package resolution");
    assert!(missing_source.message.contains("git dependency source"));

    let _cache_home = EnvVarGuard::set("XDG_CACHE_HOME", temp.path.join("cache-mismatch"));
    let source_dir = temp.path.join("cached-source");
    fs::create_dir_all(&source_dir).expect("failed to create source dir");
    let source = fs::canonicalize(&source_dir)
        .expect("source path should canonicalize")
        .to_string_lossy()
        .to_string();
    let checkout = git_cache_root()
        .join(hash_source_key(&source))
        .join("abcdef0");
    fs::create_dir_all(checkout.join("src")).expect("failed to create cached checkout");
    fs::write(
        checkout.join(MANIFEST_NAME),
        "[package]\nname = \"wrong\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    )
    .expect("failed to write cached manifest");
    fs::write(
        checkout.join("src/lib.au"),
        "public def value() -> int32:\n    return 1\n",
    )
    .expect("failed to write cached source");
    write_cached_git_revision(&checkout, "abcdef0").expect("failed to write cached rev");

    let wrong_name_app = write_package(
        &temp,
        "wrong-name-app",
        "wrong_name_app",
        &format!(
            "\n[dependencies]\nutil = {{ git = {}, rev = \"abcdef0\" }}\n",
            toml_string(&source)
        ),
    );
    let mut wrong_name_resolver =
        PackageResolver::new(BTreeMap::new(), DependencyRefreshPolicy::None);
    let wrong_name = wrong_name_resolver
        .resolve_package(&wrong_name_app, None, PackageOrigin::Path)
        .expect_err("cached git packages must still match the dependency name");
    assert!(wrong_name.message.contains("does not match package name"));
}

#[test]
fn package_git_resolution_and_checkout_helpers_cover_live_git_edges() {
    let temp = TempDir::new("aurora-package-live-git-helper-edges");
    let repo = init_git_package_repo(&temp, "util-repo", "util");
    let source = fs::canonicalize(&repo)
        .expect("repo should canonicalize")
        .to_string_lossy()
        .to_string();
    let main_rev = run_git(&repo, &["rev-parse", "HEAD"]);
    run_git(&repo, &["tag", "v1.0.0"]);
    fs::create_dir_all(temp.path.join("not-a-git-repo")).expect("failed to create non-repo");
    let non_repo_source = fs::canonicalize(temp.path.join("not-a-git-repo"))
        .expect("non-repo path should canonicalize")
        .to_string_lossy()
        .to_string();

    assert_eq!(
        resolve_git_revision(&source, &GitSelector::Branch("main".to_string()))
            .expect("branch revisions should resolve through git"),
        main_rev
    );
    assert_eq!(
        resolve_git_revision(&source, &GitSelector::Tag("v1.0.0".to_string()))
            .expect("tag revisions should resolve through git"),
        main_rev
    );
    let missing_tag =
        resolve_git_revision(&non_repo_source, &GitSelector::Tag("v1.0.0".to_string()))
            .expect_err("tag resolution against a non-repo should fail through git");
    assert!(missing_tag.message.contains("git ls-remote"));

    let branch_error = resolve_git_dependency(
        &temp.path,
        "util",
        "not-a-git-repo".to_string(),
        &GitSelector::Branch("main".to_string()),
        None,
    )
    .expect_err("branch resolution against a non-repo should fail");
    assert!(branch_error
        .message
        .contains("failed to resolve git dependency"));

    {
        let _cache_home = EnvVarGuard::set("XDG_CACHE_HOME", temp.path.join("cache-bad-rev"));
        let checkout_error = resolve_git_dependency(
            &temp.path,
            "util",
            source.clone(),
            &GitSelector::Rev("abcdef0".to_string()),
            None,
        )
        .expect_err("syntactically valid but missing revisions should fail during checkout");
        assert!(checkout_error
            .message
            .contains("git -c advice.detachedHead=false checkout"));
    }

    {
        let _cache_home = EnvVarGuard::set("XDG_CACHE_HOME", temp.path.join("cache-fresh"));
        let resolved = resolve_git_dependency(
            &temp.path,
            "util",
            source.clone(),
            &GitSelector::Rev(main_rev.clone()),
            None,
        )
        .expect("fresh revision dependencies should materialize into the cache");
        assert!(resolved.checkout_dir.join(MANIFEST_NAME).is_file());
        assert_eq!(
            read_cached_git_revision(&resolved.checkout_dir)
                .expect("cache marker should read")
                .as_deref(),
            Some(main_rev.as_str())
        );
    }

    {
        let collision_parent = temp.path.join("materialize-collisions");
        fs::create_dir_all(&collision_parent).expect("failed to create collision parent");

        let matching_checkout = collision_parent.join("matching");
        fs::create_dir_all(&matching_checkout).expect("failed to create matching checkout");
        fs::write(
            matching_checkout.join(MANIFEST_NAME),
            "[package]\nname = \"util\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
        )
        .expect("failed to write matching manifest");
        write_cached_git_revision(&matching_checkout, &main_rev)
            .expect("failed to write matching revision marker");
        assert_eq!(
            materialize_git_checkout(&source, &main_rev, &matching_checkout)
                .expect("matching concurrent checkout placements should be reused"),
            matching_checkout
        );

        let incompatible_checkout = collision_parent.join("incompatible");
        fs::create_dir_all(&incompatible_checkout).expect("failed to create incompatible checkout");
        fs::write(
            incompatible_checkout.join(MANIFEST_NAME),
            "[package]\nname = \"util\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
        )
        .expect("failed to write incompatible manifest");
        write_cached_git_revision(&incompatible_checkout, "abcdef0")
            .expect("failed to write incompatible revision marker");
        let collision = materialize_git_checkout(&source, &main_rev, &incompatible_checkout)
            .expect_err("incompatible concurrent checkout placements should be rejected");
        assert!(collision.message.contains("incompatible cached checkout"));
    }

    {
        let _cache_home = EnvVarGuard::set("XDG_CACHE_HOME", temp.path.join("cache-resolver"));
        let app_dir = write_package(
            &temp,
            "git-app",
            "git_app",
            &format!(
                "\n[dependencies]\nutil = {{ git = {}, rev = {} }}\n",
                toml_string(&source),
                toml_string(&main_rev)
            ),
        );
        let mut resolver = PackageResolver::new(BTreeMap::new(), DependencyRefreshPolicy::None);
        let root_package = resolver
            .resolve_package(&app_dir, None, PackageOrigin::Path)
            .expect("package resolver should load explicit git revision dependencies");
        assert_eq!(root_package, "git_app");
        let root = resolver
            .packages
            .get("git_app")
            .expect("root package should be resolved");
        assert_eq!(
            root.dependencies.get("util").map(String::as_str),
            Some("util")
        );
        assert!(matches!(
            resolver
                .packages
                .get("util")
                .expect("git dependency should be resolved")
                .origin,
            PackageOrigin::Git { .. }
        ));
    }

    {
        let _cache_home = EnvVarGuard::set("XDG_CACHE_HOME", temp.path.join("cache-refresh"));
        let app_dir = write_package(
            &temp,
            "git-refresh-app",
            "git_refresh_app",
            &format!(
                "\n[dependencies]\nutil = {{ git = {}, branch = \"main\" }}\n",
                toml_string(&source),
            ),
        );
        let locked_packages = BTreeMap::from([(
            "util".to_string(),
            LockedPackage {
                source: LockedPackageSource::Git {
                    source: source.clone(),
                    rev: "abcdef0".to_string(),
                    selector: GitSelector::Branch("main".to_string()),
                },
            },
        )]);
        let mut resolver = PackageResolver::new(locked_packages, DependencyRefreshPolicy::AllGit);
        let root_package = resolver
            .resolve_package(&app_dir, None, PackageOrigin::Path)
            .expect("refreshing git dependencies should resolve through the package graph");
        assert_eq!(root_package, "git_refresh_app");
        assert!(resolver.refreshed_packages.contains("util"));
        assert!(matches!(
            resolver
                .packages
                .get("util")
                .expect("git dependency should be resolved")
                .origin,
            PackageOrigin::Git { .. }
        ));
    }

    {
        let _cache_home =
            EnvVarGuard::set("XDG_CACHE_HOME", temp.path.join("cache-materialize-error"));
        let materialize_error = resolve_git_dependency(
            &temp.path,
            "util",
            non_repo_source.clone(),
            &GitSelector::Rev("abcdef0".to_string()),
            None,
        )
        .expect_err("explicit rev dependencies should wrap checkout materialization errors");
        assert!(materialize_error
            .message
            .contains("failed to materialize git dependency `util`"));
    }

    {
        let _cache_home = EnvVarGuard::set("XDG_CACHE_HOME", temp.path.join("cache-file"));
        let checkout = git_cache_root()
            .join(hash_source_key(&source))
            .join(&main_rev);
        fs::create_dir_all(checkout.parent().expect("checkout should have a parent"))
            .expect("failed to create cache parent");
        fs::write(&checkout, "not a directory").expect("failed to write cache file");
        let error = ensure_git_checkout(&source, &main_rev)
            .expect_err("non-directory cache entries should be rejected");
        assert!(error.message.contains("not a real directory"));
    }

    {
        let _cache_home = EnvVarGuard::set("XDG_CACHE_HOME", temp.path.join("cache-stale"));
        let checkout = git_cache_root()
            .join(hash_source_key(&non_repo_source))
            .join("abcdef0");
        fs::create_dir_all(&checkout).expect("failed to create stale checkout");
        fs::write(checkout.join("stale.txt"), "stale").expect("failed to write stale file");
        let error = ensure_git_checkout(&non_repo_source, "abcdef0")
            .expect_err("stale checkouts for invalid sources should fail while rematerializing");
        assert!(error.message.contains("git -c core.symlinks=false clone"));
        assert!(
            !checkout.exists(),
            "stale checkout directory should be removed before rematerialization"
        );
    }
}

#[test]
fn package_graph_helpers_report_unusual_but_supported_edges() {
    let temp = TempDir::new("aurora-package-graph-helper-edges");
    assert!(
        PackageGraph::discover_for_entry(&temp.path.join("loose.au"))
            .expect("non-package entries should be accepted")
            .is_none()
    );
    let app_dir = write_package(
        &temp,
        "app",
        "app",
        "\n[dependencies]\nutil = { path = \"../util\" }\n",
    );
    write_package(&temp, "util", "util", "");
    temp.write(
        "util/src/lib.au",
        "public def answer() -> int32:\n    return 42\n",
    );
    let main_path =
        fs::canonicalize(app_dir.join("src/main.au")).expect("main path should canonicalize");
    let graph = PackageGraph::discover_for_entry(&main_path)
        .expect("package discovery should succeed")
        .expect("entry should be in a package");

    temp.write(
        "app/tests/smoke.au",
        "from helpers import answer\n\ndef main() -> int32:\n    return answer()\n",
    );
    let test_path =
        fs::canonicalize(app_dir.join("tests/smoke.au")).expect("test entry should canonicalize");
    let test_graph = PackageGraph::discover_for_entry(&test_path)
        .expect("package test entry should be accepted")
        .expect("test entry should discover its package");
    assert_eq!(
        test_graph.module_name_for_path(&test_path).as_deref(),
        Some("tests.smoke")
    );

    let outside_entry = PackageGraph::discover_for_entry(&app_dir.join("Aurora.toml"))
        .expect_err("package files outside src should be rejected");
    assert!(outside_entry
        .message
        .contains("outside package source root"));

    assert_eq!(
        graph
            .module_name_for_path(&main_path)
            .expect("root module should resolve"),
        "main"
    );
    assert!(graph
        .module_name_for_path(&temp.path.join("outside.au"))
        .is_none());
    let util_source_root = &graph
        .packages
        .get("util")
        .expect("dependency package should be present")
        .canonical_source_root;
    assert_eq!(
        graph
            .module_name_for_path(util_source_root)
            .expect("empty dependency module path should use prefix"),
        "util"
    );
    assert!(graph
        .dependency_aliases_for_path(&main_path)
        .contains("util"));
    assert_eq!(
        graph
            .resolve_import_path(&main_path, &["util".to_string(), "lib".to_string()])
            .expect("dependency imports should resolve through the graph"),
        util_source_root.join("lib.au")
    );
    graph
        .write_lockfile()
        .expect("package graph lockfile writes should succeed");
    let lockfile = fs::read_to_string(app_dir.join("Aurora.lock"))
        .expect("package graph lockfile should be readable");
    assert!(lockfile.contains("name = \"app\""));
    assert!(lockfile.contains("name = \"util\""));
    assert!(graph
        .dependency_aliases_for_path(&temp.path.join("outside.au"))
        .is_empty());
    assert!(graph
        .resolve_import_path(&temp.path.join("outside.au"), &["main".to_string()])
        .expect_err("paths outside the graph should fail")
        .message
        .contains("could not determine package source root"));
    assert!(graph
        .resolve_import_path(&main_path, &[])
        .expect_err("empty import paths should still stay inside the source root")
        .message
        .contains("escapes package source root"));

    let mut broken_graph = graph.clone();
    broken_graph
        .packages
        .get_mut("app")
        .expect("root package should be present")
        .dependencies
        .insert("ghost".to_string(), "ghost".to_string());
    let missing_dependency = broken_graph
        .resolve_import_path(&main_path, &["ghost".to_string(), "thing".to_string()])
        .expect_err("missing dependency graph nodes should fail");
    assert!(missing_dependency
        .message
        .contains("is missing from the package graph"));

    let bad_lock_app = write_package(&temp, "bad_lock_app", "bad_lock_app", "");
    fs::create_dir_all(bad_lock_app.join(LOCKFILE_NAME)).expect("failed to create bad lockfile");
    let bad_lock_main =
        fs::canonicalize(bad_lock_app.join("src/main.au")).expect("bad lock main canonical");
    let bad_lock = PackageGraph::discover_for_entry(&bad_lock_main)
        .expect_err("directory lockfiles should fail package discovery");
    assert!(bad_lock.message.contains("failed to read lockfile"));
}

#[test]
fn package_resolver_reports_cycle_duplicate_and_expected_name_edges() {
    let temp = TempDir::new("aurora-package-resolver-edges");
    write_package(
        &temp,
        "cycle_app",
        "cycle_app",
        "\n[dependencies]\ncycle_util = { path = \"../cycle_util\" }\n",
    );
    write_package(
        &temp,
        "cycle_util",
        "cycle_util",
        "\n[dependencies]\ncycle_app = { path = \"../cycle_app\" }\n",
    );
    let mut resolver = PackageResolver::new(BTreeMap::new(), DependencyRefreshPolicy::None);
    let cycle = resolver
        .resolve_package(&temp.path.join("cycle_app"), None, PackageOrigin::Path)
        .expect_err("cyclic path dependencies should fail");
    assert!(cycle.message.contains("cyclic package dependency"));

    write_package(&temp, "actual", "actual", "");
    let mut resolver = PackageResolver::new(BTreeMap::new(), DependencyRefreshPolicy::None);
    let mismatch = resolver
        .resolve_package(
            &temp.path.join("actual"),
            Some("expected"),
            PackageOrigin::Path,
        )
        .expect_err("expected dependency aliases should match package names");
    assert!(mismatch.message.contains("does not match package name"));

    write_package(&temp, "dup_a", "dup", "");
    write_package(&temp, "dup_b", "dup", "");
    let mut resolver = PackageResolver::new(BTreeMap::new(), DependencyRefreshPolicy::None);
    let first = resolver
        .resolve_package(&temp.path.join("dup_a"), None, PackageOrigin::Path)
        .expect("first package should resolve");
    let again = resolver
        .resolve_package(&temp.path.join("dup_a"), None, PackageOrigin::Path)
        .expect("resolving the same package should be idempotent");
    assert_eq!(first, again);
    let duplicate = resolver
        .resolve_package(&temp.path.join("dup_b"), None, PackageOrigin::Path)
        .expect_err("same package name at a different path should fail");
    assert!(duplicate.message.contains("resolves to multiple paths"));
}

#[test]
fn package_io_helpers_cover_local_error_paths() {
    let temp = TempDir::new("aurora-package-io-helper-edges");
    let missing_manifest = expect_diag(
        load_raw_manifest(&temp.path.join("missing/Aurora.toml")),
        "missing manifests should report read errors",
    );
    assert!(missing_manifest.message.contains("failed to read manifest"));

    fs::create_dir_all(temp.path.join("lockdir/Aurora.lock"))
        .expect("failed to create directory lockfile");
    let lock_read_error = load_lockfile(&temp.path.join("lockdir"))
        .expect_err("directory lockfiles should report read errors");
    assert!(lock_read_error.message.contains("failed to read lockfile"));

    temp.write(
        "no_workspace/Aurora.toml",
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    let missing_workspace = load_workspace_member_dirs(&temp.path.join("no_workspace"))
        .expect_err("workspace member loading requires a workspace section");
    assert!(missing_workspace
        .message
        .contains("missing a [workspace] section"));

    let parent_file = temp.write("parent-file", "not a directory");
    let atomic_parent_error = write_atomic_file(
        &parent_file.join("child.txt"),
        b"child",
        "test file",
        "`child.txt`",
    )
    .expect_err("file parents should reject atomic writes below them");
    assert!(atomic_parent_error
        .message
        .contains("failed to prepare parent directory"));
    let atomic_no_parent_error = write_atomic_file(Path::new(""), b"payload", "test file", "``")
        .expect_err("empty paths should report missing parents");
    assert!(atomic_no_parent_error
        .message
        .contains("has no parent directory"));
    let existing_destination_dir = temp.path.join("existing-destination-dir");
    fs::create_dir_all(&existing_destination_dir).expect("failed to create destination dir");
    let atomic_replace_error = write_atomic_file(
        &existing_destination_dir,
        b"replacement",
        "test file",
        "`existing-destination-dir`",
    )
    .expect_err("atomic writes should fail when replacing a directory with a file");
    assert!(atomic_replace_error
        .message
        .contains("failed to place test file"));
    let not_a_tree = temp.write("not-a-tree", "not a directory");
    let tree_error =
        reject_symlinks_in_tree(&not_a_tree).expect_err("files cannot be inspected as trees");
    assert!(tree_error
        .message
        .contains("failed to inspect git checkout"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let unwritable_dir = temp.path.join("unwritable");
        fs::create_dir_all(&unwritable_dir).expect("failed to create unwritable dir");
        let original_permissions = fs::metadata(&unwritable_dir)
            .expect("unwritable dir metadata")
            .permissions();
        let mut locked_permissions = original_permissions.clone();
        locked_permissions.set_mode(0o500);
        fs::set_permissions(&unwritable_dir, locked_permissions)
            .expect("failed to make directory unwritable");
        let unwritable_error = write_atomic_file(
            &unwritable_dir.join("out.txt"),
            b"payload",
            "test file",
            "`out.txt`",
        )
        .expect_err("unwritable parents should reject temporary file creation");
        fs::set_permissions(&unwritable_dir, original_permissions)
            .expect("failed to restore writable permissions");
        assert!(unwritable_error
            .message
            .contains("failed to create temporary test file"));
    }

    let missing_leaf = PathBuf::from(format!(
        "aurora-missing-leaf-{}-{}",
        std::process::id(),
        unix_time_nanos().expect("clock should be readable")
    ));
    assert_eq!(
        canonicalize_if_exists(&missing_leaf).expect("single missing leaves should pass through"),
        missing_leaf
    );
    assert!(normalize_relative_path(Path::new("/tmp/aurora")).starts_with('/'));

    let original_timeout = std::env::var_os("AURORA_GIT_TIMEOUT_MS");
    std::env::set_var("AURORA_GIT_TIMEOUT_MS", "25");
    assert_eq!(git_command_timeout(), StdDuration::from_millis(25));
    std::env::set_var("AURORA_GIT_TIMEOUT_MS", "0");
    assert_eq!(git_command_timeout(), DEFAULT_GIT_COMMAND_TIMEOUT);
    std::env::set_var("AURORA_GIT_TIMEOUT_MS", "not-a-number");
    assert_eq!(git_command_timeout(), DEFAULT_GIT_COMMAND_TIMEOUT);
    match original_timeout {
        Some(value) => std::env::set_var("AURORA_GIT_TIMEOUT_MS", value),
        None => std::env::remove_var("AURORA_GIT_TIMEOUT_MS"),
    }

    let original_cache_home = std::env::var_os("XDG_CACHE_HOME");
    std::env::set_var("XDG_CACHE_HOME", std::env::temp_dir());
    assert_eq!(
        git_cache_root(),
        std::env::temp_dir().join("aurora").join("git")
    );
    assert_eq!(git_cache_roots().len(), 1);
    let distinct_cache_home = temp.path.join("xdg-cache-home");
    std::env::set_var("XDG_CACHE_HOME", &distinct_cache_home);
    assert_eq!(
        git_cache_roots(),
        vec![
            distinct_cache_home.join("aurora").join("git"),
            std::env::temp_dir().join("aurora").join("git")
        ]
    );
    match original_cache_home {
        Some(value) => std::env::set_var("XDG_CACHE_HOME", value),
        None => std::env::remove_var("XDG_CACHE_HOME"),
    }

    {
        let _xdg_home = EnvVarGuard::remove("XDG_CACHE_HOME");
        let _home = EnvVarGuard::remove("HOME");
        assert_eq!(
            git_cache_root(),
            std::env::temp_dir().join("aurora").join("git")
        );
    }

    let cache_home_file = temp.write("cache-home-file", "not a directory");
    {
        let _cache_home = EnvVarGuard::set("XDG_CACHE_HOME", &cache_home_file);
        let cache_error = ensure_git_checkout("https://example.com/aurora-test.git", "abcdef0")
            .expect_err("file cache homes should surface cache directory errors");
        assert!(cache_error
            .message
            .contains("failed to create git cache directory"));
    }

    let failure = run_git_command(None, vec!["not-a-real-aurora-test-command".to_string()])
        .expect_err("invalid git subcommands should report status failure");
    assert!(failure
        .message
        .contains("git not-a-real-aurora-test-command"));

    #[cfg(unix)]
    {
        let mut command = Command::new("sh");
        command.args(["-c", "printf command-ok"]);
        let output =
            run_command_with_timeout(command, "stdout-only-success", StdDuration::from_secs(1))
                .expect("successful commands should collect stdout");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "command-ok");

        let spawn_error = run_command_with_timeout(
            Command::new("__definitely_missing_aurora_test_command__"),
            "missing-command",
            StdDuration::from_secs(1),
        )
        .expect_err("missing commands should surface spawn failures");
        assert!(spawn_error.message.contains("failed to run"));

        let panic_pipe =
            thread::spawn(|| -> std::io::Result<Vec<u8>> { panic!("simulated reader panic") });
        let panic_error = join_command_pipe(panic_pipe, "panic-reader", "stdout")
            .expect_err("panicking reader threads should surface collection errors");
        assert!(panic_error.message.contains("failed to collect"));

        let read_error_pipe = thread::spawn(|| -> std::io::Result<Vec<u8>> {
            Err(std::io::Error::other("simulated read failure"))
        });
        let read_error = join_command_pipe(read_error_pipe, "error-reader", "stderr")
            .expect_err("reader thread IO errors should be surfaced");
        assert!(read_error.message.contains("failed to read"));

        let stdout_failure = run_git_command(
            None,
            vec![
                "-c".to_string(),
                "alias.aurora-stdout-fail=!f() { printf stdout-only; exit 7; }; f".to_string(),
                "aurora-stdout-fail".to_string(),
            ],
        )
        .expect_err("git failures with stdout-only diagnostics should surface stdout");
        assert!(
            stdout_failure.message.contains("stdout-only"),
            "unexpected stdout-only failure: {}",
            stdout_failure.message
        );
    }
}

#[cfg(unix)]
#[test]
fn package_unix_cache_helpers_reject_symlinked_manifest_and_missing_dirs() {
    let temp = TempDir::new("aurora-package-unix-cache-edges");
    let missing = temp.path.join("missing-checkout");
    assert!(read_cached_git_revision(&missing)
        .expect("missing checkout dirs should behave like missing cache markers")
        .is_none());

    let checkout = temp.path.join("checkout");
    fs::create_dir_all(&checkout).expect("failed to create checkout");
    assert!(read_cached_git_revision(&checkout)
        .expect("missing cache markers should be optional")
        .is_none());
    let target_manifest = temp.path.join("outside.toml");
    fs::write(&target_manifest, "[package]\nname = \"pkg\"\n").expect("manifest target");
    std::os::unix::fs::symlink(&target_manifest, checkout.join(MANIFEST_NAME))
        .expect("failed to create symlinked manifest");
    let symlinked_manifest = git_checkout_contains_required_files(&checkout)
        .expect_err("symlinked manifests should fail");
    assert!(symlinked_manifest.message.contains("manifest is symlinked"));

    let manifest_dir_checkout = temp.path.join("manifest-dir-checkout");
    fs::create_dir_all(manifest_dir_checkout.join(MANIFEST_NAME))
        .expect("failed to create directory manifest");
    assert!(
        !git_checkout_contains_required_files(&manifest_dir_checkout)
            .expect("directory manifests should not count as checkout manifests")
    );

    let marker_checkout = temp.path.join("marker-checkout");
    fs::create_dir_all(&marker_checkout).expect("failed to create marker checkout");
    fs::write(
        marker_checkout.join(MANIFEST_NAME),
        "[package]\nname = \"pkg\"\n",
    )
    .expect("failed to write marker manifest");
    fs::write(marker_checkout.join(".aurora-cache-rev"), "abcdef0\n")
        .expect("failed to write revision marker");
    assert!(cached_git_checkout_matches_rev(&marker_checkout, "abcdef0")
        .expect("matching revision markers should be accepted"));
    assert!(
        !cached_git_checkout_matches_rev(&marker_checkout, "1234567")
            .expect("mismatched revision markers should be rejected")
    );

    let dir_fd = open_nofollow_dir_fd(&checkout)
        .expect("checkout dir should open")
        .expect("checkout dir should exist");
    let bad_marker = open_nofollow_file_at(&dir_fd, "bad\0name", &checkout)
        .expect_err("interior NUL marker names should fail");
    assert!(bad_marker.message.contains("interior NUL byte"));

    let nul_dir =
        open_nofollow_dir_fd(Path::new("bad\0path")).expect_err("NUL paths should fail early");
    assert!(nul_dir.message.contains("interior NUL byte"));

    let regular_file = temp.write("regular-file", "not a directory");
    let regular_error =
        open_nofollow_dir_fd(&regular_file).expect_err("regular files are not checkout dirs");
    assert!(regular_error
        .message
        .contains("failed to inspect git checkout directory"));

    let marker_dir_checkout = temp.path.join("marker-dir-checkout");
    fs::create_dir_all(marker_dir_checkout.join(".aurora-cache-rev"))
        .expect("failed to create directory marker");
    let marker_error = read_cached_git_revision(&marker_dir_checkout)
        .expect_err("directory cache markers should fail while reading");
    assert!(
        marker_error.message.contains("git revision marker")
            || marker_error.message.contains("git checkout marker"),
        "unexpected marker error: {}",
        marker_error.message
    );
}
