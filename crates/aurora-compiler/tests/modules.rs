use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_compiler::{check_path, run_path, run_path_via_mir};

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

#[test]
fn from_import_runs_public_function_across_modules() {
    let temp = TempDir::new("aurora-modules-from-import");
    temp.write(
        "helpers/math.au",
        r#"public def add(left: int32, right: int32) -> int32:
    return left + right
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from helpers.math import add

def main() -> int32:
    print(add(left=2, right=3))
    return 0
"#,
    );

    let output = run_path(&main_path).expect("module import should run");
    assert_eq!(output.stdout, "5\n");

    let mir_output = run_path_via_mir(&main_path).expect("module import should run via MIR");
    assert_eq!(mir_output.stdout, "5\n");
}

#[test]
fn dotted_import_binds_module_namespace() {
    let temp = TempDir::new("aurora-modules-dotted-import");
    temp.write(
        "helpers/math.au",
        r#"public def double(value: int32) -> int32:
    return value * 2
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"import helpers.math

def main() -> int32:
    print(helpers.math.double(value=4))
    return 0
"#,
    );

    let output = run_path(&main_path).expect("dotted module import should run");
    assert_eq!(output.stdout, "8\n");

    let mir_output =
        run_path_via_mir(&main_path).expect("dotted module import should run via MIR");
    assert_eq!(mir_output.stdout, "8\n");
}

#[test]
fn importing_private_top_level_function_fails() {
    let temp = TempDir::new("aurora-modules-private-import");
    temp.write(
        "helpers/secret.au",
        r#"def hidden() -> int32:
    return 7
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from helpers.secret import hidden

def main() -> int32:
    print(hidden())
    return 0
"#,
    );

    let error = check_path(&main_path).expect_err("private import should fail");
    assert!(
        error
            .to_string()
            .contains("item `hidden` is private in module `helpers.secret`"),
        "unexpected diagnostic: {}",
        error
    );
}

#[test]
fn calling_private_method_from_another_module_fails() {
    let temp = TempDir::new("aurora-modules-private-method");
    temp.write(
        "helpers/counter.au",
        r#"public class Counter:
    public value: int32

    public def read(borrow self) -> int32:
        return self.value

    def hidden(borrow self) -> int32:
        return self.value
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from helpers.counter import Counter

def main() -> int32:
    counter = Counter(value=1)
    print(counter.hidden())
    return 0
"#,
    );

    let error = check_path(&main_path).expect_err("private method call should fail");
    assert!(
        error
            .to_string()
            .contains("method `hidden` is private on `Counter`"),
        "unexpected diagnostic: {}",
        error
    );
}
