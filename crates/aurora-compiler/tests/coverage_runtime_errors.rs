use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_compiler::{
    check_path_with_source, lower_path_with_source_to_mir, run_mir, run_path_with_source,
    run_source,
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
        fs::write(&path, source).expect("failed to write module source");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn assert_runtime_error_contains(source: &str, expected: &str) {
    let run_error = run_source(source).expect_err("source should fail at runtime");
    assert!(
        run_error.message.contains(expected),
        "public run error should contain `{expected}`, got `{}`",
        run_error.message
    );

    let mir = aurora_compiler::lower_source_to_mir(source).expect("source should lower to MIR");
    let mir_error = run_mir(&mir).expect_err("source should fail via MIR runtime");
    assert!(
        mir_error.message.contains(expected),
        "MIR runtime error should contain `{expected}`, got `{}`",
        mir_error.message
    );
}

#[test]
fn runtime_error_surface_covers_public_run_and_explicit_mir_failures() {
    assert_runtime_error_contains(
        r#"
def main() -> int32:
    return 1 // 0
"#,
        "division by zero",
    );

    assert_runtime_error_contains(
        r#"
def main() -> int32:
    values = [1, 2]
    print(values[9])
    return 0
"#,
        "out of bounds",
    );

    assert_runtime_error_contains(
        r#"
def main() -> int32:
    counts = {"a": 1}
    print(counts["missing"])
    return 0
"#,
        "was not present",
    );
}

#[test]
fn runtime_errors_unwind_with_resource_cleanups() {
    let source = r#"
class Resource:
    name: String

    def close(mut self):
        print("closed " + self.name)

def main() -> int32:
    with Resource(name="r1") as resource:
        print("inside")
        return 1 // 0
"#;

    let error = run_source(source).expect_err("division by zero should fail at runtime");
    assert!(
        error.message.contains("division by zero"),
        "unexpected runtime error: {}",
        error.message
    );
    assert_eq!(error.partial_stdout(), Some("inside\nclosed r1\n"));
}

#[test]
fn path_with_source_public_wrappers_cover_success_and_error_paths() {
    let temp = TempDir::new("aurora-path-with-source");
    temp.write(
        "helpers/math.au",
        r#"public def double(value: int32) -> int32:
    return value * 2
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from helpers.math import double

def main() -> int32:
    print(double(value=4))
    return 0
"#,
    );

    let override_source = r#"from helpers.math import double

def main() -> int32:
    print(double(value=6))
    return 0
"#;

    let program = check_path_with_source(&main_path, override_source)
        .expect("path-with-source check should resolve imports");
    assert_eq!(program.module_name, "main");

    let output =
        run_path_with_source(&main_path, override_source).expect("path-with-source should run");
    let mir = lower_path_with_source_to_mir(&main_path, override_source)
        .expect("path-with-source should lower to MIR");
    let mir_output = run_mir(&mir).expect("lowered path-with-source should run via MIR");
    assert_eq!(output.stdout, "12\n");
    assert_eq!(mir_output.stdout, output.stdout);
    assert!(!mir.functions.is_empty());

    let bad_override = r#"from helpers.math import double

def main() -> int32:
    return double()
"#;
    let error = check_path_with_source(&main_path, bad_override)
        .expect_err("invalid path-with-source override should report a checker error");
    assert!(error.message.contains("double"));
}
