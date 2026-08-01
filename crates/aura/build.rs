use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git_output(repository: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_path(repository: &Path, name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(git_output(repository, &["rev-parse", "--git-path", name])?);
    Some(if path.is_absolute() {
        path
    } else {
        repository.join(path)
    })
}

fn main() {
    println!("cargo:rerun-if-env-changed=AURA_BUILD_COMMIT");

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let repository = manifest.join("../..");
    if let Some(head) = git_path(&repository, "HEAD") {
        println!("cargo:rerun-if-changed={}", head.display());
    }
    if let Some(symbolic_ref) = git_output(&repository, &["symbolic-ref", "-q", "HEAD"]) {
        if let Some(reference) = git_path(&repository, &symbolic_ref) {
            println!("cargo:rerun-if-changed={}", reference.display());
        }
        if let Some(packed_refs) = git_path(&repository, "packed-refs") {
            println!("cargo:rerun-if-changed={}", packed_refs.display());
        }
    }

    let commit = env::var("AURA_BUILD_COMMIT").ok().or_else(|| {
        git_output(
            &repository,
            &["rev-parse", "--verify", "--short=12", "HEAD^{commit}"],
        )
    });
    let commit = commit.unwrap_or_else(|| {
        panic!(
            "Aura builds require a Git commit identity; build in a Git checkout or set AURA_BUILD_COMMIT"
        )
    });
    let commit = commit.trim();
    assert!(
        commit.len() >= 12 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "AURA_BUILD_COMMIT must contain at least 12 hexadecimal digits"
    );
    println!("cargo:rustc-env=AURA_BUILD_COMMIT={}", &commit[..12]);
}
