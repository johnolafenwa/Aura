use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use aura_compiler::mir::Instruction;
use aura_compiler::{
    check_path, check_source, emit_host_native_debug_objects_with_metadata, lower_path_to_mir,
    lower_path_to_mir_with_source_locations, lower_path_with_source_to_mir_with_source_locations,
    parse_source, run_path, MirModule,
};

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

/// A debug build's markers change nothing else: with its `SourceLocation`s
/// and `SourceName`s taken out, every run-pass fixture lowers to the MIR a
/// normal build lowers, and its debug object emits.
#[test]
fn run_pass_fixtures_lower_the_same_mir_with_source_locations() {
    let fixtures = fixture_files("run-pass");
    on_large_stack(move || {
        for fixture in fixtures {
            let normal = lower_path_to_mir(&fixture)
                .unwrap_or_else(|error| panic!("{} should lower: {}", fixture.display(), error));
            let mut debug = lower_path_to_mir_with_source_locations(&fixture)
                .unwrap_or_else(|error| panic!("{} should lower: {}", fixture.display(), error));
            let from_source =
                lower_path_with_source_to_mir_with_source_locations(&fixture, &read(&fixture))
                    .unwrap_or_else(|error| {
                        panic!("{} should lower: {}", fixture.display(), error)
                    });
            assert_eq!(
                serde_json::to_string(&from_source).unwrap(),
                serde_json::to_string(&debug).unwrap(),
                "{} lowers alike from its path and its source",
                fixture.display()
            );
            let markers = remove_source_markers(&mut debug);
            assert!(markers > 0, "{} has no statements", fixture.display());
            assert_eq!(
                serde_json::to_string(&debug).unwrap(),
                serde_json::to_string(&normal).unwrap(),
                "debug lowering of {} differs beyond its markers",
                fixture.display()
            );
        }
    });
}

#[test]
fn run_pass_fixtures_emit_debug_objects() {
    let fixtures = fixture_files("run-pass");
    on_large_stack(move || {
        for fixture in fixtures {
            let debug = lower_path_to_mir_with_source_locations(&fixture)
                .unwrap_or_else(|error| panic!("{} should lower: {}", fixture.display(), error));
            let source = read(&fixture);
            let objects = emit_host_native_debug_objects_with_metadata(
                &debug,
                &fixture.display().to_string(),
                &source,
            )
            .unwrap_or_else(|error| panic!("{} should emit: {}", fixture.display(), error));
            assert_eq!(objects.len(), 1, "{} is one unit", fixture.display());
            assert!(
                contains(&objects[0], b"debug_line"),
                "{} has a line table",
                fixture.display()
            );
        }
    });
}

fn remove_source_markers(module: &mut MirModule) -> usize {
    let mut removed = 0;
    for function in module
        .functions
        .iter_mut()
        .chain(module.top_level.iter_mut())
    {
        for block in &mut function.blocks {
            let before = block.instructions.len();
            block.instructions.retain(|instruction| {
                !matches!(
                    instruction,
                    Instruction::SourceLocation { .. } | Instruction::SourceName { .. }
                )
            });
            removed += before - block.instructions.len();
        }
    }
    removed
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn on_large_stack(work: impl FnOnce() + Send + 'static) {
    thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(work)
        .unwrap_or_else(|error| panic!("failed to spawn fixture thread: {}", error))
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

#[test]
fn run_fail_fixtures_match_expected_diagnostics() {
    for fixture in fixture_files("run-fail") {
        let source = read(&fixture);
        let error = match run_path_on_large_stack(fixture.clone()) {
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
        // A run-fail fixture may also pin the output printed before the
        // failure, such as the cleanup trace of a trapping `with` body.
        let stdout_path = fixture.with_extension("stdout");
        if stdout_path.exists() {
            assert_eq!(
                normalize_newlines(error.partial_stdout().unwrap_or_default()),
                normalize_newlines(&read(&stdout_path)),
                "unexpected partial stdout for {}",
                fixture.display()
            );
        }
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
