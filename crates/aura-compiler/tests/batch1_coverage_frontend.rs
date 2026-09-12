use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aura_compiler::ffi::FfiError;
use aura_compiler::{analyze_program, analyze_source, check_path, check_source, complete_source};

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
        fs::write(&path, source).expect("failed to write fixture file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn hovers(source: &str) -> Vec<String> {
    let analysis = analyze_source(source);
    assert!(
        analysis.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        analysis.diagnostics
    );
    analysis
        .occurrences
        .into_iter()
        .map(|occurrence| occurrence.hover)
        .collect()
}

fn assert_hover(source: &str, expected: &str) {
    let hovers = hovers(source);
    assert!(
        hovers.iter().any(|hover| hover.contains(expected)),
        "missing hover `{expected}` in {hovers:#?}"
    );
}

#[test]
fn union_annotation_binding_hover_lowers_the_written_union() {
    assert_hover(
        "def main():\n    tagged: int64 | str = 1\n    print(tagged)\n",
        "binding tagged: int64 | str",
    );
}

#[test]
fn callable_annotation_binding_hover_lowers_the_written_callable() {
    let source = concat!(
        "type Doubler = Callable[def(value: int64) -> int64]\n",
        "\n",
        "def double(value: int64) -> int64:\n",
        "    return value * 2\n",
        "\n",
        "def main():\n",
        "    written: Callable[def(value: int64) -> int64] = Doubler(double)\n",
        "    aliased: Doubler = Doubler(double)\n",
        "    print(written(2))\n",
        "    print(aliased(3))\n",
    );
    let hovers = hovers(source);
    for expected in [
        "binding written: Callable[def(value: int64) -> int64]",
        "binding aliased: Doubler",
    ] {
        assert!(
            hovers.iter().any(|hover| hover.contains(expected)),
            "missing hover `{expected}` in {hovers:#?}"
        );
    }
}

#[test]
fn completion_scope_lowers_annotations_through_aliases_unions_and_callables() {
    let source = concat!(
        "type Pair = (int64, int64)\n",
        "type Doubler = Callable[def(value: int64) -> int64]\n",
        "\n",
        "def double(value: int64) -> int64:\n",
        "    return value * 2\n",
        "\n",
        "def main():\n",
        "    pair: Pair = (1, 2)\n",
        "    tagged: int64 | str = 1\n",
        "    doubler: Callable[def(value: int64) -> int64] = Doubler(double)\n",
        "    print(pair[0])\n",
        "    print(tagged)\n",
        "    print(doubler(1))\n",
    );
    let completions = complete_source(source, 10, 4, None).expect("completion should succeed");
    let details = completions
        .iter()
        .filter(|completion| completion.kind == "variable")
        .map(|completion| (completion.name.as_str(), completion.detail.as_str()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(details.get("pair"), Some(&"(int64, int64)"));
    assert_eq!(details.get("tagged"), Some(&"int64 | str"));
    assert_eq!(
        details.get("doubler"),
        Some(&"Callable[def(value: int64) -> int64]")
    );
}

#[test]
fn written_callable_types_appear_in_document_symbols() {
    let source = concat!(
        "type Doubler = Callable[def(value: int64) -> int64]\n",
        "\n",
        "class Holder:\n",
        "    handler: Callable[def(value: int64) -> int64]\n",
        "\n",
        "def double(value: int64) -> int64:\n",
        "    return value * 2\n",
        "\n",
        "def make() -> Callable[def(value: int64) -> int64]:\n",
        "    return Doubler(double)\n",
        "\n",
        "def main():\n",
        "    print(make()(1))\n",
    );
    let analysis = analyze_source(source);
    assert!(
        analysis.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        analysis.diagnostics
    );
    let holder = analysis
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Holder")
        .expect("class symbol");
    let handler = holder
        .children
        .iter()
        .find(|symbol| symbol.name == "handler")
        .expect("field symbol");
    assert_eq!(handler.detail, "Callable[def(value: int64) -> int64]");
    let make = analysis
        .symbols
        .iter()
        .find(|symbol| symbol.name == "make")
        .expect("function symbol");
    assert!(
        make.detail.contains("Callable[def(value: int64) -> int64]"),
        "unexpected detail {:?}",
        make.detail
    );
}

#[test]
fn match_statement_guards_are_analyzed_in_the_arm_scope() {
    let source = concat!(
        "enum Shape:\n",
        "    Circle(float64)\n",
        "    Square(float64)\n",
        "\n",
        "def describe(shape: Shape) -> str:\n",
        "    match shape:\n",
        "        case Shape.Circle(radius) if radius > 1.0:\n",
        "            return \"big\"\n",
        "        case Shape.Circle(radius):\n",
        "            return \"small\"\n",
        "        case Shape.Square(side):\n",
        "            return \"square\"\n",
        "\n",
        "def main():\n",
        "    print(describe(Shape.Circle(2.0)))\n",
    );
    let analysis = analyze_source(source);
    assert!(
        analysis.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        analysis.diagnostics
    );
    let guard_line = 6;
    assert!(
        analysis.occurrences.iter().any(|occurrence| {
            occurrence.line == guard_line && occurrence.hover.contains("radius: float64")
        }),
        "guard operand should resolve to the arm binding: {:#?}",
        analysis.occurrences
    );
}

#[test]
fn for_loop_over_user_function_call_infers_element_type() {
    assert_hover(
        concat!(
            "def make_list() -> list[int64]:\n",
            "    return [1, 2]\n",
            "\n",
            "def main():\n",
            "    for value in make_list():\n",
            "        print(value)\n",
        ),
        "local value: int64",
    );
}

#[test]
fn indexing_a_field_list_infers_the_element_type() {
    assert_hover(
        concat!(
            "class Bag:\n",
            "    values: list[int64]\n",
            "\n",
            "def main():\n",
            "    bag = Bag(values=[1, 2])\n",
            "    first = bag.values[0]\n",
            "    print(first)\n",
        ),
        "binding first: int64",
    );
}

#[test]
fn conditional_with_float_and_integer_literal_infers_float64() {
    let source = concat!(
        "def main():\n",
        "    flag = true\n",
        "    scale = 2.5 if flag else 1\n",
        "    print(scale)\n",
    );
    assert_hover(source, "binding scale: float64");
}

#[test]
fn completion_after_indexed_receiver_lists_element_members() {
    let source = "def main():\n    words: list[str] = [\"a\"]\n    words[0].\n";
    let completions = complete_source(source, 2, 13, Some('.'))
        .expect("indexed receiver completion should recover");
    let names = completions
        .iter()
        .map(|completion| completion.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"len"), "unexpected completions {names:?}");
}

#[test]
fn completion_inside_comprehension_output_with_tuple_target_sees_both_names() {
    let source = concat!(
        "def main():\n",
        "    pairs: list[(int64, int64)] = [(1, 2)]\n",
        "    sums = [a + b for (a, b) in pairs]\n",
        "    print(sums)\n",
    );
    let completions =
        complete_source(source, 2, 16, None).expect("comprehension completion should succeed");
    let details = completions
        .iter()
        .filter(|completion| completion.kind == "variable")
        .map(|completion| (completion.name.as_str(), completion.detail.as_str()))
        .collect::<Vec<_>>();
    assert!(
        details.contains(&("a", "int64")) && details.contains(&("b", "int64")),
        "unexpected variables {details:?}"
    );
}

#[test]
fn completion_scope_walks_is_none_formatted_fstrings_and_match_guards() {
    let source = concat!(
        "def main():\n",
        "    maybe: Option[int64] = Option.Some(1)\n",
        "    missing = maybe is None\n",
        "    count = 3\n",
        "    label = f\"{count:>5}\"\n",
        "    code = match missing:\n",
        "        case true if count > 1: 1\n",
        "        case _: 2\n",
        "    print(label)\n",
        "    print(code)\n",
    );
    let completions = complete_source(source, 9, 4, None).expect("completion should succeed");
    let names = completions
        .iter()
        .map(|completion| completion.name.as_str())
        .collect::<Vec<_>>();
    for expected in ["missing", "label", "code"] {
        assert!(
            names.contains(&expected),
            "missing `{expected}` in {names:?}"
        );
    }
}

#[test]
fn analysis_of_a_program_checked_from_other_source_skips_missing_import_aliases() {
    let program = check_source(
        "import math as m\nfrom math import pi as circle\n\ndef main():\n    print(m.pi)\n    print(circle)\n",
    )
    .expect("aliased builtin imports should check");
    let analysis = analyze_program("", &program);
    assert!(analysis.diagnostics.is_empty());
    assert!(
        analysis
            .occurrences
            .iter()
            .all(|occurrence| occurrence.line > 1),
        "import alias occurrences cannot be located in an empty source: {:#?}",
        analysis.occurrences
    );
}

#[test]
fn ffi_error_display_covers_unsupported_process_lookup_platform() {
    assert_eq!(
        FfiError::UnsupportedProcessLookupPlatform.to_string(),
        "process-global FFI symbol lookup is unavailable on this platform"
    );
}

#[test]
fn duplicate_aliased_builtin_module_imports_are_rejected() {
    let error = check_source("import math as m\nimport json as m\n\ndef main():\n    pass\n")
        .expect_err("duplicate aliases should be rejected");
    assert!(
        error.message.contains("duplicate import binding `m`"),
        "unexpected message {:?}",
        error.message
    );
}

#[test]
fn package_manifests_declaring_too_many_dependencies_are_rejected() {
    let temp = TempDir::new("aura-frontend-too-many-deps");
    let mut manifest =
        String::from("[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n");
    for index in 0..1025 {
        manifest.push_str(&format!(
            "[dependencies.dep{index}]\npath = \"../dep{index}\"\n\n"
        ));
    }
    temp.write("app/Aura.toml", &manifest);
    let main_path = temp.write("app/src/main.au", "def main():\n    pass\n");
    let error = check_path(&main_path).expect_err("oversized dependency lists should be rejected");
    assert!(
        error
            .message
            .contains("declares 1025 dependencies, which exceeds the supported limit of 1024"),
        "unexpected message {:?}",
        error.message
    );
}

#[test]
fn package_dependencies_with_both_path_and_git_sources_are_rejected() {
    let temp = TempDir::new("aura-frontend-dual-source-dep");
    temp.write(
        "app/Aura.toml",
        concat!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n",
            "[dependencies]\n",
            "helper = { path = \"../helper\", git = \"https://example.invalid/helper.git\" }\n",
        ),
    );
    let main_path = temp.write("app/src/main.au", "def main():\n    pass\n");
    let error = check_path(&main_path).expect_err("dual-source dependencies should be rejected");
    assert!(
        error
            .message
            .contains("dependency `helper` must choose exactly one dependency source"),
        "unexpected message {:?}",
        error.message
    );
}

#[test]
fn dependency_modules_with_from_imports_keep_namespace_qualification() {
    let temp = TempDir::new("aura-frontend-dependency-from-import");
    temp.write(
        "app/Aura.toml",
        concat!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n",
            "[dependencies]\nutil = { path = \"../util\" }\n",
        ),
    );
    let main_path = temp.write(
        "app/src/main.au",
        "import util.math\n\ndef main():\n    print(util.math.double(value=2))\n",
    );
    temp.write(
        "util/Aura.toml",
        "[package]\nname = \"util\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    temp.write(
        "util/src/helpers/core.au",
        "public def scale(value: int32) -> int32:\n    return value * 2\n",
    );
    temp.write(
        "util/src/math.au",
        concat!(
            "from helpers.core import scale\n",
            "\n",
            "public def double(value: int32) -> int32:\n",
            "    return scale(value=value)\n",
        ),
    );
    let program = check_path(&main_path).expect("dependency modules may use `from` imports");
    assert!(
        program.module_registry.contains_key("util.math"),
        "dependency modules should be registered under the package prefix: {:?}",
        program.module_registry.keys().collect::<Vec<_>>()
    );
}
