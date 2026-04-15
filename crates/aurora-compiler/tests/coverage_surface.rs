use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_compiler::{
    analyze_path_source, analyze_source, check_path, check_source, complete_path_source,
    complete_source, emit_host_native_object, lower_path_to_mir, lower_source_to_mir, run_path,
    run_path_via_mir, run_source, run_source_via_mir,
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

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("compiler crate should live under repo root")
        .parent()
        .expect("compiler crate should live under repo root")
        .to_path_buf()
}

fn line_and_character(source: &str, needle: &str) -> (usize, usize) {
    let offset = source
        .find(needle)
        .unwrap_or_else(|| panic!("expected to find `{needle}` in source"));
    let before = &source[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count();
    let character = before
        .rsplit_once('\n')
        .map(|(_, tail)| tail.chars().count())
        .unwrap_or_else(|| before.chars().count());
    (line, character + needle.chars().count())
}

#[test]
fn broad_surface_source_covers_public_compiler_entrypoints() {
    let source = r#"
trait Labelled:
    def label(borrow self) -> String

class Counter:
    value: int32

    def bump(borrow mut self, amount: int32 = 1) -> int32:
        self.value += amount
        return self.value

impl Labelled for Counter:
    def label(borrow self) -> String:
        return f"Counter({self.value})"

class Badge:
    text: String

impl Labelled for Badge:
    def label(borrow self) -> String:
        return self.text

class Resource:
    closed: bool = false

    def close(borrow mut self):
        self.closed = true

def worker(value: int32) -> int32:
    return value + 1

def summarize[T: Labelled](value: T) -> String:
    return value.label()

def parse_value(text: String) -> Result[int32, String]:
    return parse_int32(text)

def parse_and_offset(text: String) -> Result[int32, String]:
    parsed = try parse_value(text)
    return Result.Ok(parsed + 5)

def main() -> int32:
    text = "  Aurora Repo  "
    trimmed = text.trim()
    words = trimmed.split(" ")
    print(trimmed.len())
    print(trimmed.contains("Repo"))
    print(trimmed.starts_with("Aurora"))
    print(trimmed.ends_with("Repo"))
    print(trimmed.replace("Repo", "Lang"))
    print(trimmed.to_lower())
    print(trimmed.to_upper())
    print("-".join(words))
    print(trimmed.strip_prefix("Aurora "))
    print(trimmed.strip_suffix(" Repo"))

    print(parse_int32("12"))
    print(parse_int64("42"))
    print(parse_float64("2.5"))
    print(abs(-3))
    print(min(9, 4))
    print(max(9, 4))
    print(sqrt(9.0))
    mut total = 9.0
    total = total + 1.0
    total = total - 2.0
    total = total * 3.0
    total = total / 2.0
    total = total % 2.0
    print(total)
    if total > 0.0 and total >= 0.0 and total < 10.0 and total <= 10.0 and total != 4.5 and total == total:
        print("float-ok")
    rounded = total as int32
    print(rounded)
    print((rounded as float64))
    parsed_result = parse_and_offset("7")
    print(parsed_result)

    mut values = [1, 2]
    values.push(3)
    print(values.get(1))
    print(values[0])
    print(values.set(0, 9))
    values[1] = 8
    print(values.remove(0))
    print(values.swap(0, 0))
    print(values.contains(8))
    print(values.insert(1, 7))
    values.reverse()
    values.extend([5, 6])
    print(values == [3, 7, 8, 5, 6])
    values.clear()
    print(values.is_empty())

    mut counts = {"a": 1}
    print(counts.get("a"))
    print(counts["a"])
    print(counts.set("a", 2))
    counts["b"] = 3
    print(counts.remove("a"))
    print(counts.contains_key("b"))
    print(counts.keys().len())
    print(counts.values().len())
    print(counts.items().len())
    print(counts.entries().len())
    counts.extend({"c": 4})
    counts.clear()
    print(counts.is_empty())

    mut seen = Set{"x"}
    print(seen.insert("y"))
    print(seen.remove("x"))
    print(seen.contains("y"))
    print(seen.len())

    mut counter = Counter(value=1)
    print(counter.bump())
    print(summarize(counter))
    print(summarize(Badge(text="badge")))

    jobs: Channel[int32] = channel()
    print(jobs.send(7))
    print(jobs.recv())
    jobs.close()

    task = spawn worker(4)
    print(task.join())

    with Resource() as resource:
        print(resource.closed)

    match 1:
        case 0:
            print("zero")
        case 1:
            print("one")
        case _:
            print("other")

    match "go":
        case "stop":
            print("stop")
        case "go":
            print("go")
        case _:
            print("other")

    match true:
        case false:
            print("no")
        case true:
            print("yes")

    return rounded
"#;

    let program = check_source(source).expect("broad source should type-check");
    let analysis = analyze_source(source);
    assert!(
        analysis.diagnostics.is_empty(),
        "analysis should stay clean: {:?}",
        analysis.diagnostics
    );

    let completion_source = r#"
def main() -> int32:
    mut values = [1, 2]
    values.
    return 0
"#;
    let (line, character) = line_and_character(completion_source, "values.");
    let completions = complete_source(completion_source, line, character, Some('.'))
        .expect("completion should work on collection receiver");
    let completion_names = completions
        .iter()
        .map(|item| item.name.as_str())
        .collect::<BTreeSet<_>>();
    assert!(completion_names.contains("push"));
    assert!(completion_names.contains("reverse"));

    let output = run_source(source).expect("broad source should run");
    let mir_output = run_source_via_mir(source).expect("broad source should run via MIR");
    assert_eq!(mir_output.stdout, output.stdout);
    assert!(!program.functions.is_empty());

    let mir = lower_source_to_mir(source).expect("broad source should lower to MIR");
    let object = emit_host_native_object(&mir).expect("broad source should emit a native object");
    assert!(!object.is_empty());
}

#[test]
fn path_surface_covers_modules_analysis_completion_and_direct_codegen() {
    let temp = TempDir::new("aurora-coverage-surface");
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
    temp.write(
        "helpers/factory.au",
        r#"from pkg.user import User

public def describe_user(name: String) -> String:
    user = User(label=name)
    return user.name()
"#,
    );
    let main_source = r#"import pkg.user
from helpers.factory import describe_user

def main() -> int32:
    user: pkg.user.User = pkg.user.User(label="Ada")
    print(describe_user(name=user.label.clone()))
    print(user.name())
    return 0
"#;
    let main_path = temp.write("main.au", main_source);

    let program = check_path(&main_path).expect("package program should type-check");
    let analysis = analyze_path_source(&main_path, main_source);
    assert!(
        analysis.diagnostics.is_empty(),
        "path analysis should stay clean: {:?}",
        analysis.diagnostics
    );

    let completion_source = r#"import pkg.user
from helpers.factory import describe_user

def main() -> int32:
    user: pkg.user.User = pkg.user.User(label="Ada")
    user.
    print(describe_user(name=user.label.clone()))
    return 0
"#;
    let (line, character) = line_and_character(completion_source, "user.");
    let completions =
        complete_path_source(&main_path, completion_source, line, character, Some('.'))
            .expect("path completion should recover through imports");
    let completion_names = completions
        .iter()
        .map(|item| item.name.as_str())
        .collect::<BTreeSet<_>>();
    assert!(!completion_names.is_empty());

    let output = run_path(&main_path).expect("package program should run");
    let mir_output = run_path_via_mir(&main_path).expect("package program should run via MIR");
    assert_eq!(output.stdout, "Ada\nAda\n");
    assert_eq!(mir_output.stdout, output.stdout);
    assert!(!program.module.items.is_empty());

    let mir = lower_path_to_mir(&main_path).expect("package program should lower to MIR");
    let object =
        emit_host_native_object(&mir).expect("package program should emit a native object");
    assert!(!object.is_empty());
}

#[test]
fn maintained_example_subset_runs_via_public_entrypoints_and_direct_codegen() {
    let root = repo_root();
    let examples = [
        "examples/collections/vec_polish.au",
        "examples/collections/map_basics.au",
        "examples/collections/set_basics.au",
        "examples/numbers/numeric_builtins.au",
        "examples/strings/string_methods.au",
        "examples/strings/string_parsing_and_formatting.au",
        "examples/concurrency/channels_spawn.au",
        "examples/concurrency/select_timeout_named.au",
        "examples/modules/trait_impl_imports.au",
        "examples/traits/operator_traits.au",
    ];

    for relative in examples {
        let path = root.join(relative);
        let source = fs::read_to_string(&path).expect("example source should exist");
        let analysis = analyze_path_source(&path, &source);
        assert!(
            analysis.diagnostics.is_empty(),
            "maintained example analysis should stay clean for {}: {:?}",
            path.display(),
            analysis.diagnostics
        );

        let output = run_path(&path).expect("maintained example should run");
        let mir_output = run_path_via_mir(&path).expect("maintained example should run via MIR");
        assert_eq!(mir_output.stdout, output.stdout, "{}", path.display());

        let mir = lower_path_to_mir(&path).expect("maintained example should lower to MIR");
        let object =
            emit_host_native_object(&mir).expect("maintained example should emit a native object");
        assert!(!object.is_empty(), "{}", path.display());
    }
}
