use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_compiler::{analyze_path_source, check_path, run_path, run_path_via_mir};

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

    let mir_output = run_path_via_mir(&main_path).expect("dotted module import should run via MIR");
    assert_eq!(mir_output.stdout, "8\n");
}

#[test]
fn dotted_import_supports_public_classes_and_methods() {
    let temp = TempDir::new("aurora-modules-dotted-import-classes");
    temp.write(
        "pkg/types.au",
        r#"public class Counter:
    public value: int32

    public def read(borrow self) -> int32:
        return self.value
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"import pkg.types

def main() -> int32:
    counter = pkg.types.Counter(value=4)
    print(counter.read())
    return 0
"#,
    );

    let output = run_path(&main_path).expect("dotted class import should run");
    assert_eq!(output.stdout, "4\n");

    let mir_output = run_path_via_mir(&main_path).expect("dotted class import should run via MIR");
    assert_eq!(mir_output.stdout, "4\n");
}

#[test]
fn dotted_import_supports_namespace_qualified_type_annotations() {
    let temp = TempDir::new("aurora-modules-qualified-annotations");
    temp.write(
        "pkg/types.au",
        r#"public class Counter:
    public value: int32

    public def read(borrow self) -> int32:
        return self.value
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"import pkg.types

def main() -> int32:
    counter: pkg.types.Counter = pkg.types.Counter(value=9)
    print(counter.read())
    return 0
"#,
    );

    let output = run_path(&main_path).expect("qualified type annotations should run");
    assert_eq!(output.stdout, "9\n");

    let mir_output =
        run_path_via_mir(&main_path).expect("qualified type annotations should run via MIR");
    assert_eq!(mir_output.stdout, "9\n");
}

#[test]
fn nested_package_module_can_be_checked_and_analyzed_directly() {
    let temp = TempDir::new("aurora-modules-nested-direct");
    temp.write(
        "pkg/named.au",
        r#"public trait Named:
    def name(borrow self) -> String
"#,
    );
    let user_path = temp.write(
        "pkg/user.au",
        r#"from pkg.named import Named

public class User:
    public label: String

impl Named for User:
    def name(borrow self) -> String:
        return self.label
"#,
    );

    let program = check_path(&user_path).expect("nested package module should type-check");
    assert_eq!(program.module_name, "pkg.user");

    let source = fs::read_to_string(&user_path).expect("user module source should be readable");
    let analysis = analyze_path_source(&user_path, &source);
    assert!(
        analysis.diagnostics.is_empty(),
        "analysis should not report false import errors: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn dotted_import_supports_enum_variants_and_qualified_match_patterns() {
    let temp = TempDir::new("aurora-modules-dotted-import-enums");
    temp.write(
        "pkg/types.au",
        r#"public enum Status:
    Ready
    Busy
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"import pkg.types

def main() -> int32:
    status = pkg.types.Status.Ready
    print(status == pkg.types.Status.Ready)
    match status:
        case pkg.types.Status.Ready:
            print(1)
        case pkg.types.Status.Busy:
            print(2)
    return 0
"#,
    );

    let output = run_path(&main_path).expect("dotted enum import should run");
    assert_eq!(output.stdout, "true\n1\n");

    let mir_output = run_path_via_mir(&main_path).expect("dotted enum import should run via MIR");
    assert_eq!(mir_output.stdout, "true\n1\n");
}

#[test]
fn imported_public_function_can_call_public_sibling_function() {
    let temp = TempDir::new("aurora-modules-sibling-call");
    temp.write(
        "helpers/math.au",
        r#"public def leaf() -> int32:
    return 42

public def wrapper() -> int32:
    return leaf()
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from helpers.math import wrapper

def main() -> int32:
    print(wrapper())
    return 0
"#,
    );

    let program = check_path(&main_path).expect("program should type-check");
    assert!(
        program
            .module_registry
            .get("helpers.math")
            .and_then(|namespace| namespace.all_functions.get("leaf"))
            .is_some(),
        "module registry should preserve helper function metadata"
    );

    let output = run_path(&main_path).expect("imported sibling function call should run");
    assert_eq!(output.stdout, "42\n");

    let mir_output =
        run_path_via_mir(&main_path).expect("imported sibling function call should run via MIR");
    assert_eq!(mir_output.stdout, "42\n");
}

#[test]
fn spawned_module_functions_and_associated_methods_run_across_modules() {
    let temp = TempDir::new("aurora-modules-spawn-targets");
    temp.write(
        "helpers/work.au",
        r#"public def add_one(value: int32) -> int32:
    return value + 1

public class Worker:
    public def run(value: int32) -> int32:
        return value + 2
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"import helpers.work

def main() -> int32:
    first = spawn helpers.work.add_one(4)
    second = spawn helpers.work.Worker.run(5)
    print(first.join())
    print(second.join())
    with task_group() as group:
        third = group.spawn(helpers.work.add_one, 6)
        fourth = group.spawn(helpers.work.Worker.run, 7)
        print(third.join())
        print(fourth.join())
    return 0
"#,
    );

    let output = run_path(&main_path).expect("spawned module call targets should run");
    assert_eq!(output.stdout, "5\n7\n7\n9\n");

    let mir_output =
        run_path_via_mir(&main_path).expect("spawned module call targets should run via MIR");
    assert_eq!(mir_output.stdout, "5\n7\n7\n9\n");
}

#[test]
fn imported_public_function_can_construct_public_class_and_call_method() {
    let temp = TempDir::new("aurora-modules-constructor-method");
    temp.write(
        "helpers/counter.au",
        r#"public class Counter:
    public value: int32

    public def read(borrow self) -> int32:
        return self.value

public def make() -> Counter:
    return Counter(value=4)

public def read_created() -> int32:
    return make().read()
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from helpers.counter import read_created

def main() -> int32:
    print(read_created())
    return 0
"#,
    );

    let output = run_path(&main_path).expect("imported constructor/method flow should run");
    assert_eq!(output.stdout, "4\n");

    let mir_output =
        run_path_via_mir(&main_path).expect("imported constructor/method flow should run via MIR");
    assert_eq!(mir_output.stdout, "4\n");
}

#[test]
fn transitive_reexported_imports_run_via_mir() {
    let temp = TempDir::new("aurora-modules-transitive-reexport");
    temp.write(
        "pkg/base.au",
        r#"public def answer() -> int32:
    return 42
"#,
    );
    temp.write(
        "pkg/reexport.au",
        r#"from pkg.base import answer

public def wrapped() -> int32:
    return answer()
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from pkg.reexport import wrapped

def main() -> int32:
    print(wrapped())
    return 0
"#,
    );

    let output = run_path(&main_path).expect("transitive re-export should run");
    assert_eq!(output.stdout, "42\n");

    let mir_output = run_path_via_mir(&main_path).expect("transitive re-export should run via MIR");
    assert_eq!(mir_output.stdout, "42\n");
}

#[test]
fn namespace_imports_inside_imported_modules_resolve_in_their_own_scope() {
    let temp = TempDir::new("aurora-modules-nested-namespace-scope");
    temp.write(
        "pkg/types.au",
        r#"public class Counter:
    public value: int32
"#,
    );
    temp.write(
        "pkg/helpers.au",
        r#"import pkg.types

public def make_counter() -> pkg.types.Counter:
    return pkg.types.Counter(value=7)
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from pkg.helpers import make_counter

def main() -> int32:
    counter = make_counter()
    print(counter.value)
    return 0
"#,
    );

    let output = run_path(&main_path).expect("namespace imports in imported modules should run");
    assert_eq!(output.stdout, "7\n");

    let mir_output = run_path_via_mir(&main_path)
        .expect("namespace imports in imported modules should run via MIR");
    assert_eq!(mir_output.stdout, "7\n");
}

#[test]
fn namespace_imports_inside_imported_modules_resolve_in_function_bodies() {
    let temp = TempDir::new("aurora-modules-nested-namespace-body");
    temp.write(
        "pkg/types.au",
        r#"public class Counter:
    public value: int32
"#,
    );
    temp.write(
        "pkg/helpers.au",
        r#"import pkg.types

public def read_value() -> int32:
    counter = pkg.types.Counter(value=7)
    return counter.value
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from pkg.helpers import read_value

def main() -> int32:
    print(read_value())
    return 0
"#,
    );

    let output =
        run_path(&main_path).expect("namespace imports in imported module bodies should run");
    assert_eq!(output.stdout, "7\n");

    let mir_output = run_path_via_mir(&main_path)
        .expect("namespace imports in imported module bodies should run via MIR");
    assert_eq!(mir_output.stdout, "7\n");
}

#[test]
fn imported_trait_impls_apply_across_module_boundaries() {
    let temp = TempDir::new("aurora-modules-imported-trait-impls");
    temp.write(
        "pkg/named.au",
        r#"public trait Named:
    def name(borrow self) -> String
"#,
    );
    temp.write(
        "pkg/user.au",
        r#"from pkg.named import Named

public class User:
    public label: String

impl Named for User:
    def name(borrow self) -> String:
        return self.label
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from pkg.named import Named
from pkg.user import User

def show[T: Named](value: T) -> String:
    return value.name()

def main() -> int32:
    print(show(value=User(label="Ada")))
    print(User(label="Ada").name())
    return 0
"#,
    );

    let output = run_path(&main_path).expect("imported trait impls should run");
    assert_eq!(output.stdout, "Ada\nAda\n");

    let mir_output = run_path_via_mir(&main_path).expect("imported trait impls should run via MIR");
    assert_eq!(mir_output.stdout, "Ada\nAda\n");
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
