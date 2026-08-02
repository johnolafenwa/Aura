use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use aura_compiler::{check_path, check_source, parse_source, run_path, run_source};

#[test]
fn parse_pass_fixtures_parse() {
    for fixture in fixture_files("parse-pass") {
        let source = read(&fixture);
        parse_source(&source)
            .unwrap_or_else(|error| panic!("{} should parse: {}", fixture.display(), error));
    }
}

#[test]
fn parse_fail_fixtures_match_expected_diagnostics() {
    for fixture in fixture_files("parse-fail") {
        let source = read(&fixture);
        let error = match parse_source(&source) {
            Ok(_) => panic!("{} should fail to parse", fixture.display()),
            Err(error) => error,
        };
        let expected = read_expected(&fixture, "diag");
        let rendered = error.render_with_source(&display_path(&fixture), &source);
        assert_eq!(
            normalize_newlines(&rendered),
            normalize_newlines(&expected),
            "unexpected diagnostic for {}",
            fixture.display()
        );
    }
}

#[test]
fn check_pass_fixtures_type_check() {
    for fixture in fixture_files("check-pass") {
        let source = read(&fixture);
        check_source(&source)
            .unwrap_or_else(|error| panic!("{} should type-check: {}", fixture.display(), error));
    }
}

#[test]
fn check_fail_fixtures_match_expected_diagnostics() {
    for fixture in fixture_files("check-fail") {
        let source = read(&fixture);
        let error = match check_source(&source) {
            Ok(_) => panic!("{} should fail to type-check", fixture.display()),
            Err(error) => error,
        };
        let expected = read_expected(&fixture, "diag");
        let rendered = error.render_with_source(&display_path(&fixture), &source);
        assert_eq!(
            normalize_newlines(&rendered),
            normalize_newlines(&expected),
            "unexpected diagnostic for {}",
            fixture.display()
        );
    }
}

#[test]
fn check_path_pass_fixtures_type_check_with_imports() {
    for fixture in fixture_files("check-path-pass") {
        check_path(&fixture)
            .unwrap_or_else(|error| panic!("{} should type-check: {}", fixture.display(), error));
    }
}

#[test]
fn check_path_fail_fixtures_match_expected_diagnostics() {
    for fixture in fixture_files("check-path-fail") {
        let source = read(&fixture);
        let error = match check_path(&fixture) {
            Ok(_) => panic!("{} should fail to type-check", fixture.display()),
            Err(error) => error,
        };
        let expected = read_expected(&fixture, "diag");
        let rendered = error.render_with_source(&display_path(&fixture), &source);
        assert_eq!(
            normalize_newlines(&normalize_workspace_path(&rendered)),
            normalize_newlines(&expected),
            "unexpected diagnostic for {}",
            fixture.display()
        );
    }
}

#[test]
fn python_migration_hint_fixtures_match_expected_messages_and_codes() {
    for fixture in fixture_files("python-hints") {
        let source = read(&fixture);
        let error = match check_source(&source) {
            Ok(_) => panic!("{} should produce a migration hint", fixture.display()),
            Err(error) => error,
        };
        assert_eq!(
            error.message,
            read_expected(&fixture, "diag").trim_end(),
            "unexpected migration hint for {}",
            fixture.display()
        );
        assert_eq!(
            error.code,
            read_expected(&fixture, "code").trim_end(),
            "unexpected diagnostic code for {}",
            fixture.display()
        );
    }
}

#[test]
fn run_pass_fixtures_match_expected_stdout() {
    for fixture in fixture_files("run-pass") {
        let output = run_path_on_large_stack(fixture.clone())
            .unwrap_or_else(|error| panic!("{} should run: {}", fixture.display(), error));
        let expected = read_expected(&fixture, "stdout");
        assert_eq!(
            normalize_newlines(&output.stdout),
            normalize_newlines(&expected),
            "unexpected stdout for {}",
            fixture.display()
        );
    }
}

#[test]
fn run_fail_fixtures_match_expected_diagnostics() {
    for fixture in fixture_files("run-fail") {
        let source = read(&fixture);
        let error = match run_source_on_large_stack(source.clone()) {
            Ok(output) => panic!(
                "{} should fail at runtime, but produced stdout:\n{}",
                fixture.display(),
                output.stdout
            ),
            Err(error) => error,
        };
        let expected = read_expected(&fixture, "diag");
        let rendered = error.render_with_source(&display_path(&fixture), &source);
        assert_eq!(
            normalize_newlines(&rendered),
            normalize_newlines(&expected),
            "unexpected runtime diagnostic for {}",
            fixture.display()
        );
    }
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture_files(category: &str) -> Vec<PathBuf> {
    let mut files = fs::read_dir(fixture_root().join(category))
        .unwrap_or_else(|error| panic!("failed to read fixture category `{}`: {}", category, error))
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("au"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {}", path.display(), error))
}

fn read_expected(path: &Path, extension: &str) -> String {
    let expected_path = path.with_extension(extension);
    read(&expected_path)
}

fn display_path(path: &Path) -> String {
    path.strip_prefix(workspace_root())
        .unwrap_or(path)
        .display()
        .to_string()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap()
        .to_path_buf()
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}

fn normalize_workspace_path(text: &str) -> String {
    let prefix = format!(
        "{}{}",
        workspace_root().display(),
        std::path::MAIN_SEPARATOR
    );
    text.replace(&prefix, "")
}

fn run_path_on_large_stack(path: PathBuf) -> aura_compiler::Result<aura_compiler::RunOutput> {
    thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || run_path(&path))
        .unwrap_or_else(|error| panic!("failed to spawn runtime fixture thread: {}", error))
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}

fn run_source_on_large_stack(source: String) -> aura_compiler::Result<aura_compiler::RunOutput> {
    thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || run_source(&source))
        .unwrap_or_else(|error| panic!("failed to spawn runtime fixture thread: {}", error))
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}
