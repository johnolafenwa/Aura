use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_compiler::{
    analyze_path_source, check_path, run_path, run_path_via_mir,
    update_git_dependencies_in_working_dir,
};

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

struct GitRepo {
    path: PathBuf,
}

impl GitRepo {
    fn init(temp: &TempDir, relative: &str, package_name: &str, files: &[(&str, &str)]) -> Self {
        let path = temp.path().join(relative);
        fs::create_dir_all(path.join("src")).expect("failed to create git package src");
        fs::write(
            path.join("Aurora.toml"),
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
        repo.git(&["config", "user.name", "Aurora Tests"]);
        repo.git(&["config", "user.email", "aurora-tests@example.com"]);
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
        self.rev_parse("HEAD")
    }

    fn rev_parse(&self, rev: &str) -> String {
        self.git(&["rev-parse", rev])
    }

    fn create_tag(&self, name: &str) -> String {
        self.git(&["tag", name]);
        self.rev_parse(name)
    }

    fn create_branch(&self, name: &str) -> String {
        self.git(&["checkout", "-b", name]);
        self.rev_parse("HEAD")
    }
}

#[test]
fn manifest_rooted_src_package_resolves_local_and_path_dependencies_and_writes_lockfile() {
    let temp = TempDir::new("aurora-packages-manifest-path-deps");
    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { path = "../util" }
"#,
    );
    temp.write(
        "app/src/helpers/math.au",
        r#"public def triple(value: int32) -> int32:
    return value * 3
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
        "util/Aurora.toml",
        r#"[package]
name = "util"
version = "0.1.0"
edition = "2026"
"#,
    );
    temp.write(
        "util/src/helpers/core.au",
        r#"public def scale(value: int32) -> int32:
    return value * 2
"#,
    );
    temp.write(
        "util/src/math.au",
        r#"import helpers.core

public def double(value: int32) -> int32:
    return helpers.core.scale(value=value)
"#,
    );

    let program = check_path(&main_path).expect("manifest-aware package should type-check");
    assert_eq!(program.module_name, "main");
    assert!(
        program.module_registry.contains_key("helpers.math"),
        "local src-root module should be registered"
    );
    assert!(
        program.module_registry.contains_key("util.math"),
        "dependency module should be registered with package prefix"
    );
    assert!(
        program.module_registry.contains_key("util.helpers.core"),
        "dependency internal modules should keep the dependency package prefix"
    );

    let output = run_path(&main_path).expect("manifest-aware package should run");
    assert_eq!(output.stdout, "12\n");

    let mir_output =
        run_path_via_mir(&main_path).expect("manifest-aware package should run via MIR");
    assert_eq!(mir_output.stdout, "12\n");

    let source = fs::read_to_string(&main_path).expect("main source should be readable");
    let analysis = analyze_path_source(&main_path, &source);
    assert!(
        analysis.diagnostics.is_empty(),
        "analysis should remain clean for manifest-aware packages: {:?}",
        analysis.diagnostics
    );

    let lockfile = temp.path.join("app/Aurora.lock");
    let lockfile_source = fs::read_to_string(&lockfile)
        .expect("manifest-aware package load should write a local lockfile");
    assert!(
        lockfile_source.contains("name = \"app\""),
        "lockfile should record the root package"
    );
    assert!(
        lockfile_source.contains("name = \"util\""),
        "lockfile should record resolved path dependencies"
    );
}

#[test]
fn workspace_member_packages_resolve_dependencies_and_write_workspace_lockfile() {
    let temp = TempDir::new("aurora-packages-workspace");
    temp.write(
        "Aurora.toml",
        r#"[workspace]
members = ["app", "util"]
"#,
    );
    temp.write(
        "app/Aurora.toml",
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

def main() -> int32:
    print(util.math.double(value=4))
    return 0
"#,
    );
    temp.write(
        "util/Aurora.toml",
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

    let output = run_path(&main_path).expect("workspace member package should run");
    assert_eq!(output.stdout, "8\n");

    let workspace_lockfile = temp.path.join("Aurora.lock");
    assert!(
        workspace_lockfile.exists(),
        "workspace-root lockfile should be generated"
    );
    assert!(
        !temp.path.join("app/Aurora.lock").exists(),
        "workspace members should not each write their own lockfile"
    );
}

#[test]
fn version_only_dependencies_report_clear_unsupported_error() {
    let temp = TempDir::new("aurora-packages-unsupported-version-deps");
    let main_path = temp.write(
        "app/src/main.au",
        r#"import util.math

def main() -> int32:
    return 0
"#,
    );
    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = "0.1.0"
"#,
    );

    let error = check_path(&main_path).expect_err("version-only dependencies should not resolve");
    assert!(
        error
            .message
            .contains("version-only dependencies are not supported yet"),
        "unexpected error message: {}",
        error.message
    );
    assert!(
        error.message.contains("util"),
        "unsupported dependency error should name the dependency: {}",
        error.message
    );
}

#[test]
fn transitive_path_dependencies_resolve_through_the_package_graph() {
    let temp = TempDir::new("aurora-packages-transitive-path-deps");
    temp.write(
        "app/Aurora.toml",
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

def main() -> int32:
    print(util.math.answer())
    return 0
"#,
    );
    temp.write(
        "util/Aurora.toml",
        r#"[package]
name = "util"
version = "0.1.0"
edition = "2026"

[dependencies]
jsonx = { path = "../jsonx" }
"#,
    );
    temp.write(
        "util/src/math.au",
        r#"import jsonx.parse

public def answer() -> int32:
    return jsonx.parse.add_one(value=41)
"#,
    );
    temp.write(
        "jsonx/Aurora.toml",
        r#"[package]
name = "jsonx"
version = "0.1.0"
edition = "2026"
"#,
    );
    temp.write(
        "jsonx/src/parse.au",
        r#"public def add_one(value: int32) -> int32:
    return value + 1
"#,
    );

    let program = check_path(&main_path).expect("transitive path dependencies should type-check");
    assert!(
        program.module_registry.contains_key("util.math"),
        "direct dependency module should be present in the registry"
    );
    assert!(
        program.module_registry.contains_key("jsonx.parse"),
        "transitive dependency module should be present in the registry"
    );

    let output = run_path(&main_path).expect("transitive path dependencies should run");
    assert_eq!(output.stdout, "42\n");

    let lockfile_source =
        fs::read_to_string(temp.path.join("app/Aurora.lock")).expect("lockfile should exist");
    assert!(
        lockfile_source.contains("name = \"jsonx\""),
        "lockfile should capture transitive dependencies"
    );
}

#[test]
fn git_dependencies_default_to_main_and_use_the_lockfile_pinned_revision() {
    let temp = TempDir::new("aurora-packages-git-main");
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
    let initial_rev = repo.rev_parse("HEAD");

    temp.write(
        "app/Aurora.toml",
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

    let output = run_path(&main_path).expect("git dependency on default main should run");
    assert_eq!(output.stdout, "6\n");
    let mir_output =
        run_path_via_mir(&main_path).expect("git dependency on default main should run via MIR");
    assert_eq!(mir_output.stdout, "6\n");

    let lockfile_path = temp.path.join("app/Aurora.lock");
    let initial_lockfile =
        fs::read_to_string(&lockfile_path).expect("git dependency should write a lockfile");
    assert!(initial_lockfile.contains("source = \"git\""));
    assert!(initial_lockfile.contains("branch = \"main\""));
    assert!(initial_lockfile.contains(&format!("rev = \"{}\"", initial_rev)));

    temp.write(
        "util-repo/src/math.au",
        r#"public def double(value: int32) -> int32:
    return value * 4
"#,
    );
    let updated_rev = repo.commit_all("change-main");
    assert_ne!(updated_rev, initial_rev, "repo should have advanced");

    let locked_output =
        run_path(&main_path).expect("existing lockfile should keep the original git revision");
    assert_eq!(locked_output.stdout, "6\n");
    let lockfile_after =
        fs::read_to_string(&lockfile_path).expect("lockfile should remain readable");
    assert_eq!(lockfile_after, initial_lockfile);
}

#[test]
fn git_dependencies_support_explicit_branch_selection() {
    let temp = TempDir::new("aurora-packages-git-branch");
    let repo = GitRepo::init(
        &temp,
        "util-repo",
        "util",
        &[(
            "src/math.au",
            r#"public def value() -> int32:
    return 4
"#,
        )],
    );
    repo.create_branch("release");
    temp.write(
        "util-repo/src/math.au",
        r#"public def value() -> int32:
    return 9
"#,
    );
    let release_rev = repo.commit_all("release-change");
    repo.git(&["checkout", "main"]);

    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { git = "../util-repo", branch = "release" }
"#,
    );
    let main_path = temp.write(
        "app/src/main.au",
        r#"import util.math

def main() -> int32:
    print(util.math.value())
    return 0
"#,
    );

    let output = run_path(&main_path).expect("git dependency branch selector should run");
    assert_eq!(output.stdout, "9\n");

    let lockfile = fs::read_to_string(temp.path.join("app/Aurora.lock"))
        .expect("branch-selected git dependency should write a lockfile");
    assert!(lockfile.contains("branch = \"release\""));
    assert!(lockfile.contains(&format!("rev = \"{}\"", release_rev)));
}

#[test]
fn git_dependencies_support_explicit_tag_selection() {
    let temp = TempDir::new("aurora-packages-git-tag");
    let repo = GitRepo::init(
        &temp,
        "util-repo",
        "util",
        &[(
            "src/math.au",
            r#"public def value() -> int32:
    return 7
"#,
        )],
    );
    let tag_rev = repo.create_tag("v0.1.0");

    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { git = "../util-repo", tag = "v0.1.0" }
"#,
    );
    let main_path = temp.write(
        "app/src/main.au",
        r#"import util.math

def main() -> int32:
    print(util.math.value())
    return 0
"#,
    );

    let output = run_path(&main_path).expect("git dependency tag selector should run");
    assert_eq!(output.stdout, "7\n");

    let lockfile = fs::read_to_string(temp.path.join("app/Aurora.lock"))
        .expect("tag-selected git dependency should write a lockfile");
    assert!(lockfile.contains("tag = \"v0.1.0\""));
    assert!(lockfile.contains(&format!("rev = \"{}\"", tag_rev)));
}

#[test]
fn git_dependency_manifest_rejects_mutually_exclusive_source_fields() {
    let temp = TempDir::new("aurora-packages-git-invalid");
    let main_path = temp.write(
        "app/src/main.au",
        r#"def main() -> int32:
    return 0
"#,
    );
    temp.write(
        "app/Aurora.toml",
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { path = "../util", git = "https://example.com/util.git" }
"#,
    );

    let error = check_path(&main_path).expect_err("path and git should be mutually exclusive");
    assert!(
        error
            .message
            .contains("must choose exactly one dependency source"),
        "unexpected error message: {}",
        error.message
    );
}

#[test]
fn update_git_dependencies_refreshes_targeted_and_all_branch_dependencies() {
    let temp = TempDir::new("aurora-packages-git-update");
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
        "app/Aurora.toml",
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

    let initial = run_path(&main_path).expect("initial git package should run");
    assert_eq!(initial.stdout, "12\n");

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

    let specific = update_git_dependencies_in_working_dir(&temp.path.join("app"), Some("util"))
        .expect("specific git dependency update should succeed");
    assert_eq!(specific.updated_packages, vec!["util".to_string()]);
    let after_specific =
        run_path(&main_path).expect("specific update should keep package runnable");
    assert_eq!(after_specific.stdout, "14\n");

    let lockfile_after_specific =
        fs::read_to_string(temp.path.join("app/Aurora.lock")).expect("lockfile should exist");
    assert!(lockfile_after_specific.contains(&new_util_rev));
    assert!(!lockfile_after_specific.contains(&new_jsonx_rev));

    let update_all = update_git_dependencies_in_working_dir(&temp.path.join("app"), None)
        .expect("all git dependency update should succeed");
    assert_eq!(update_all.updated_packages, vec!["jsonx".to_string()]);

    let after_all = run_path(&main_path).expect("full update should keep package runnable");
    assert_eq!(after_all.stdout, "24\n");
    let final_lockfile =
        fs::read_to_string(temp.path.join("app/Aurora.lock")).expect("lockfile should exist");
    assert!(final_lockfile.contains(&new_util_rev));
    assert!(final_lockfile.contains(&new_jsonx_rev));
}
