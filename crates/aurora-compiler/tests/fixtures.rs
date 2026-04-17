use std::fs;
use std::path::{Path, PathBuf};

use aurora_compiler::{check_source, parse_source, run_source};

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
fn run_pass_fixtures_match_expected_stdout() {
    for fixture in fixture_files("run-pass") {
        let source = read(&fixture);
        let output = run_source(&source)
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
        let error = match run_source(&source) {
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
