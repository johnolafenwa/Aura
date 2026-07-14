use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_compiler::call::BuiltinMember;
use aurora_compiler::diag::Span;
use aurora_compiler::mir::{BasicBlock, MirFunction, MirModule, Terminator};
use aurora_compiler::sema::Type;
use aurora_compiler::{
    analyze_path_source, analyze_source, check_path, check_source, complete_path_source,
    complete_source, emit_host_native_object, emit_host_native_object_with_metadata,
    lower_path_to_mir, lower_source_to_mir, run_mir, run_path,
    run_path_with_source_and_stdout_sink, run_path_with_stdout_sink, run_source,
    run_source_with_stdout_sink, StdoutSink,
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

fn capture_stdout_sink() -> (Arc<Mutex<String>>, StdoutSink) {
    let captured = Arc::new(Mutex::new(String::new()));
    let sink_capture = captured.clone();
    let sink: StdoutSink = Arc::new(move |chunk| {
        sink_capture
            .lock()
            .expect("capture sink lock should not be poisoned")
            .push_str(chunk);
    });
    (captured, sink)
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
        return self.text.clone()

class Resource:
    closed: bool = false

    def close(borrow mut self):
        self.closed = true

def worker(value: int32) -> int32:
    return value + 1

def produce(queue: Queue[int32]) -> None:
    queue.put(11)
    queue.close()

def summarize[T: Labelled](value: T) -> String:
    return value.label()

def parse_value(text: String) -> Result[int32, String]:
    return parse_int32(text)

def parse_and_offset(text: String) -> Result[int32, String]:
    parsed = try parse_value(text)
    return Result.Ok(parsed + 5)

def print_int_option(value: Option[int32]) -> None:
    match value:
        case Some(inner):
            print(inner)
        case None:
            print(-1)

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

    mut values: Vec[int32] = [1, 2]
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
    print_int_option(values.get(0))
    mut range_total: int32 = 0
    for number in range(values.len()):
        range_total += number
    print(range_total)
    for value in borrow values:
        print(value)
    for value in borrow mut values:
        value += 1
    print(values[0])
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

    jobs = Queue[int32]()
    print(jobs.put(7))
    print(jobs.get())
    jobs.close()

    with TaskGroup() as group:
        task = group.start(worker, 4)
        print(task.result())

    stream = Queue[int32]()
    with TaskGroup() as group:
        group.start_soon(produce, stream)
        for item in stream:
            print(item)

    empty_any = Vec[Task[int32]]()
    match wait_any(empty_any, timeout=1ms):
        case WaitAny.Ready(index, value):
            print(index)
            print(value)
        case WaitAny.Error(index, message):
            print(index)
            print(message)
        case WaitAny.TimedOut:
            print("timedout")
        case WaitAny.Cancelled:
            print("cancelled")

    empty_all = Vec[Task[int32]]()
    match wait_all(empty_all, timeout=1ms):
        case WaitAll.Ready(results):
            print(results.len())
        case WaitAll.Error(index, message):
            print(index)
            print(message)
        case WaitAll.TimedOut:
            print("timedout")
        case WaitAll.Cancelled:
            print("cancelled")

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
    let mir = lower_source_to_mir(source).expect("broad source should lower to MIR");
    let mir_output = run_mir(&mir).expect("broad source MIR should run");
    assert_eq!(mir_output.stdout, output.stdout);
    assert!(!program.functions.is_empty());

    let object = emit_host_native_object(&mir).expect("broad source should emit a native object");
    assert!(!object.is_empty());
    let metadata_object = emit_host_native_object_with_metadata(&mir, "/tmp/broad.au", source)
        .expect("broad source should emit a metadata-backed native object");
    assert!(!metadata_object.is_empty());
}

#[test]
fn public_native_codegen_rejects_invalid_mir_surface() {
    let invalid_module = MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Unreachable,
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };
    let error = emit_host_native_object(&invalid_module)
        .expect_err("invalid MIR terminators should fail through the public native codegen API");
    assert!(
        error.contains("does not yet support MIR terminator"),
        "unexpected native codegen error: {error}"
    );
}

#[test]
fn public_stdout_sink_wrappers_capture_source_path_and_path_override_output() {
    let source = r#"
def main() -> int32:
    print("source")
    return 0
"#;
    let (captured_source, source_sink) = capture_stdout_sink();
    let source_output =
        run_source_with_stdout_sink(source, source_sink).expect("source sink wrapper should run");
    assert_eq!(source_output.stdout, "source\n");
    assert_eq!(
        captured_source
            .lock()
            .expect("captured source output should be readable")
            .as_str(),
        "source\n"
    );

    let temp = TempDir::new("aurora-stdout-sink");
    let main_path = temp.write(
        "main.au",
        r#"def main() -> int32:
    print("path")
    return 0
"#,
    );
    let (captured_path, path_sink) = capture_stdout_sink();
    let path_output =
        run_path_with_stdout_sink(&main_path, path_sink).expect("path sink wrapper should run");
    assert_eq!(path_output.stdout, "path\n");
    assert_eq!(
        captured_path
            .lock()
            .expect("captured path output should be readable")
            .as_str(),
        "path\n"
    );

    let override_source = r#"def main() -> int32:
    print("override")
    return 0
"#;
    let (captured_override, override_sink) = capture_stdout_sink();
    let override_output =
        run_path_with_source_and_stdout_sink(&main_path, override_source, override_sink)
            .expect("path-with-source sink wrapper should run");
    assert_eq!(override_output.stdout, "override\n");
    assert_eq!(
        captured_override
            .lock()
            .expect("captured override output should be readable")
            .as_str(),
        "override\n"
    );
}

#[test]
fn public_from_imports_cover_builtin_module_export_resolution() {
    let import_source = r#"
from fs import exists, File
from io import Error
from process import Stdio

def main() -> None:
    pass
"#;
    check_source(import_source).expect("builtin function class and enum imports should resolve");

    let run_source_text = r#"
from fs import exists

def main() -> int32:
    print(exists(path="/path/that/should/not/exist"))
    return 0
"#;
    let output = run_source(run_source_text).expect("builtin from-imported function should run");
    assert_eq!(output.stdout, "false\n");

    let missing_export = check_source(
        r#"from fs import Missing

def main() -> None:
    pass
"#,
    )
    .expect_err("missing builtin export should fail through from-import resolution");
    assert!(
        missing_export
            .message
            .contains("module `fs` has no export named `Missing`"),
        "unexpected builtin import diagnostic: {}",
        missing_export.message
    );

    let duplicate_source = check_source(
        r#"from fs import exists
from fs import exists

def main() -> None:
    pass
"#,
    )
    .expect_err("duplicate builtin source imports should fail");
    assert!(
        duplicate_source
            .message
            .contains("duplicate import binding `exists`"),
        "unexpected duplicate builtin source import diagnostic: {}",
        duplicate_source.message
    );

    let temp = TempDir::new("aurora-builtin-from-import-duplicate");
    let duplicate_path = temp.write(
        "main.au",
        r#"from fs import exists
from fs import exists

def main() -> None:
    pass
"#,
    );
    let duplicate_path_error =
        check_path(&duplicate_path).expect_err("duplicate builtin path imports should fail");
    assert!(
        duplicate_path_error
            .message
            .contains("duplicate import binding `exists`"),
        "unexpected duplicate builtin path import diagnostic: {}",
        duplicate_path_error.message
    );
}

#[test]
fn imported_main_function_is_not_treated_as_the_local_entrypoint() {
    let temp = TempDir::new("aurora-imported-main-entrypoint");
    temp.write(
        "helpers/entry.au",
        r#"public def main(value: int32) -> int32:
    return value + 3
"#,
    );
    let main_path = temp.write(
        "main.au",
        r#"from helpers.entry import main

print(main(4))
"#,
    );

    let output = run_path(&main_path).expect("imported main should be callable from a script");
    assert_eq!(output.stdout, "7\n");

    let mir = lower_path_to_mir(&main_path).expect("imported main script should lower to MIR");
    let mir_output = run_mir(&mir).expect("imported main script MIR should run");
    assert_eq!(mir_output.stdout, output.stdout);

    let object = emit_host_native_object(&mir)
        .expect("direct backend should emit imported main script object");
    assert!(!object.is_empty());
}

#[test]
fn public_surface_covers_escape_diagnostics_argument_counts_and_builtin_member_metadata() {
    let source = r#"
def main() -> int32:
    text = "\0\x41\u{1F600}"
    label = f"\x42\u{43}"
    braces = f"{{literal}}"
    print(text.contains("A"))
    print(label)
    print(braces)
    return 0
"#;

    let output = run_source(source).expect("escape and writeback source should run");
    assert_eq!(output.stdout, "true\nBC\n{literal}\n");

    let mir = lower_source_to_mir(source).expect("escape and writeback source should lower");
    let mir_output = run_mir(&mir).expect("escape and writeback MIR should run");
    assert_eq!(mir_output.stdout, output.stdout);

    assert!(BuiltinMember::VecPush.requires_mutable_receiver());
    assert!(BuiltinMember::MapSet.requires_mutable_receiver());
    assert!(!BuiltinMember::VecLen.requires_mutable_receiver());
    assert!(!BuiltinMember::StringContains.requires_mutable_receiver());

    let invalid_escape_cases = [
        (
            "def main() -> None:\n    text = \"\\x4g\"\n",
            "invalid hexadecimal escape sequence",
        ),
        (
            "def main() -> None:\n    text = \"\\xg4\"\n",
            "invalid hexadecimal escape sequence",
        ),
        (
            "def main() -> None:\n    text = \"\\u{}\"\n",
            "unicode escape sequences must include at least one hexadecimal digit",
        ),
        (
            "def main() -> None:\n    text = \"\\u{110000}\"\n",
            "unicode escape sequence is out of range",
        ),
        (
            "def main() -> None:\n    text = \"\\u{100000000}\"\n",
            "unicode escape sequence is out of range",
        ),
        (
            "def main() -> None:\n    text = \"\\u{zz}\"\n",
            "invalid unicode escape sequence",
        ),
        (
            "def main() -> None:\n    text = \"\\u{12",
            "unterminated string literal",
        ),
        (
            "def main() -> None:\n    text = \"\\u1234\"\n",
            "unicode escape sequences must use the form `\\u{...}`",
        ),
        (
            "def main() -> None:\n    text = \"\\x",
            "unsupported escape sequence `\\x`",
        ),
        (
            "def main() -> None:\n    text = \"\\x4",
            "unsupported escape sequence `\\x`",
        ),
        (
            "def main() -> None:\n    text = \"\\x4\"\n",
            "invalid hexadecimal escape sequence",
        ),
    ];
    for (source, expected) in invalid_escape_cases {
        let error = check_source(source).expect_err("invalid escape should fail through check");
        assert!(
            error.message.contains(expected),
            "expected `{expected}`, got `{}`",
            error.message
        );
    }

    let arity_error = check_source("def main() -> None:\n    print(1, 2)\n")
        .expect_err("too many builtin args should fail through check");
    assert!(
        arity_error
            .message
            .contains("`print` expects 1 argument, found 2"),
        "unexpected arity diagnostic: {}",
        arity_error.message
    );

    for (source, expected) in [
        (
            "def main() -> None:\n    value = 1e\n",
            "invalid floating-point literal",
        ),
        (
            "def main() -> None:\n    value = 1e99999\n",
            "floating-point literal is out of range",
        ),
    ] {
        let error = check_source(source).expect_err("invalid float literal should fail");
        assert!(
            error.message.contains(expected),
            "expected `{expected}`, got `{}`",
            error.message
        );
    }
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
        return self.label.clone()

public enum Outcome:
    Ready(code: int32, reason: String)
    Empty
"#,
    );
    temp.write(
        "helpers/factory.au",
        r#"from pkg.user import User

public def describe_user(name: own String) -> String:
    user = User(label=name)
    return user.name()
"#,
    );
    let main_source = r#"import pkg.user
from helpers.factory import describe_user

def main() -> int32:
    user: pkg.user.User = pkg.user.User(label="Ada")
    outcome = pkg.user.Outcome.Ready(code=7, reason="ok")
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
    assert!(
        analysis
            .occurrences
            .iter()
            .any(|occurrence| occurrence.hover == "```aurora\nenum Outcome\n```"),
        "qualified enum references should expose the imported enum hover"
    );
    assert!(
        analysis.occurrences.iter().any(|occurrence| {
            occurrence.hover
                == "```aurora\nvariant Ready(code: own int32, reason: own String) -> Outcome\n```"
        }),
        "qualified enum constructors should expose every named payload as owned"
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

    let enum_completion_source = r#"import pkg.user

def main() -> int32:
    outcome = pkg.user.Outcome.
    return 0
"#;
    let (line, character) = line_and_character(enum_completion_source, "pkg.user.Outcome.");
    let enum_completions = complete_path_source(
        &main_path,
        enum_completion_source,
        line,
        character,
        Some('.'),
    )
    .expect("qualified imported enum completion should recover through the module namespace");
    assert_eq!(
        enum_completions
            .iter()
            .find(|completion| completion.name == "Ready")
            .map(|completion| completion.detail.as_str()),
        Some("Ready(code: own int32, reason: own String) -> Outcome")
    );

    let output = run_path(&main_path).expect("package program should run");
    let mir = lower_path_to_mir(&main_path).expect("package program should lower to MIR");
    let mir_output = run_mir(&mir).expect("package program MIR should run");
    assert_eq!(output.stdout, "Ada\nAda\n");
    assert_eq!(mir_output.stdout, output.stdout);
    assert!(!program.module.items.is_empty());

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
        "examples/concurrency/task_group_start.au",
        "examples/concurrency/queue_timeout.au",
        "examples/concurrency/queue_get_timeout_named.au",
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
        let mir = lower_path_to_mir(&path).expect("maintained example should lower to MIR");
        let mir_output = run_mir(&mir).expect("maintained example MIR should run");
        assert_eq!(mir_output.stdout, output.stdout, "{}", path.display());

        let object =
            emit_host_native_object(&mir).expect("maintained example should emit a native object");
        assert!(!object.is_empty(), "{}", path.display());
    }
}
