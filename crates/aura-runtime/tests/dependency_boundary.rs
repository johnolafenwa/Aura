//! The runtime must never depend on compiler-only crates, so nothing from the
//! parser, checker, or code generator can reach a user executable.

use std::process::Command;

const FORBIDDEN: &[&str] = &["aura-compiler", "inkwell", "llvm-sys"];

#[test]
fn runtime_normal_dependencies_exclude_compiler_crates() {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("CARGO_TERM_COLOR", "never")
        .args([
            "tree",
            "--offline",
            "-p",
            "aura-runtime",
            "-e",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .output()
        .expect("failed to run `cargo tree`");
    assert!(
        output.status.success(),
        "`cargo tree` failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8(output.stdout).expect("cargo tree output is UTF-8");
    assert!(
        tree.lines().any(|line| line.starts_with("aura-core ")),
        "{tree}"
    );
    let forbidden = tree
        .lines()
        .filter(|line| FORBIDDEN.iter().any(|name| line.starts_with(name)))
        .collect::<Vec<_>>();
    assert!(
        forbidden.is_empty(),
        "aura-runtime must not depend on compiler crates: {forbidden:?}"
    );
}
