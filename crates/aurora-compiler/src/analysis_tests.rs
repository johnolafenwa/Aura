use super::{
    analysis_diagnostic, analyze_path_source, analyze_source, base_type_name, block_contains_line,
    builtin_enum_hover, builtin_enum_variant_completions, builtin_function_hover,
    builtin_function_return_type, builtin_member_completions, callable_contains_line,
    complete_path_source, complete_source, enclosing_function_return_placeholder,
    extract_receiver_before_dot, extract_receiver_ending_before, find_identifier_in_line,
    find_receiver_start, format_class_hover, format_enum_hover, format_function_detail,
    format_function_hover, format_method_hover, format_value_hover, format_variant_hover,
    infer_builtin_variant_call, lower_type_ref, placeholder_stmt_for_return_type, range_from_span,
    range_from_span_with_path, recover_checked_program_after_member_errors,
    recover_checked_program_after_member_errors_with,
    recover_checked_program_after_parse_error_with, recover_checked_program_after_position,
    replace_dangling_member_stmt_with_recovery_stmt, sanitize_member_completion_source,
    stmt_end_line, stmt_start_line, AnalysisBuilder, TypeExt,
};
use crate::ast::{
    Argument, AssignStmt, AssignTarget, BinaryOp, ClassDecl, Expr, ExprKind, FunctionDecl, Item,
    ParamMode, PassStmt, ReceiverKind, ReturnStmt, TypeRef, VariantPattern,
};
use crate::diag::{Diagnostic, Span};
use crate::sema::{
    ClassInfo, EnumInfo, EnumVariantInfo, FieldInfo, FunctionSignature, MethodInfo, TraitBound,
    Type,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

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

    fn path(&self) -> &PathBuf {
        &self.path
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
        .and_then(|path| path.parent())
        .unwrap()
        .to_path_buf()
}

fn run_with_large_stack<T, F>(operation: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(operation)
        .expect("large-stack helper thread should spawn")
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}

fn collect_aurora_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut files = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {}", dir.display(), error))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let is_aurora = path.extension().and_then(|ext| ext.to_str()) == Some("au");
            if is_aurora {
                Some(path)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn type_ref(name: &str) -> TypeRef {
    TypeRef {
        name: name.to_string(),
        args: Vec::new(),
        indirect: false,
        span: Span::new(1, 1),
    }
}

fn checked_program(source: &str) -> crate::sema::Program {
    crate::check_source(source).expect("source should type check")
}

fn function_decl(name: &str, return_type: &str) -> FunctionDecl {
    FunctionDecl {
        public: true,
        name: name.to_string(),
        type_params: Vec::new(),
        type_param_bounds: Default::default(),
        receiver: Some(ReceiverKind::Borrow),
        params: vec![crate::ast::Param {
            name: "value".to_string(),
            mode: ParamMode::Default,
            borrow_label: None,
            ty: type_ref("int32"),
            default: None,
            span: Span::new(1, 1),
        }],
        return_passing: ReceiverKind::Value,
        return_borrow_source: None,
        return_type: type_ref(return_type),
        body: Vec::new(),
        span: Span::new(1, 1),
    }
}

fn arg(value: Expr) -> Argument {
    Argument {
        name: None,
        span: value.span,
        value,
    }
}

fn expr(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: Span::new(1, 1),
    }
}

#[test]
fn d5_analysis_renders_canonical_receiver_modes_and_completes_own_keyword() {
    let mut method = function_decl("render", "bool");
    assert_eq!(
        format_function_detail(&method),
        "render(self, value: int32) -> bool"
    );
    assert_eq!(
        format_method_hover(&method),
        "```aurora\nmethod render(self, value: int32) -> bool\n```"
    );

    method.receiver = Some(ReceiverKind::Value);
    assert_eq!(
        format_function_detail(&method),
        "render(own self, value: int32) -> bool"
    );
    assert_eq!(
        format_method_hover(&method),
        "```aurora\nmethod render(own self, value: int32) -> bool\n```"
    );

    method.receiver = Some(ReceiverKind::BorrowMut);
    assert_eq!(
        format_function_detail(&method),
        "render(borrow mut self, value: int32) -> bool"
    );
    assert_eq!(
        format_method_hover(&method),
        "```aurora\nmethod render(borrow mut self, value: int32) -> bool\n```"
    );

    let completions = complete_source("def main():\n    pass\n", 0, 0, None)
        .expect("top-level completion should succeed");
    assert!(completions
        .iter()
        .any(|completion| completion.name == "own" && completion.kind == "keyword"));
}

#[test]
fn d6_analysis_renders_source_parameter_and_transfer_ownership() {
    let source = r#"
class Box:
    value: String

enum Message:
    Text(String)

def inspect(value: String):
    print(value)

def consume(value: own String):
    print(value)
"#;
    let completions = complete_source(source, 0, 0, None).expect("D6 source should complete");
    assert_eq!(
        completions
            .iter()
            .find(|item| item.name == "inspect")
            .map(|item| item.detail.as_str()),
        Some("inspect(value: String) -> None")
    );
    assert_eq!(
        completions
            .iter()
            .find(|item| item.name == "consume")
            .map(|item| item.detail.as_str()),
        Some("consume(value: own String) -> None")
    );
    assert_eq!(
        completions
            .iter()
            .find(|item| item.name == "Box")
            .map(|item| item.detail.as_str()),
        Some("Box(value: own String)")
    );

    let vec_members =
        builtin_member_completions(&Type::Named("Vec".to_string(), vec![Type::named("String")]));
    assert!(vec_members
        .iter()
        .any(|item| item.name == "push" && item.detail.contains("own T")));
    let message_members = builtin_enum_variant_completions("Option");
    assert!(message_members
        .iter()
        .any(|item| item.name == "Some" && item.detail.contains("own T")));
}

#[test]
fn d3_analysis_reports_canonical_int64_for_aliases_and_defaulted_expressions() {
    assert_eq!(lower_type_ref(&type_ref("int")), Type::named("int64"));

    let source = r#"
def main() -> int32:
    scalar = 1
    numbers = [1, 2]
    maybe = Option.Some(1)
    print(scalar)
    print(numbers.len())
    print(maybe != Option.None)
    return 0
"#;
    let output = analyze_source(source);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);

    for expected_hover in [
        "binding scalar: int64",
        "binding numbers: Vec[int64]",
        "binding maybe: Option[int64]",
    ] {
        assert!(
            output
                .occurrences
                .iter()
                .any(|occurrence| occurrence.hover.contains(expected_hover)),
            "missing hover `{expected_hover}` in {:?}",
            output.occurrences
        );
    }
}

#[test]
fn machine_readable_analysis_covers_symbols_and_occurrences() {
    let source = include_str!("../../../examples/point.au");
    let analysis = analyze_source(source);

    assert!(analysis.diagnostics.is_empty());
    assert!(analysis
        .symbols
        .iter()
        .any(|symbol| symbol.kind == "class" && symbol.name == "Point"));
    assert!(analysis
        .occurrences
        .iter()
        .any(|occurrence| occurrence.hover.contains("sqrt() -> float64")));
}

#[test]
fn machine_readable_analysis_reports_diagnostics() {
    let source = "def main():\n    print(total)\n";
    let analysis = analyze_source(source);

    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, "AU2001");
    assert_eq!(analysis.diagnostics[0].severity, 1);
    assert!(analysis.diagnostics[0].secondary_spans.is_empty());
    assert!(analysis.diagnostics[0].notes.is_empty());
    assert!(analysis.diagnostics[0].help.is_empty());
    assert!(analysis.diagnostics[0].edits.is_empty());
    assert!(analysis.diagnostics[0]
        .message
        .contains("unknown name `total`"));
}

#[test]
fn queue_timeout_analysis_does_not_treat_queue_receive_or_ms_as_unknown() {
    let source = "def main() -> int32:\n    jobs = Queue[int32]()\n    match jobs.get(timeout=5ms):\n        case QueueReceive.Item(value):\n            print(value)\n        case QueueReceive.TimedOut:\n            print(\"timeout\")\n        case _:\n            pass\n    return 0\n";
    let analysis = analyze_source(source);

    assert!(analysis.diagnostics.is_empty());
    assert!(analysis.occurrences.iter().any(|occurrence| occurrence
        .hover
        .contains("get(timeout: Duration = ...) -> QueueReceive[T]")));
}

#[test]
fn compiler_member_completion_returns_class_fields() {
    let source = include_str!("../../../examples/point.au");
    let line_index = source
        .lines()
        .position(|line| line.contains("a.x"))
        .expect("point example should contain member access");
    let line_text = source.lines().nth(line_index).unwrap();
    let character = line_text.find('.').unwrap() + 1;

    let completions =
        complete_source(source, line_index, character, Some('.')).expect("completion should work");
    let names = completions
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"x".to_string()));
    assert!(names.contains(&"y".to_string()));
}

#[test]
fn compiler_member_completion_for_string_exposes_string_methods() {
    let source = "def main() -> int32:\n    text = \"  aurora  \"\n    text.\n    return 0\n";
    let line_index = source
        .lines()
        .position(|line| line.contains("text."))
        .expect("string clone example should contain member access");
    let line_text = source.lines().nth(line_index).unwrap();
    let character = line_text.find('.').unwrap() + 1;

    let completions =
        complete_source(source, line_index, character, Some('.')).expect("completion should work");
    let names = completions
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"len".to_string()));
    assert!(names.contains(&"byte_len".to_string()));
    assert!(names.contains(&"contains".to_string()));
    assert!(names.contains(&"starts_with".to_string()));
    assert!(names.contains(&"ends_with".to_string()));
    assert!(names.contains(&"trim".to_string()));
    assert!(names.contains(&"split".to_string()));
    assert!(names.contains(&"replace".to_string()));
    assert!(names.contains(&"to_lower".to_string()));
    assert!(names.contains(&"to_upper".to_string()));
    assert!(names.contains(&"strip_prefix".to_string()));
    assert!(names.contains(&"strip_suffix".to_string()));
    assert!(names.contains(&"clone".to_string()));
    assert!(!names.contains(&"as_str".to_string()));
}

#[test]
fn compiler_duration_tooling_exposes_static_constructors_and_instance_conversions() {
    let static_source = "def main() -> int32:\n    Duration.\n    return 0\n";
    let static_names = completion_names_after_marker(static_source, "Duration.");
    assert!(static_names.contains(&"ms".to_string()));
    assert!(static_names.contains(&"seconds".to_string()));
    assert!(static_names.contains(&"minutes".to_string()));
    assert!(!static_names.contains(&"to_ms".to_string()));

    let instance_source =
        "def inspect(duration: Duration):\n    duration.\n\ndef main() -> int32:\n    return 0\n";
    let instance_names = completion_names_after_marker(instance_source, "duration.");
    assert!(instance_names.contains(&"to_ms".to_string()));
    assert!(instance_names.contains(&"to_seconds".to_string()));
    assert!(!instance_names.contains(&"seconds".to_string()));

    let analysis = analyze_source(
        r#"
def convert(value: int64, duration: Duration) -> float64:
    built = Duration.ms(value)
    scaled = duration * value
    return built.to_ms() + scaled.to_seconds()

def main() -> int32:
    return 0
"#,
    );
    assert!(analysis.diagnostics.is_empty());
    assert!(analysis
        .occurrences
        .iter()
        .any(|occurrence| occurrence.hover.contains("type Duration")));
    assert!(analysis
        .occurrences
        .iter()
        .any(|occurrence| occurrence.hover.contains("ms(value: int64) -> Duration")));
    assert!(analysis
        .occurrences
        .iter()
        .any(|occurrence| occurrence.hover.contains("to_ms() -> float64")));
    assert!(analysis
        .occurrences
        .iter()
        .any(|occurrence| occurrence.hover.contains("to_seconds() -> float64")));
}

#[test]
fn analysis_ignores_builtin_omitted_defaults_outside_source_inference() {
    let program = checked_program("def main() -> int32:\n    return 0\n");
    let marker = expr(ExprKind::BuiltinOmitted);
    let mut builder = AnalysisBuilder::new("", &program, Vec::new());

    assert_eq!(builder.infer_expr_type(&marker, &BTreeMap::new()), None);
    builder.visit_expr(&marker, &BTreeMap::new());
    assert!(builder.output.occurrences.is_empty());
}

#[test]
fn compiler_member_completion_for_map_exposes_map_methods() {
    let source =
        "def main() -> int32:\n    mut counts = Map[String, int32]()\n    counts.\n    return 0\n";
    let line_index = source
        .lines()
        .position(|line| line.contains("counts."))
        .expect("map source should contain member access");
    let line_text = source.lines().nth(line_index).unwrap();
    let character = line_text.find('.').unwrap() + 1;

    let completions =
        complete_source(source, line_index, character, Some('.')).expect("completion should work");
    let names = completions
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"len".to_string()));
    assert!(names.contains(&"is_empty".to_string()));
    assert!(names.contains(&"clone".to_string()));
    assert!(names.contains(&"get".to_string()));
    assert!(names.contains(&"set".to_string()));
    assert!(names.contains(&"remove".to_string()));
    assert!(names.contains(&"contains_key".to_string()));
    assert!(names.contains(&"keys".to_string()));
    assert!(names.contains(&"values".to_string()));
}

#[test]
fn compiler_member_completion_for_vec_reports_insert_bool_detail() {
    let source = "def main() -> int32:\n    mut values = [1, 2, 3]\n    values.\n    return 0\n";
    let line_index = source
        .lines()
        .position(|line| line.contains("values."))
        .expect("vec source should contain member access");
    let line_text = source.lines().nth(line_index).unwrap();
    let character = line_text.find('.').unwrap() + 1;

    let completions =
        complete_source(source, line_index, character, Some('.')).expect("completion should work");
    let insert = completions
        .into_iter()
        .find(|item| item.name == "insert")
        .expect("insert completion should exist");

    assert_eq!(insert.detail, "insert(index: int32, value: own T) -> bool");
}

#[test]
fn compiler_member_completion_includes_trait_impl_methods() {
    let source = include_str!("../../../examples/traits/greeter.au");
    let line_index = source
        .lines()
        .position(|line| line.contains("value.greet()"))
        .expect("trait example should contain member access");
    let line_text = source.lines().nth(line_index).unwrap();
    let character = line_text.find('.').unwrap() + 1;

    let completions =
        complete_source(source, line_index, character, Some('.')).expect("completion should work");
    let names = completions
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"greet".to_string()));
}

#[test]
fn compiler_top_level_completion_includes_keywords_and_builtins() {
    let source = include_str!("../../../examples/point.au");
    let completions = complete_source(source, 0, 0, None).expect("completion should work");
    let names = completions
        .iter()
        .map(|item| item.name.clone())
        .collect::<Vec<_>>();

    assert!(names.contains(&"class".to_string()));
    assert!(names.contains(&"trait".to_string()));
    assert!(names.contains(&"Point".to_string()));
    assert!(names.contains(&"distance".to_string()));
    assert!(names.contains(&"print".to_string()));
    assert!(names.contains(&"abs".to_string()));
    assert!(names.contains(&"min".to_string()));
    assert!(names.contains(&"max".to_string()));
    assert!(names.contains(&"sqrt".to_string()));
    let range = completions
        .iter()
        .find(|item| item.name == "range")
        .expect("range builtin should appear in completions");
    assert!(range.detail.contains("start: int32"));
}

fn completion_names_after_marker(source: &str, marker: &str) -> Vec<String> {
    let line_index = source
        .lines()
        .position(|line| line.contains(marker))
        .expect("source should contain completion marker");
    let line_text = source.lines().nth(line_index).unwrap();
    let character = line_text.find(marker).unwrap() + marker.len();

    complete_source(source, line_index, character, Some('.'))
        .expect("completion should work")
        .into_iter()
        .map(|item| item.name)
        .collect()
}

#[test]
fn compiler_completion_uses_nested_scopes_for_methods_match_for_and_trait_bounds() {
    let source = [
        "trait Show:",
        "    def show(borrow self) -> String",
        "",
        "class Label:",
        "    value: int32",
        "    def collect(borrow self) -> int32:",
        "        mut items: Vec[String] = [\"ready\"]",
        "        for item in items:",
        "            item.len()",
        "        self.value",
        "        return 0",
        "",
        "def unwrap(value: own Option[String]) -> String:",
        "    match value:",
        "        case Option.Some(text):",
        "            text.len()",
        "            return text",
        "        case Option.None:",
        "            return \"\"",
        "",
        "def noop():",
        "    pass",
        "",
        "def use_group():",
        "    with TaskGroup() as group:",
        "        group.start_soon(noop)",
        "",
        "def render[T: Show](value: T) -> String:",
        "    value.show()",
        "    return value.show()",
        "",
        "def after_branch(flag: bool) -> int32:",
        "    label = \"ready\"",
        "    if flag:",
        "        branch = \"yes\"",
        "    else:",
        "        branch = \"no\"",
        "    label.len()",
        "    return 0",
    ]
    .join("\n");

    let for_scope = completion_names_after_marker(&source, "item.");
    assert!(for_scope.contains(&"len".to_string()));

    let method_scope = completion_names_after_marker(&source, "self.");
    assert!(method_scope.contains(&"value".to_string()));

    let match_scope = completion_names_after_marker(&source, "text.");
    assert!(match_scope.contains(&"len".to_string()));

    let with_scope = completion_names_after_marker(&source, "group.");
    assert!(with_scope.contains(&"start".to_string()));

    let trait_bound_scope = completion_names_after_marker(&source, "value.");
    assert!(trait_bound_scope.contains(&"show".to_string()));

    let after_branch_scope = completion_names_after_marker(&source, "label.");
    assert!(after_branch_scope.contains(&"len".to_string()));
}

#[test]
fn analysis_recovery_helpers_cover_member_error_paths() {
    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    counter.",
        "    counter.",
        "    return 0",
    ]
    .join("\n");
    let mut check_program = crate::check_source;

    let error = crate::parser::parse(&source).expect_err("dangling member should not parse");
    let recovered =
        recover_checked_program_after_parse_error_with(&source, &error, &mut check_program);
    assert!(recovered.is_some());

    let recovered_after_position =
        recover_checked_program_after_position(&source, 5, 12, &mut check_program);
    assert!(recovered_after_position.is_some());

    let recovered_after_members =
        recover_checked_program_after_member_errors(&source, &mut check_program);
    assert!(recovered_after_members.is_some());

    let too_many_dangling_members = [
        "def main() -> int32:",
        "    counter.",
        "    counter.",
        "    counter.",
        "    counter.",
        "    counter.",
        "    counter.",
        "    counter.",
        "    counter.",
        "    counter.",
        "    return 0",
    ]
    .join("\n");
    assert!(
        recover_checked_program_after_member_errors(&too_many_dangling_members, &mut check_program)
            .is_none(),
        "recovery should stop after the bounded retry budget"
    );

    let non_member = Diagnostic::at(Span::new(1, 1), "expected Colon, found Newline");
    assert!(recover_checked_program_after_parse_error_with(
        &source,
        &non_member,
        &mut check_program
    )
    .is_none());
    assert!(
        recover_checked_program_after_member_errors("def main(\n", &mut check_program).is_none()
    );
    assert!(
        complete_source("def main(\n", 0, 1, None).is_err(),
        "non-member completion requests should surface parse errors instead of recovering"
    );
}

#[test]
fn analysis_recovery_helpers_stop_when_replacement_makes_no_progress() {
    fn no_progress(source: &str, _line: usize) -> String {
        source.to_string()
    }

    let source = ["def main() -> None:", "    value."].join("\n");
    let mut check_program = crate::check_source;

    assert!(
        recover_checked_program_after_member_errors_with(&source, &mut check_program, no_progress,)
            .is_none(),
        "member recovery should stop if the replacement leaves the candidate unchanged"
    );
}

#[test]
fn analysis_trait_impl_helpers_cover_generic_bound_resolution() {
    let source = [
        "trait Show:",
        "    def show(borrow self) -> String",
        "",
        "trait Named:",
        "    def label(borrow self) -> String",
        "",
        "trait Mapper[T]:",
        "    def map(borrow self) -> T",
        "",
        "class Box[T]:",
        "    value: T",
        "",
        "impl Show for int32:",
        "    def show(borrow self) -> String:",
        "        return f\"{self}\"",
        "",
        "impl[T: Show] Named for Box[T]:",
        "    def label(borrow self) -> String:",
        "        return self.value.show()",
        "",
        "impl Mapper[int32] for Box[int32]:",
        "    def map(borrow self) -> int32:",
        "        return self.value",
    ]
    .join("\n");
    let program = checked_program(&source);
    let builder = AnalysisBuilder::new(&source, &program, Vec::new());
    let trait_impl = program
        .trait_impls
        .iter()
        .find(|info| info.trait_name == "Named")
        .expect("Named impl should exist");
    let bound = TraitBound {
        trait_name: "Named".to_string(),
        trait_args: Vec::new(),
    };

    let substitutions = builder
        .trait_impl_substitutions(
            trait_impl,
            &Type::Named("Box".to_string(), vec![Type::named("int32")]),
        )
        .expect("Box[String] should satisfy Named impl");
    assert_eq!(substitutions.get("T"), Some(&Type::named("int32")));

    let bound_substitutions = builder
        .trait_impl_substitutions_for_bound(
            trait_impl,
            &Type::Named("Box".to_string(), vec![Type::named("int32")]),
            &bound,
        )
        .expect("bound substitution should resolve");
    assert_eq!(bound_substitutions.get("T"), Some(&Type::named("int32")));

    assert!(builder.type_implements_trait_bound(
        &Type::Named("Box".to_string(), vec![Type::named("int32")]),
        &bound,
    ));
    assert!(!builder.type_implements_trait_bound(
        &Type::Named("Box".to_string(), vec![Type::named("String")]),
        &bound,
    ));
    let mapper_impl = program
        .trait_impls
        .iter()
        .find(|info| info.trait_name == "Mapper")
        .expect("Mapper impl should exist");
    let mismatched_mapper_bound = TraitBound {
        trait_name: "Mapper".to_string(),
        trait_args: vec![Type::named("String")],
    };
    assert!(
        builder
            .trait_impl_substitutions_for_bound(
                mapper_impl,
                &Type::Named("Box".to_string(), vec![Type::named("int32")]),
                &mismatched_mapper_bound,
            )
            .is_none(),
        "trait argument mismatch should reject otherwise matching impls"
    );
    let matching_mapper_bound = TraitBound {
        trait_name: "Mapper".to_string(),
        trait_args: vec![Type::named("int32")],
    };
    let mapper_substitutions = builder
        .trait_impl_substitutions_for_bound(
            mapper_impl,
            &Type::Named("Box".to_string(), vec![Type::named("int32")]),
            &matching_mapper_bound,
        )
        .expect("matching trait arguments should keep the impl in scope");
    assert!(mapper_substitutions.is_empty());

    let (_impl_info, method, resolved) = builder
        .trait_method_for_receiver(
            &Type::Named("Box".to_string(), vec![Type::named("int32")]),
            "label",
        )
        .expect("trait method should resolve for Box[int32]");
    assert_eq!(method.signature.return_type, Type::named("String"));
    assert_eq!(resolved.get("T"), Some(&Type::named("int32")));
}

#[test]
fn analysis_scope_and_call_inference_helpers_cover_methods_assignments_and_builtins() {
    let source = [
        "class Counter:",
        "    value: int32",
        "    def bump(borrow mut self, step: int32) -> int32:",
        "        start = self.value",
        "        mut total = start",
        "        total = total + step",
        "        self.value = total",
        "        return total",
        "",
        "def helper() -> int32:",
        "    return 1",
        "",
        "def main() -> int32:",
        "    return helper()",
    ]
    .join("\n");
    let program = checked_program(&source);
    let mut builder = AnalysisBuilder::new(&source, &program, Vec::new());
    let class_decl = program
        .module
        .items
        .iter()
        .find_map(|item| match item {
            Item::Class(class_decl) if class_decl.name == "Counter" => Some(class_decl),
            _ => None,
        })
        .expect("Counter class should exist");
    let method_decl = class_decl
        .methods
        .iter()
        .find(|method| method.name == "bump")
        .expect("bump method should exist");
    let method_info = program
        .classes
        .get("Counter")
        .and_then(|class| class.methods.get("bump"))
        .expect("method info should exist");

    let mut scope = builder.method_scope("Counter", method_decl, method_info);
    assert_eq!(
        scope.get("self").map(|binding| binding.ty.clone()),
        Some(Type::named("Counter"))
    );
    assert_eq!(
        scope.get("step").map(|binding| binding.ty.clone()),
        Some(Type::named("int32"))
    );

    builder.visit_stmts(&method_decl.body, &mut scope);
    assert_eq!(
        scope.get("start").map(|binding| binding.ty.clone()),
        Some(Type::named("int32"))
    );
    assert_eq!(
        scope.get("total").map(|binding| binding.ty.clone()),
        Some(Type::named("int32"))
    );
    assert!(!builder.output.occurrences.is_empty());

    let scope_for_return = builder.scope_for_line(7);
    assert!(scope_for_return.contains_key("self"));
    assert!(scope_for_return.contains_key("total"));

    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Name("abs".to_string())),
            &[arg(expr(ExprKind::Int(4)))],
            &BTreeMap::new(),
        ),
        Some(Type::named("int64"))
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Name("sqrt".to_string())),
            &[arg(expr(ExprKind::Float(4.0)))],
            &BTreeMap::new(),
        ),
        Some(Type::named("float64"))
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Vec".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            &[],
            &BTreeMap::new(),
        ),
        Some(Type::Named("Vec".to_string(), vec![Type::named("int32")]))
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Name("helper".to_string())),
            &[],
            &BTreeMap::new(),
        ),
        Some(Type::named("int32"))
    );
}

#[test]
fn completion_scope_walks_past_if_else_and_while_blocks() {
    let source = [
        "def scoped(flag: bool) -> int32:",
        "    mut total: int32 = 0",
        "    if flag:",
        "        in_if = total",
        "    else:",
        "        in_else = total",
        "    after_if = total",
        "    while flag:",
        "        in_while = total",
        "        break",
        "    after_while = total",
        "    return after_while",
        "",
        "def main() -> int32:",
        "    return scoped(false)",
    ]
    .join("\n");
    let program = checked_program(&source);
    let builder = AnalysisBuilder::new(&source, &program, Vec::new());

    let scope_inside_else = builder.scope_for_line(5);
    assert!(scope_inside_else.contains_key("total"));
    assert!(scope_inside_else.contains_key("in_else"));
    assert!(!scope_inside_else.contains_key("after_if"));

    let scope_after_if = builder.scope_for_line(7);
    assert!(scope_after_if.contains_key("total"));
    assert!(scope_after_if.contains_key("after_if"));
    assert!(!scope_after_if.contains_key("in_else"));

    let scope_after_while = builder.scope_for_line(11);
    assert!(scope_after_while.contains_key("after_while"));
    assert!(!scope_after_while.contains_key("in_while"));
}

#[test]
fn analysis_completion_and_inference_helpers_cover_builtin_collection_and_enum_surfaces() {
    let source = [
        "trait Show:",
        "    def show(borrow self) -> String",
        "",
        "trait Greeter:",
        "    def greet(borrow self) -> String",
        "",
        "class User:",
        "    label: String",
        "",
        "    def greet(borrow self) -> String:",
        "        return self.label.clone()",
        "",
        "impl Show for User:",
        "    def show(borrow self) -> String:",
        "        return self.label.clone()",
        "",
        "impl Greeter for User:",
        "    def greet(borrow self) -> String:",
        "        return self.label.clone()",
        "",
        "enum Status:",
        "    Ready",
        "    Failed(code: int32, reason: String)",
        "",
        "def helper() -> int32:",
        "    return 1",
        "",
        "def resultify(value: int32) -> Result[int32, String]:",
        "    return Result.Ok(value)",
    ]
    .join("\n");
    let mut program = checked_program(&source);
    let remote_source = [
        "trait RemoteTrait:",
        "    def render(borrow self) -> String",
        "",
        "enum RemoteStatus:",
        "    Ready",
        "    Failed(code: int32, reason: String)",
        "",
        "class Remote:",
        "    value: int32",
        "",
        "def remote_fn() -> int32:",
        "    return 9",
    ]
    .join("\n");
    let remote_program = checked_program(&remote_source);
    let mut tools_namespace = crate::sema::ModuleNamespace {
        name: "tools".to_string(),
        path: "pkg.tools".to_string(),
        source_path: None,
        modules: Default::default(),
        functions: remote_program.functions.clone(),
        classes: remote_program.classes.clone(),
        enums: remote_program.enums.clone(),
        traits: remote_program.traits.clone(),
        trait_impls: remote_program.trait_impls.clone(),
        all_functions: remote_program.functions.clone(),
        all_classes: remote_program.classes.clone(),
        all_enums: remote_program.enums.clone(),
        all_traits: remote_program.traits.clone(),
        imported_modules: Default::default(),
    };
    tools_namespace.modules.insert(
        "inner".to_string(),
        crate::sema::ModuleNamespace {
            name: "inner".to_string(),
            path: "pkg.tools.inner".to_string(),
            source_path: None,
            modules: Default::default(),
            functions: Default::default(),
            classes: Default::default(),
            enums: Default::default(),
            traits: Default::default(),
            trait_impls: Vec::new(),
            all_functions: Default::default(),
            all_classes: Default::default(),
            all_enums: Default::default(),
            all_traits: Default::default(),
            imported_modules: Default::default(),
        },
    );
    program.imported_modules.insert(
        "pkg".to_string(),
        crate::sema::ModuleNamespace {
            name: "pkg".to_string(),
            path: "pkg".to_string(),
            source_path: None,
            modules: std::collections::BTreeMap::from([(
                "tools".to_string(),
                tools_namespace.clone(),
            )]),
            functions: Default::default(),
            classes: Default::default(),
            enums: Default::default(),
            traits: Default::default(),
            trait_impls: Vec::new(),
            all_functions: Default::default(),
            all_classes: Default::default(),
            all_enums: Default::default(),
            all_traits: Default::default(),
            imported_modules: Default::default(),
        },
    );
    let builder = AnalysisBuilder::new(&source, &program, Vec::new());
    assert!(builder.complete(100, 0, Some('.')).unwrap().is_empty());
    let unresolved_completion_program = checked_program("def main():\n    pass\n");
    let unresolved_completion_builder =
        AnalysisBuilder::new("missing.", &unresolved_completion_program, Vec::new());
    assert!(unresolved_completion_builder
        .complete(0, "missing.".len(), Some('.'))
        .unwrap()
        .is_empty());

    let top_level_names = builder
        .top_level_completions()
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(top_level_names.contains(&"User".to_string()));
    assert!(top_level_names.contains(&"Status".to_string()));
    assert!(top_level_names.contains(&"Show".to_string()));
    assert!(top_level_names.contains(&"Result".to_string()));
    assert!(top_level_names.contains(&"SendError".to_string()));
    assert!(top_level_names.contains(&"pkg".to_string()));

    let send_error_symbol = builder
        .resolve_name("SendError", &BTreeMap::new())
        .expect("builtin SendError should resolve");
    assert!(send_error_symbol.hover.contains("SendError[T]"));
    assert!(send_error_symbol.definition.is_none());

    let trait_bound_names = builder
        .trait_bound_member_completions(&[
            TraitBound {
                trait_name: "Missing".to_string(),
                trait_args: Vec::new(),
            },
            TraitBound {
                trait_name: "Show".to_string(),
                trait_args: Vec::new(),
            },
            TraitBound {
                trait_name: "Show".to_string(),
                trait_args: Vec::new(),
            },
        ])
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert_eq!(
        trait_bound_names
            .iter()
            .filter(|name| name.as_str() == "show")
            .count(),
        1
    );

    let module_names = builder
        .member_completions(&Type::Module("pkg.tools".to_string()))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(module_names.contains(&"inner".to_string()));
    assert!(module_names.contains(&"remote_fn".to_string()));
    assert!(module_names.contains(&"Remote".to_string()));
    assert!(module_names.contains(&"RemoteStatus".to_string()));
    assert!(module_names.contains(&"RemoteTrait".to_string()));
    assert!(
        builder
            .resolve_member_type(&Type::Module("pkg.tools".to_string()), "missing")
            .is_none(),
        "unknown module members should not resolve"
    );

    let remote_trait_member = builder
        .resolve_member_type(&Type::Module("pkg.tools".to_string()), "RemoteTrait")
        .expect("qualified imported traits should resolve as module members");
    assert!(remote_trait_member.hover.contains("trait RemoteTrait"));
    assert!(remote_trait_member.definition.is_some());
    assert_eq!(remote_trait_member.ty, None);

    let user_member_names = builder
        .member_completions(&Type::named("User"))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(user_member_names.contains(&"label".to_string()));
    assert!(user_member_names.contains(&"greet".to_string()));
    assert!(user_member_names.contains(&"show".to_string()));

    let status_member_names = builder
        .member_completions(&Type::named("Status"))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(status_member_names.contains(&"Ready".to_string()));
    assert!(status_member_names.contains(&"Failed".to_string()));
    assert_eq!(
        builder
            .member_completions(&Type::named("Status"))
            .into_iter()
            .find(|completion| completion.name == "Failed")
            .map(|completion| completion.detail),
        Some("Failed(code: own int32, reason: own String) -> Status".to_string())
    );
    let ready_member = builder
        .resolve_member_type(&Type::named("Status"), "Ready")
        .expect("enum variants should resolve as static members");
    assert!(ready_member.hover.contains("Status"));
    assert!(ready_member.hover.contains("Ready"));
    assert!(ready_member.definition.is_some());
    assert!(
        builder
            .resolve_member_type(&Type::named("Status"), "Missing")
            .is_none(),
        "unknown enum variants should not resolve"
    );
    let option_member_names = builder
        .member_completions(&Type::named("Option"))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(option_member_names.contains(&"Some".to_string()));
    assert!(option_member_names.contains(&"None".to_string()));
    let local_status = builder
        .resolve_match_variant_enum("Status")
        .expect("local enum should resolve as a match variant enum");
    assert!(local_status.hover.contains("enum Status"));
    assert!(local_status.definition.is_some());
    let inferred_status_variant = builder
        .resolve_match_variant(
            Some(&Type::named("Status")),
            &VariantPattern {
                enum_name: None,
                variant_name: "Ready".to_string(),
                subpatterns: Vec::new(),
                span: Span::new(1, 1),
            },
        )
        .expect("inferred user enum variants should resolve in match patterns");
    assert!(inferred_status_variant
        .hover
        .contains("variant Ready -> Status"));
    assert!(inferred_status_variant.definition.is_some());
    let imported_variant = builder
        .resolve_match_variant(
            Some(&Type::named("pkg.tools.RemoteStatus")),
            &VariantPattern {
                enum_name: Some("pkg.tools.RemoteStatus".to_string()),
                variant_name: "Failed".to_string(),
                subpatterns: Vec::new(),
                span: Span::new(1, 1),
            },
        )
        .expect("qualified imported enum variants should resolve in match patterns");
    assert!(imported_variant
        .hover
        .contains("variant Failed(code: own int32, reason: own String) -> RemoteStatus"));
    assert!(imported_variant.definition.is_some());
    assert_eq!(
        builder
            .member_completions(&Type::named("pkg.tools.RemoteStatus"))
            .into_iter()
            .find(|completion| completion.name == "Failed")
            .map(|completion| completion.detail),
        Some("Failed(code: own int32, reason: own String) -> RemoteStatus".to_string())
    );
    assert!(builder
        .resolve_member_type(&Type::named("pkg.tools.RemoteStatus"), "Failed")
        .expect("qualified imported enum variants should resolve as static members")
        .hover
        .contains("variant Failed(code: own int32, reason: own String) -> RemoteStatus"));
    let remote_status = builder
        .resolve_match_variant_enum("pkg.tools.RemoteStatus")
        .expect("qualified imported enum should resolve as a match variant enum");
    assert!(remote_status.hover.contains("enum RemoteStatus"));
    assert!(remote_status.definition.is_some());
    assert!(builder
        .resolve_match_variant_enum("SendError")
        .expect("builtin SendError should resolve as a match variant enum")
        .hover
        .contains("SendError[T]"));
    assert!(builder
        .resolve_member_type(&Type::named("WaitAny"), "Ready")
        .expect("WaitAny.Ready should resolve")
        .hover
        .contains("variant Ready(own int32, own T) -> WaitAny"));
    assert!(builder
        .resolve_member_type(&Type::named("WaitAny"), "Error")
        .expect("WaitAny.Error should resolve")
        .hover
        .contains("variant Error(own int32, own String) -> WaitAny"));
    assert!(builder
        .resolve_member_type(&Type::named("WaitAll"), "Ready")
        .expect("WaitAll.Ready should resolve")
        .hover
        .contains("variant Ready(own Vec[T]) -> WaitAll"));
    assert!(builder
        .resolve_member_type(&Type::named("WaitAll"), "Error")
        .expect("WaitAll.Error should resolve")
        .hover
        .contains("variant Error(own int32, own String) -> WaitAll"));
    assert_eq!(
        builtin_enum_variant_completions("WaitAny")
            .into_iter()
            .find(|completion| completion.name == "Ready")
            .map(|completion| completion.detail),
        Some("Ready(own int32, own T) -> WaitAny".to_string())
    );
    assert_eq!(
        builtin_enum_variant_completions("WaitAll")
            .into_iter()
            .find(|completion| completion.name == "Error")
            .map(|completion| completion.detail),
        Some("Error(own int32, own String) -> WaitAll".to_string())
    );

    let string_member_names = builder
        .member_completions(&Type::named("String"))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(string_member_names.contains(&"split".to_string()));
    assert!(string_member_names.contains(&"trim".to_string()));
    assert!(string_member_names.contains(&"strip_prefix".to_string()));

    let map_entry_member_names = builder
        .member_completions(&Type::Named(
            "MapEntry".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(map_entry_member_names.contains(&"key".to_string()));
    assert!(map_entry_member_names.contains(&"value".to_string()));
    assert!(
        builder
            .resolve_member_type(
                &Type::Named(
                    "MapEntry".to_string(),
                    vec![Type::named("String"), Type::named("int32")],
                ),
                "missing",
            )
            .is_none(),
        "unknown MapEntry fields should not resolve"
    );

    let task_group_member_names = builder
        .member_completions(&Type::named("TaskGroup"))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(task_group_member_names.contains(&"start".to_string()));
    assert!(task_group_member_names.contains(&"start_soon".to_string()));

    let assert_resolved_member =
        |receiver: Type, field: &str, hover_fragment: &str, expected_ty: Type| {
            let member = builder
                .resolve_member_type(&receiver, field)
                .unwrap_or_else(|| panic!("expected {receiver}.{field} to resolve"));
            assert!(
                member.hover.contains(hover_fragment),
                "hover for {receiver}.{field} should mention {hover_fragment}: {}",
                member.hover
            );
            assert_eq!(member.definition, None);
            assert_eq!(member.ty, Some(expected_ty), "{receiver}.{field}");
        };
    assert_resolved_member(
        Type::named("TaskGroup"),
        "start",
        "Task[T]",
        Type::Named("Task".to_string(), vec![Type::Unit]),
    );
    assert_resolved_member(
        Type::named("TaskGroup"),
        "start_soon",
        "start_soon",
        Type::Unit,
    );
    assert_resolved_member(Type::named("Option"), "Some", "Some", Type::named("Option"));
    assert_resolved_member(Type::named("Option"), "None", "None", Type::named("Option"));
    assert_resolved_member(Type::named("Result"), "Ok", "Ok", Type::named("Result"));
    assert_resolved_member(Type::named("Result"), "Err", "Err", Type::named("Result"));
    assert_resolved_member(
        Type::named("SendError"),
        "Closed",
        "Closed",
        Type::named("SendError"),
    );
    assert_resolved_member(
        Type::named("QueueReceive"),
        "Item",
        "Item",
        Type::named("QueueReceive"),
    );
    assert_resolved_member(
        Type::named("QueueReceive"),
        "TimedOut",
        "TimedOut",
        Type::named("QueueReceive"),
    );
    assert_resolved_member(
        Type::named("TaskResult"),
        "Ready",
        "Ready",
        Type::named("TaskResult"),
    );
    assert_resolved_member(
        Type::named("TaskResult"),
        "Error",
        "Error",
        Type::named("TaskResult"),
    );
    assert_resolved_member(
        Type::named("TaskResult"),
        "Cancelled",
        "Cancelled",
        Type::named("TaskResult"),
    );
    assert_resolved_member(
        Type::named("WaitAny"),
        "Ready",
        "Ready",
        Type::named("WaitAny"),
    );
    assert_resolved_member(
        Type::named("WaitAny"),
        "Error",
        "Error",
        Type::named("WaitAny"),
    );
    assert_resolved_member(
        Type::named("WaitAny"),
        "TimedOut",
        "TimedOut",
        Type::named("WaitAny"),
    );
    assert_resolved_member(
        Type::named("WaitAll"),
        "Ready",
        "Ready",
        Type::named("WaitAll"),
    );
    assert_resolved_member(
        Type::named("WaitAll"),
        "Error",
        "Error",
        Type::named("WaitAll"),
    );
    assert_resolved_member(
        Type::named("WaitAll"),
        "Cancelled",
        "Cancelled",
        Type::named("WaitAll"),
    );

    let scope = BTreeMap::from([
        (
            "numbers".to_string(),
            super::BindingInfo {
                ty: Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                trait_bounds: Vec::new(),
                definition: super::AnalysisRange {
                    file_path: None,
                    line: 0,
                    start_character: 0,
                    end_character: 7,
                },
                hover: "binding numbers: Vec[int32]".to_string(),
            },
        ),
        (
            "mapping".to_string(),
            super::BindingInfo {
                ty: Type::Named(
                    "Map".to_string(),
                    vec![Type::named("String"), Type::named("int32")],
                ),
                trait_bounds: Vec::new(),
                definition: super::AnalysisRange {
                    file_path: None,
                    line: 0,
                    start_character: 0,
                    end_character: 7,
                },
                hover: "binding mapping: Map[String, int32]".to_string(),
            },
        ),
        (
            "task".to_string(),
            super::BindingInfo {
                ty: Type::Named("Task".to_string(), vec![Type::named("int32")]),
                trait_bounds: Vec::new(),
                definition: super::AnalysisRange {
                    file_path: None,
                    line: 0,
                    start_character: 0,
                    end_character: 4,
                },
                hover: "binding task: Task[int32]".to_string(),
            },
        ),
        (
            "tasks".to_string(),
            super::BindingInfo {
                ty: Type::Named(
                    "Vec".to_string(),
                    vec![Type::Named("Task".to_string(), vec![Type::named("int32")])],
                ),
                trait_bounds: Vec::new(),
                definition: super::AnalysisRange {
                    file_path: None,
                    line: 0,
                    start_character: 0,
                    end_character: 5,
                },
                hover: "binding tasks: Vec[Task[int32]]".to_string(),
            },
        ),
    ]);

    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::List(vec![
                expr(ExprKind::Int(1)),
                expr(ExprKind::Int(2))
            ])),
            &scope,
        ),
        Some(Type::Named("Vec".to_string(), vec![Type::named("int64")]))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Set(vec![
                expr(ExprKind::String("a".to_string())),
                expr(ExprKind::String("b".to_string())),
            ])),
            &scope,
        ),
        Some(Type::Named("Set".to_string(), vec![Type::named("String")]))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Map(vec![crate::ast::MapEntryExpr {
                key: expr(ExprKind::String("a".to_string())),
                value: expr(ExprKind::Int(1)),
            }])),
            &scope,
        ),
        Some(Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int64")],
        ))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("wait_any".to_string()))),
                args: vec![arg(expr(ExprKind::Name("tasks".to_string())))],
            }),
            &scope,
        ),
        Some(Type::Named(
            "WaitAny".to_string(),
            vec![Type::named("int32")],
        ))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("wait_all".to_string()))),
                args: vec![arg(expr(ExprKind::Name("tasks".to_string())))],
            }),
            &scope,
        ),
        Some(Type::Named(
            "WaitAll".to_string(),
            vec![Type::named("int32")],
        ))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("task".to_string()))),
                    field: "result".to_string(),
                })),
                args: Vec::new(),
            }),
            &scope,
        ),
        Some(Type::Named(
            "TaskResult".to_string(),
            vec![Type::named("int32")],
        ))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("task".to_string()))),
                    field: "result_or_none".to_string(),
                })),
                args: Vec::new(),
            }),
            &scope,
        ),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("int32")],
        ))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("resultify".to_string()))),
                args: vec![arg(expr(ExprKind::Int(7)))],
            })))),
            &scope,
        ),
        Some(Type::named("int32"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Index {
                object: Box::new(expr(ExprKind::Name("numbers".to_string()))),
                index: Box::new(expr(ExprKind::Int(0))),
            }),
            &scope,
        ),
        Some(Type::named("int32"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Index {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                index: Box::new(expr(ExprKind::String("a".to_string()))),
            }),
            &scope,
        ),
        Some(Type::named("int32"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Cast {
                expr: Box::new(expr(ExprKind::Int(1))),
                ty: type_ref("String"),
            }),
            &scope,
        ),
        Some(Type::named("String"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Unary {
                op: crate::ast::UnaryOp::Not,
                expr: Box::new(expr(ExprKind::Bool(false))),
            }),
            &scope,
        ),
        Some(Type::named("bool"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Unary {
                op: crate::ast::UnaryOp::Neg,
                expr: Box::new(expr(ExprKind::Float(1.5))),
            }),
            &scope,
        ),
        Some(Type::named("float64"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Group(Box::new(expr(ExprKind::Int(3))))),
            &scope
        ),
        Some(Type::named("int64"))
    );
    assert_eq!(
        builder.infer_expr_type(&expr(ExprKind::Name("pkg".to_string())), &scope),
        Some(Type::Module("pkg".to_string()))
    );
    assert_eq!(
        builder.infer_expr_type(&expr(ExprKind::Name("Status".to_string())), &scope),
        Some(Type::named("Status"))
    );
    assert_eq!(
        builder.infer_expr_type(&expr(ExprKind::Name("helper".to_string())), &scope),
        Some(Type::named("int32"))
    );
    for builtin_name in [
        "SendError",
        "QueueReceive",
        "TaskResult",
        "WaitAny",
        "WaitAll",
        "Queue",
        "TaskGroup",
    ] {
        assert_eq!(
            builder.infer_expr_type(&expr(ExprKind::Name(builtin_name.to_string())), &scope),
            Some(Type::named(builtin_name)),
            "{builtin_name} should infer as a builtin type constructor"
        );
    }
    for (builtin_name, args) in [
        ("SendError", vec![type_ref("int32")]),
        ("Queue", vec![type_ref("String")]),
        ("Vec", vec![type_ref("int32")]),
        ("Set", vec![type_ref("String")]),
        ("Map", vec![type_ref("String"), type_ref("int32")]),
        ("Task", vec![type_ref("int32")]),
    ] {
        assert_eq!(
            builder.infer_expr_type(
                &expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name(builtin_name.to_string()))),
                    type_args: args.clone(),
                }),
                &scope,
            ),
            Some(Type::Named(
                builtin_name.to_string(),
                args.into_iter().map(|arg| lower_type_ref(&arg)).collect(),
            )),
            "{builtin_name} specialization should infer its generic type"
        );
    }
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("helper".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            &scope,
        ),
        Some(Type::named("int32"))
    );
    assert_eq!(Type::Unit.type_arguments(), &[]);
    assert_eq!(Type::Module("pkg".to_string()).type_arguments(), &[]);
    assert_eq!(Type::TypeParam("T".to_string()).type_arguments(), &[]);
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("numbers".to_string()))),
                field: "len".to_string(),
            }),
            &scope,
        ),
        Some(Type::named("int32"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Match {
                scrutinee: Box::new(expr(ExprKind::Name("numbers".to_string()))),
                borrow_mode: None,
                arms: vec![crate::ast::MatchExprArm {
                    pattern: crate::ast::Pattern::Wildcard(Span::new(1, 1)),
                    value: expr(ExprKind::Int(4)),
                    span: Span::new(1, 1),
                }],
            }),
            &scope,
        ),
        Some(Type::named("int64"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Int(1))))),
            &scope,
        ),
        None
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Index {
                object: Box::new(expr(ExprKind::String("abc".to_string()))),
                index: Box::new(expr(ExprKind::Int(0))),
            }),
            &scope,
        ),
        None
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Binary {
                op: BinaryOp::And,
                left: Box::new(expr(ExprKind::Bool(true))),
                right: Box::new(expr(ExprKind::Bool(false))),
            }),
            &scope,
        ),
        Some(Type::named("bool"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Binary {
                op: BinaryOp::Eq,
                left: Box::new(expr(ExprKind::Int(1))),
                right: Box::new(expr(ExprKind::Int(1))),
            }),
            &scope,
        ),
        Some(Type::named("bool"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Binary {
                op: BinaryOp::Add,
                left: Box::new(expr(ExprKind::Int(1))),
                right: Box::new(expr(ExprKind::Float(2.0))),
            }),
            &scope,
        ),
        Some(Type::named("float64"))
    );
    assert_eq!(
        builder.infer_expr_type(
            &expr(ExprKind::Binary {
                op: BinaryOp::Add,
                left: Box::new(expr(ExprKind::String("left".to_string()))),
                right: Box::new(expr(ExprKind::Int(2))),
            }),
            &scope,
        ),
        None
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Name("wait_any".to_string())),
            &[arg(expr(ExprKind::Name("numbers".to_string())))],
            &scope,
        ),
        None
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Name("wait_any".to_string())),
            &[arg(expr(ExprKind::Int(1)))],
            &scope,
        ),
        None
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Name("wait_all".to_string())),
            &[arg(expr(ExprKind::Name("numbers".to_string())))],
            &scope,
        ),
        None
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Name("wait_all".to_string())),
            &[arg(expr(ExprKind::Int(1)))],
            &scope,
        ),
        None
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("Option".to_string()))),
                field: "Some".to_string(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            &scope,
        ),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("int64")]
        ))
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("Result".to_string()))),
                field: "Err".to_string(),
            }),
            &[arg(expr(ExprKind::String("no".to_string())))],
            &scope,
        ),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::Unit, Type::named("String")],
        ))
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Task".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            &[],
            &scope,
        ),
        Some(Type::Named("Task".to_string(), vec![Type::named("int32")]))
    );
    assert_eq!(
        builder.infer_call_type(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("helper".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            &[],
            &scope,
        ),
        Some(Type::named("int32"))
    );
    assert_eq!(
        builder.infer_call_type(&expr(ExprKind::Int(1)), &[], &scope),
        None
    );
    assert_eq!(
        builder.infer_iterable_binding_type(
            &expr(ExprKind::Set(vec![expr(ExprKind::String("a".to_string()))])),
            &scope,
        ),
        Some(Type::named("String"))
    );
    assert_eq!(
        builder.match_binding_type(
            Some(&Type::Named(
                "Result".to_string(),
                vec![Type::named("int32"), Type::named("String")],
            )),
            None,
            "Err",
        ),
        Some(Type::named("String"))
    );
    assert_eq!(
        builder.match_binding_type(
            Some(&Type::Named(
                "Option".to_string(),
                vec![Type::named("int32")],
            )),
            None,
            "Some",
        ),
        Some(Type::named("int32"))
    );
    assert_eq!(
        builder.match_binding_type(
            Some(&Type::Named(
                "Result".to_string(),
                vec![Type::named("int32"), Type::named("String")],
            )),
            None,
            "Ok",
        ),
        Some(Type::named("int32"))
    );
    assert_eq!(
        builder.match_binding_type(
            Some(&Type::Named(
                "SendError".to_string(),
                vec![Type::named("String")],
            )),
            None,
            "Closed",
        ),
        Some(Type::named("String"))
    );
}

#[test]
fn analysis_import_and_match_resolution_helpers_cover_fallbacks() {
    let source = [
        "import pkg.types",
        "",
        "enum Status:",
        "    Ready",
        "    Failed(String)",
        "",
        "def inspect(status: own Status, value: Option[int32]) -> int32:",
        "    match status:",
        "        case Status.Ready:",
        "            return 1",
        "        case Status.Failed(reason):",
        "            return 2",
        "    match value:",
        "        case Some(found):",
        "            return found",
        "        case None:",
        "            return 0",
    ]
    .join("\n");
    let mut program = checked_program(&source);
    program.source_path = Some("/tmp/main.au".to_string());
    program.imported_modules.insert(
        "pkg".to_string(),
        crate::sema::ModuleNamespace {
            name: "pkg".to_string(),
            path: "pkg".to_string(),
            source_path: None,
            modules: std::collections::BTreeMap::from([(
                "types".to_string(),
                crate::sema::ModuleNamespace {
                    name: "types".to_string(),
                    path: "pkg.types".to_string(),
                    source_path: None,
                    modules: Default::default(),
                    functions: Default::default(),
                    classes: Default::default(),
                    enums: Default::default(),
                    traits: Default::default(),
                    trait_impls: Vec::new(),
                    all_functions: Default::default(),
                    all_classes: Default::default(),
                    all_enums: Default::default(),
                    all_traits: Default::default(),
                    imported_modules: Default::default(),
                },
            )]),
            functions: Default::default(),
            classes: Default::default(),
            enums: Default::default(),
            traits: Default::default(),
            trait_impls: Vec::new(),
            all_functions: Default::default(),
            all_classes: Default::default(),
            all_enums: Default::default(),
            all_traits: Default::default(),
            imported_modules: Default::default(),
        },
    );

    let builder = AnalysisBuilder::new(&source, &program, Vec::new());
    assert_eq!(
        builder.current_source_path().as_deref(),
        Some("/tmp/main.au")
    );
    let import_range = builder
        .find_imported_module_range("pkg.types")
        .expect("import range should fall back to current file");
    assert_eq!(import_range.file_path.as_deref(), Some("/tmp/main.au"));
    assert_eq!(import_range.line, 0);
    assert!(
        builder
            .find_imported_module_range("pkg.types.inner")
            .is_none(),
        "longer target paths should not match shorter imports"
    );
    let mismatched_source_builder =
        AnalysisBuilder::new("def other():\n    pass\n", &program, vec![]);
    assert!(
        mismatched_source_builder
            .find_imported_module_range("pkg.types")
            .is_none(),
        "fallback import ranges require the token to be present on the source line"
    );

    let option_symbol = builder
        .resolve_match_variant_enum("Option")
        .expect("builtin Option enum should resolve");
    assert!(option_symbol.definition.is_none());
    assert!(option_symbol.hover.contains("Option[T]"));

    let status_symbol = builder
        .resolve_match_variant_enum("Status")
        .expect("named enum should resolve");
    assert!(status_symbol.definition.is_some());
    assert!(status_symbol.hover.contains("enum Status"));

    let builtin_variant = builder
        .resolve_match_variant(
            Some(&Type::Named(
                "Option".to_string(),
                vec![Type::named("int32")],
            )),
            &VariantPattern {
                enum_name: None,
                variant_name: "Some".to_string(),
                subpatterns: vec![crate::ast::Pattern::Binding(crate::ast::BindingPattern {
                    name: "found".to_string(),
                    span: Span::new(14, 14),
                })],
                span: Span::new(14, 14),
            },
        )
        .expect("builtin variant should resolve");
    assert!(builtin_variant.definition.is_none());
    assert!(builtin_variant.hover.contains("Some"));
    assert!(builtin_variant.hover.contains("int32"));

    let named_variant = builder
        .resolve_match_variant(
            Some(&Type::named("Status")),
            &VariantPattern {
                enum_name: Some("Status".to_string()),
                variant_name: "Failed".to_string(),
                subpatterns: vec![crate::ast::Pattern::Binding(crate::ast::BindingPattern {
                    name: "reason".to_string(),
                    span: Span::new(10, 14),
                })],
                span: Span::new(10, 14),
            },
        )
        .expect("named enum variant should resolve");
    assert!(named_variant.definition.is_some());
    assert!(named_variant.hover.contains("Failed"));
    assert!(named_variant.hover.contains("String"));

    let inferred_named_variant = builder
        .resolve_match_variant(
            Some(&Type::named("Status")),
            &VariantPattern {
                enum_name: None,
                variant_name: "Failed".to_string(),
                subpatterns: Vec::new(),
                span: Span::new(10, 14),
            },
        )
        .expect("scrutinee-inferred enum variants should resolve");
    assert!(inferred_named_variant.definition.is_some());

    let result_err_variant = builder
        .resolve_match_variant(
            Some(&Type::Named(
                "Result".to_string(),
                vec![Type::named("int32"), Type::named("String")],
            )),
            &VariantPattern {
                enum_name: None,
                variant_name: "Err".to_string(),
                subpatterns: Vec::new(),
                span: Span::new(12, 14),
            },
        )
        .expect("builtin Result.Err should resolve");
    assert!(result_err_variant.hover.contains("String"));

    let send_cancelled_variant = builder
        .resolve_match_variant(
            Some(&Type::Named(
                "SendError".to_string(),
                vec![Type::named("int32")],
            )),
            &VariantPattern {
                enum_name: None,
                variant_name: "Cancelled".to_string(),
                subpatterns: Vec::new(),
                span: Span::new(13, 14),
            },
        )
        .expect("builtin SendError.Cancelled should resolve");
    assert!(send_cancelled_variant.hover.contains("int32"));

    assert!(
        builder
            .resolve_match_variant(
                Some(&Type::Named(
                    "Option".to_string(),
                    vec![Type::named("int32")],
                )),
                &VariantPattern {
                    enum_name: None,
                    variant_name: "Missing".to_string(),
                    subpatterns: Vec::new(),
                    span: Span::new(14, 14),
                },
            )
            .is_none(),
        "unknown builtin enum variants should fall through to named enum resolution"
    );
}

#[test]
fn analysis_completion_helpers_cover_top_level_module_and_enum_surfaces() {
    let source = [
        "import pkg",
        "",
        "trait Show:",
        "    def show(borrow self) -> String",
        "",
        "enum Status:",
        "    Ready",
        "    Failed(String)",
        "",
        "class Local:",
        "    value: int32",
        "",
        "def helper() -> int32:",
        "    return 1",
    ]
    .join("\n");
    let mut program = checked_program(&source);
    let remote_source = [
        "trait RemoteTrait:",
        "    def show(borrow self) -> String",
        "",
        "enum RemoteStatus:",
        "    Ready",
        "",
        "class Remote:",
        "    value: int32",
        "",
        "def remote_fn() -> int32:",
        "    return 7",
    ]
    .join("\n");
    let remote_program = checked_program(&remote_source);
    let tools_namespace = crate::sema::ModuleNamespace {
        name: "tools".to_string(),
        path: "pkg.tools".to_string(),
        source_path: None,
        modules: Default::default(),
        functions: remote_program.functions.clone(),
        classes: remote_program.classes.clone(),
        enums: remote_program.enums.clone(),
        traits: remote_program.traits.clone(),
        trait_impls: remote_program.trait_impls.clone(),
        all_functions: remote_program.functions.clone(),
        all_classes: remote_program.classes.clone(),
        all_enums: remote_program.enums.clone(),
        all_traits: remote_program.traits.clone(),
        imported_modules: Default::default(),
    };
    program.imported_modules.insert(
        "pkg".to_string(),
        crate::sema::ModuleNamespace {
            name: "pkg".to_string(),
            path: "pkg".to_string(),
            source_path: None,
            modules: std::collections::BTreeMap::from([(
                "tools".to_string(),
                tools_namespace.clone(),
            )]),
            functions: Default::default(),
            classes: Default::default(),
            enums: Default::default(),
            traits: Default::default(),
            trait_impls: Vec::new(),
            all_functions: Default::default(),
            all_classes: Default::default(),
            all_enums: Default::default(),
            all_traits: Default::default(),
            imported_modules: Default::default(),
        },
    );

    let builder = AnalysisBuilder::new(&source, &program, Vec::new());
    let top_level_names = builder
        .top_level_completions()
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(top_level_names.contains(&"Local".to_string()));
    assert!(top_level_names.contains(&"Status".to_string()));
    assert!(top_level_names.contains(&"Show".to_string()));
    assert!(top_level_names.contains(&"helper".to_string()));
    assert!(top_level_names.contains(&"print".to_string()));
    assert!(top_level_names.contains(&"Option".to_string()));
    assert!(top_level_names.contains(&"pkg".to_string()));

    let module_names = builder
        .member_completions(&Type::Module("pkg.tools".to_string()))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(module_names.contains(&"remote_fn".to_string()));
    assert!(module_names.contains(&"Remote".to_string()));
    assert!(module_names.contains(&"RemoteStatus".to_string()));
    assert!(module_names.contains(&"RemoteTrait".to_string()));
    assert!(
        builder
            .member_completions(&Type::Module("pkg.missing".to_string()))
            .is_empty(),
        "unknown module namespaces should complete to an empty member list"
    );

    let enum_names = builder
        .member_completions(&Type::named("Status"))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(enum_names.contains(&"Ready".to_string()));
    assert!(enum_names.contains(&"Failed".to_string()));

    assert_eq!(
        builder.match_binding_type(None, Some("Status"), "Failed"),
        Some(Type::named("String"))
    );
    assert_eq!(
        builder.match_binding_type(
            Some(&Type::Named(
                "SendError".to_string(),
                vec![Type::named("int32")]
            )),
            None,
            "Cancelled"
        ),
        Some(Type::named("int32"))
    );
}

#[test]
fn complete_path_source_recovers_imported_module_member_completion() {
    let temp = TempDir::new("analysis-complete-path");
    fs::create_dir_all(temp.path().join("helpers")).expect("should create helper module dir");
    fs::write(
        temp.path().join("helpers").join("math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("should write helper module");

    let source = "import helpers.math\n\ndef main() -> int32:\n    helpers.math.\n    return 0\n";
    let path = temp.path().join("main.au");
    fs::write(&path, source).expect("should write main module");
    let line_index = source
        .lines()
        .position(|line| line.contains("helpers.math."))
        .expect("source should contain member access");
    let line_text = source.lines().nth(line_index).unwrap();
    let character = line_text.rfind('.').unwrap() + 1;

    let completions = complete_path_source(&path, source, line_index, character, Some('.'))
        .expect("path-aware completion should recover");
    let names = completions
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"double".to_string()));
}

#[test]
fn completion_scope_tracks_nested_statement_bindings() {
    let source = [
        "class FileHandle:",
        "    name: String",
        "",
        "    def close(borrow mut self):",
        "        pass",
        "",
        "class Counter:",
        "    value: int32",
        "",
        "    def inspect(borrow self) -> int32:",
        "        print(self.value)",
        "        return self.value",
        "",
        "def scoped(value: int32) -> int32:",
        "    jobs = Queue[int32]()",
        "    if value > 0:",
        "        positive = value",
        "        print(positive)",
        "    else:",
        "        negative = value",
        "        print(negative)",
        "    match value:",
        "        case 0:",
        "            zero = value",
        "            print(zero)",
        "        case _:",
        "            wildcard = value",
        "            print(wildcard)",
        "    for item in [1, 2, 3]:",
        "        print(item)",
        "    with TaskGroup() as group:",
        "        print(group.cancel())",
        "    match jobs.get(timeout=1ms):",
        "        case QueueReceive.Item(received):",
        "            print(received)",
        "        case _:",
        "            pass",
        "    while value > 0:",
        "        loop_value = value",
        "        print(loop_value)",
        "        break",
        "    return value",
        "",
        "top = 1",
        "print(top)",
    ]
    .join("\n");

    let program = checked_program(&source);
    let builder = AnalysisBuilder::new(&source, &program, Vec::new());
    let checks = [
        ("print(self.value)", "self"),
        ("print(positive)", "positive"),
        ("print(negative)", "negative"),
        ("print(zero)", "zero"),
        ("print(wildcard)", "wildcard"),
        ("print(item)", "item"),
        ("print(group.cancel())", "group"),
        ("print(received)", "received"),
        ("print(loop_value)", "loop_value"),
        ("print(top)", "top"),
    ];

    for (needle, expected) in checks {
        let line_index = source
            .lines()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("source should contain `{needle}`"));
        let completions = builder.scope_for_line(line_index);
        assert!(
            completions.contains_key(expected),
            "completion scope for `{needle}` should include `{expected}`"
        );
    }
}

#[test]
fn compiler_analysis_accepts_builtin_named_arguments() {
    let source = include_str!("../../../examples/basics/named_builtin_arguments.au");
    let analysis = analyze_source(source);

    assert!(analysis.diagnostics.is_empty());
}

#[test]
fn compiler_analysis_handles_named_wait_any_timeout() {
    let source = "def worker(value: int32) -> int32:\n    return value\n\ndef main() -> int32:\n    with TaskGroup() as group:\n        mut tasks = Vec[Task[int32]]()\n        tasks.push(group.start(worker, 1))\n        print(wait_any(tasks, timeout=5ms))\n    return 0\n";
    let analysis = analyze_source(source);

    assert!(analysis.diagnostics.is_empty());
    assert!(analysis.occurrences.iter().any(|occurrence| occurrence
        .hover
        .contains("wait_any(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAny[T]")));
}

#[test]
fn compiler_analysis_preserves_real_ownership_diagnostic_metadata() {
    let source = "def take(value: String) -> String:\n    return value\n";
    let analysis = analyze_source(source);

    assert_eq!(analysis.diagnostics.len(), 1);
    let diagnostic = &analysis.diagnostics[0];
    assert_eq!(diagnostic.code, "AU3002");
    assert_eq!((diagnostic.line, diagnostic.start_character), (1, 11));
    assert_eq!(diagnostic.secondary_spans.len(), 1);
    assert_eq!(
        (
            diagnostic.secondary_spans[0].line,
            diagnostic.secondary_spans[0].start_character,
            diagnostic.secondary_spans[0].label.as_str(),
        ),
        (0, 9, "parameter `value` is borrowed here")
    );
    assert_eq!(
        diagnostic.help,
        ["declare the parameter as `own String` when the function should consume it, or call `.clone()` to consume an independent copy"]
    );
    assert_eq!(diagnostic.edits.len(), 1);
    assert_eq!(
        (
            diagnostic.edits[0].line,
            diagnostic.edits[0].start_character,
            diagnostic.edits[0].end_character,
            diagnostic.edits[0].replacement.as_str(),
            diagnostic.edits[0].applicability.as_str(),
        ),
        (1, 16, 16, ".clone()", "machine-applicable")
    );
    assert!(
        crate::check_source("def take(value: String) -> String:\n    return value.clone()\n")
            .is_ok()
    );
}

#[test]
fn compiler_analysis_reports_provenance_for_representative_ownership_paths() {
    let cases = [
        (
            "def consume(value: own String):\n    pass\n\ndef main() -> int32:\n    value = \"x\"\n    consume(value)\n    print(value)\n    return 0\n",
            "AU3001",
            "use of moved value",
            true,
        ),
        (
            "def main() -> int32:\n    mut values = [1]\n    for value in borrow values:\n        values.clear()\n    return 0\n",
            "AU3002",
            "borrowed for iteration",
            false,
        ),
        (
            "class Counter:\n    value: int32\n\n    def bump(self):\n        self.value += 1\n",
            "AU3003",
            "shared receiver `self`",
            false,
        ),
        (
            "class Data:\n    value: int32\n\ndef use(r: borrow Data, w: borrow mut Data):\n    pass\n\ndef main() -> int32:\n    mut data = Data(value=1)\n    use(data, data)\n    return 0\n",
            "AU3002",
            "overlaps borrow",
            false,
        ),
    ];

    for (source, code, message_fragment, has_safe_edit) in cases {
        let analysis = analyze_source(source);
        assert_eq!(analysis.diagnostics.len(), 1, "{message_fragment}");
        let diagnostic = &analysis.diagnostics[0];
        assert_eq!(diagnostic.code, code, "{message_fragment}");
        assert!(
            diagnostic.message.contains(message_fragment),
            "{}",
            diagnostic.message
        );
        assert_eq!(diagnostic.secondary_spans.len(), 1, "{message_fragment}");
        assert!(!diagnostic.secondary_spans[0].label.is_empty());
        assert!(!diagnostic.help.is_empty(), "{message_fragment}");
        assert_eq!(!diagnostic.edits.is_empty(), has_safe_edit);
    }
}

#[test]
fn compiler_member_completion_tolerates_dangling_dot_buffers() {
    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    counter.",
        "    return 0",
    ]
    .join("\n");

    let completions = complete_source(&source, 5, 12, Some('.')).expect("completion should work");
    let names = completions
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"value".to_string()));
}

#[test]
fn compiler_member_completion_tolerates_dangling_dot_at_eof_buffers() {
    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    counter.",
    ]
    .join("\n");

    let completions = complete_source(&source, 5, 12, Some('.')).expect("completion should work");
    let names = completions
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"value".to_string()));
}

#[test]
fn compiler_member_completion_tolerates_multiple_dangling_dot_buffers() {
    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    print(counter.",
        "    print(counter.",
        "    return 0",
    ]
    .join("\n");

    let completions = complete_source(&source, 5, 18, Some('.')).expect("completion should work");
    let names = completions
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"value".to_string()));
}

#[test]
fn machine_readable_analysis_recovers_symbols_for_dangling_dot_buffers() {
    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    counter.",
        "    return 0",
    ]
    .join("\n");

    let analysis = analyze_source(&source);

    assert!(!analysis.symbols.is_empty());
    assert!(analysis
        .symbols
        .iter()
        .any(|symbol| symbol.kind == "class" && symbol.name == "Counter"));
    assert!(!analysis.occurrences.is_empty());
}

#[test]
fn machine_readable_analysis_recovers_symbols_for_dangling_dot_at_eof_buffers() {
    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    counter.",
    ]
    .join("\n");

    let analysis = analyze_source(&source);

    assert!(!analysis.symbols.is_empty());
    assert!(analysis
        .symbols
        .iter()
        .any(|symbol| symbol.kind == "class" && symbol.name == "Counter"));
    assert!(!analysis.occurrences.is_empty());
}

#[test]
fn machine_readable_analysis_recovers_symbols_for_multiple_dangling_dot_buffers() {
    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    print(counter.",
        "    print(counter.",
        "    return 0",
    ]
    .join("\n");

    let analysis = analyze_source(&source);

    assert!(!analysis.symbols.is_empty());
    assert!(analysis
        .symbols
        .iter()
        .any(|symbol| symbol.kind == "class" && symbol.name == "Counter"));
    assert!(!analysis.occurrences.is_empty());
}

#[test]
fn path_aware_analysis_tracks_definitions_for_namespace_imported_symbols() {
    let path = repo_root().join("examples/modules/namespace_import_types.au");
    let source = std::fs::read_to_string(&path).expect("example should exist");
    let analysis = analyze_path_source(&path, &source);
    let types_path = fs::canonicalize(repo_root().join("examples/modules/pkg/types.au"))
        .expect("types path should canonicalize")
        .display()
        .to_string();

    assert!(analysis.diagnostics.is_empty());
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.hover.contains("module pkg.types")
            && occurrence
                .definition
                .as_ref()
                .and_then(|definition| definition.file_path.as_deref())
                == Some(types_path.as_str())
    }));
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.hover.contains("class Counter")
            && occurrence
                .definition
                .as_ref()
                .and_then(|definition| definition.file_path.as_deref())
                == Some(types_path.as_str())
    }));
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.hover.contains("enum Status")
            && occurrence
                .definition
                .as_ref()
                .and_then(|definition| definition.file_path.as_deref())
                == Some(types_path.as_str())
    }));
}

#[test]
fn path_aware_analysis_tracks_imported_function_field_and_trait_method_definitions() {
    let temp_dir = TempDir::new("aurora-analysis-cross-file");
    fs::create_dir_all(temp_dir.path().join("pkg")).expect("failed to create pkg dir");
    let math_path = temp_dir.path().join("pkg/math.au");
    let named_path = temp_dir.path().join("pkg/named.au");
    let user_path = temp_dir.path().join("pkg/user.au");
    let main_path = temp_dir.path().join("main.au");

    fs::write(
        &math_path,
        "public def add(left: int32, right: int32) -> int32:\n    return left + right\n",
    )
    .expect("failed to write math module");
    fs::write(
        &named_path,
        "public trait Named:\n    def name(borrow self) -> String\n",
    )
    .expect("failed to write named module");
    fs::write(
        &user_path,
        [
            "from pkg.named import Named",
            "",
            "public class User:",
            "    public label: String",
            "",
            "impl Named for User:",
            "    def name(borrow self) -> String:",
            "        return self.label.clone()",
        ]
        .join("\n"),
    )
    .expect("failed to write user module");
    let source = [
        "from pkg.math import add",
        "from pkg.user import User",
        "",
        "def main() -> int32:",
        "    total = add(left=1, right=2)",
        "    user = User(label=\"Ada\")",
        "    print(user.label)",
        "    print(user.name())",
        "    return total",
    ]
    .join("\n");
    fs::write(&main_path, &source).expect("failed to write main module");

    let analysis = analyze_path_source(&main_path, &source);
    let math_path = fs::canonicalize(&math_path)
        .expect("math path should canonicalize")
        .display()
        .to_string();
    let user_path = fs::canonicalize(&user_path)
        .expect("user path should canonicalize")
        .display()
        .to_string();

    assert!(analysis.diagnostics.is_empty());
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.hover.contains("function add")
            && occurrence
                .definition
                .as_ref()
                .and_then(|definition| definition.file_path.as_deref())
                == Some(math_path.as_str())
    }));
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.hover.contains("class User")
            && occurrence
                .definition
                .as_ref()
                .and_then(|definition| definition.file_path.as_deref())
                == Some(user_path.as_str())
    }));
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.hover.contains("field label: String")
            && occurrence
                .definition
                .as_ref()
                .and_then(|definition| definition.file_path.as_deref())
                == Some(user_path.as_str())
    }));
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.hover.contains("method name(self) -> String")
            && occurrence
                .definition
                .as_ref()
                .and_then(|definition| definition.file_path.as_deref())
                == Some(user_path.as_str())
    }));
}

#[test]
fn analysis_records_variant_occurrences_inside_match_patterns() {
    let source = [
        "enum Status:",
        "    Ready",
        "    Busy",
        "",
        "def render(status: Status) -> int32:",
        "    match status:",
        "        case Status.Ready:",
        "            return 1",
        "        case Status.Busy:",
        "            return 0",
        "",
        "def render_unqualified(status: Status) -> int32:",
        "    match status:",
        "        case Ready:",
        "            return 1",
        "        case Busy:",
        "            return 0",
    ]
    .join("\n");

    let analysis = analyze_source(&source);

    assert!(analysis.diagnostics.is_empty());
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.line == 6
            && occurrence.hover.contains("variant Ready")
            && occurrence.definition.is_some()
    }));
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.line == 6
            && occurrence.hover.contains("enum Status")
            && occurrence.definition.is_some()
    }));
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.line == 8
            && occurrence.hover.contains("variant Busy")
            && occurrence.definition.is_some()
    }));
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.line == 13
            && occurrence.hover.contains("variant Ready")
            && occurrence.definition.is_some()
    }));
    assert!(analysis.occurrences.iter().any(|occurrence| {
        occurrence.line == 15
            && occurrence.hover.contains("variant Busy")
            && occurrence.definition.is_some()
    }));
}

#[test]
fn analysis_member_assignment_without_source_field_range_does_not_emit_occurrence() {
    let assignment_source = [
        "class Counter:",
        "    value: int32",
        "",
        "def update():",
        "    mut counter = Counter(value=0)",
        "    counter.value = 1",
    ]
    .join("\n");
    let assignment_analysis = analyze_source(&assignment_source);
    assert!(assignment_analysis.diagnostics.is_empty());
    assert!(assignment_analysis.occurrences.iter().any(|occurrence| {
        occurrence.line == 5
            && occurrence.hover.contains("field value")
            && occurrence.definition.is_some()
    }));

    let source = [
        "class Counter:",
        "    value: int32",
        "",
        "def update(counter: Counter):",
        "    pass",
    ]
    .join("\n");
    let program = checked_program(&source);
    let mut builder = AnalysisBuilder::new(&source, &program, Vec::new());
    let function_decl = program
        .module
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function_decl) if function_decl.name == "update" => Some(function_decl),
            _ => None,
        })
        .expect("update function should exist");
    let function_info = program
        .functions
        .get("update")
        .expect("update function info should exist");
    let mut scope = builder.function_scope(function_decl, function_info);
    let assignment = AssignStmt {
        mutable: false,
        target: AssignTarget::Member {
            object: Box::new(expr(ExprKind::Name("counter".to_string()))),
            field: "value".to_string(),
        },
        annotation: None,
        op: None,
        value: expr(ExprKind::Int(1)),
        span: Span::new(5, 5),
    };

    builder.visit_assign(&assignment, &mut scope);

    assert!(builder
        .output
        .occurrences
        .iter()
        .all(|occurrence| !occurrence.hover.contains("field value")));

    let unresolved_receiver_assignment = AssignStmt {
        mutable: false,
        target: AssignTarget::Member {
            object: Box::new(expr(ExprKind::Name("missing".to_string()))),
            field: "value".to_string(),
        },
        annotation: None,
        op: None,
        value: expr(ExprKind::Int(2)),
        span: Span::new(5, 5),
    };
    builder.visit_assign(&unresolved_receiver_assignment, &mut scope);

    assert!(builder
        .output
        .occurrences
        .iter()
        .all(|occurrence| !occurrence.hover.contains("missing")));
}

#[test]
fn analysis_helper_functions_cover_formatting_ranges_and_builtin_surface() {
    let diagnostic = analysis_diagnostic(&Diagnostic::at(Span::new(3, 5), "problem"));
    assert_eq!(diagnostic.line, 2);
    assert_eq!(diagnostic.start_character, 4);
    assert_eq!(diagnostic.end_character, 5);

    assert_eq!(
        range_from_span(Span::new(4, 2), 3),
        super::AnalysisRange {
            file_path: None,
            line: 3,
            start_character: 1,
            end_character: 4,
        }
    );
    assert_eq!(
        range_from_span_with_path(Span::new(2, 4), 2, Some("/tmp/example.au".to_string())),
        super::AnalysisRange {
            file_path: Some("/tmp/example.au".to_string()),
            line: 1,
            start_character: 3,
            end_character: 5,
        }
    );
    assert_eq!(
        find_identifier_in_line("value total value2", "value"),
        Some((0, 5))
    );
    assert_eq!(find_identifier_in_line("value2", "value"), None);
    assert_eq!(find_identifier_in_line("prefixvalue suffix", "value"), None);
    assert_eq!(
        find_identifier_in_line("prefix_value value", "value"),
        Some((13, 18))
    );

    assert_eq!(lower_type_ref(&type_ref("None")), Type::Unit);
    assert_eq!(lower_type_ref(&type_ref("str")), Type::named("String"));
    assert_eq!(
        base_type_name(&Type::Module("pkg.types".to_string())),
        "pkg.types"
    );
    assert!(format_value_hover("let", "count", &Type::named("int32")).contains("count: int32"));
    assert!(format_function_hover(&function_decl("total", "int32")).contains("function total"));
    assert!(format_method_hover(&function_decl("name", "String")).contains("method name"));

    let class_info = ClassInfo {
        module_name: "<test>".to_string(),
        decl: ClassDecl {
            public: true,
            copy: false,
            name: "Counter".to_string(),
            type_params: Vec::new(),
            type_param_bounds: Default::default(),
            fields: Vec::new(),
            methods: Vec::new(),
            span: Span::new(1, 1),
        },
        type_param_bounds: Default::default(),
        fields: std::collections::BTreeMap::from([(
            "value".to_string(),
            FieldInfo {
                public: true,
                ty: Type::named("int32"),
                span: Span::new(1, 1),
            },
        )]),
        methods: Default::default(),
    };
    let enum_info = EnumInfo {
        module_name: "<test>".to_string(),
        decl: crate::ast::EnumDecl {
            public: true,
            name: "Status".to_string(),
            type_params: Vec::new(),
            type_param_bounds: Default::default(),
            variants: Vec::new(),
            span: Span::new(1, 1),
        },
        type_param_bounds: Default::default(),
        variants: std::collections::BTreeMap::from([(
            "Ready".to_string(),
            EnumVariantInfo {
                payloads: Vec::new(),
                named_payloads: false,
                span: Span::new(1, 1),
            },
        )]),
    };
    assert!(format_class_hover(&class_info).contains("value: int32"));
    assert!(format_enum_hover(&enum_info).contains("enum Status"));
    assert!(builtin_enum_hover("Option[T]", "docs").contains("docs"));
    assert!(builtin_function_hover("print(value)", "docs").contains("print(value)"));
    assert!(
        format_variant_hover("Option", "Some", Some(&Type::named("String")))
            .contains("variant Some(own String) -> Option")
    );

    let option_variants = builtin_enum_variant_completions("Option");
    assert!(option_variants.iter().any(|item| item.name == "Some"));
    assert!(builtin_enum_variant_completions("Result")
        .iter()
        .any(|item| item.name == "Err"));
    assert!(builtin_enum_variant_completions("SendError")
        .iter()
        .any(|item| item.name == "Full"));
    assert!(builtin_enum_variant_completions("QueueReceive")
        .iter()
        .any(|item| item.name == "Item"));
    assert!(builtin_enum_variant_completions("TaskResult")
        .iter()
        .any(|item| item.name == "Ready"));
    assert!(builtin_enum_variant_completions("WaitAny")
        .iter()
        .any(|item| item.name == "Error"));
    assert!(builtin_enum_variant_completions("WaitAll")
        .iter()
        .any(|item| item.name == "Cancelled"));
    assert!(builtin_enum_variant_completions("Unknown").is_empty());
    assert!(builtin_member_completions(&Type::Named(
        "Set".to_string(),
        vec![Type::named("int32")],
    ))
    .iter()
    .any(|item| item.name == "insert"));
    assert!(builtin_member_completions(&Type::Named(
        "Queue".to_string(),
        vec![Type::named("int32")],
    ))
    .iter()
    .any(|item| item.name == "put"));
    assert!(builtin_member_completions(&Type::Named(
        "Queue".to_string(),
        vec![Type::named("int32")],
    ))
    .iter()
    .any(|item| item.name == "get"));
    assert!(builtin_member_completions(&Type::Named(
        "Task".to_string(),
        vec![Type::named("int32")],
    ))
    .iter()
    .any(|item| item.name == "result"));
    assert!(
        builtin_member_completions(&Type::Named("TaskGroup".to_string(), Vec::new(),))
            .iter()
            .any(|item| item.name == "start")
    );
    assert_eq!(
        builtin_function_return_type("parse_float64"),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("float64"), Type::named("String")],
        ))
    );
    assert_eq!(builtin_function_return_type("min"), None);
    assert_eq!(builtin_function_return_type("queue"), None);
    assert_eq!(builtin_function_return_type("TaskGroup"), None);
    assert_eq!(
        format_function_detail(&function_decl("render", "bool")),
        "render(self, value: int32) -> bool"
    );
}

#[test]
fn builtin_variant_inference_helpers_cover_builtin_constructors_and_unknowns() {
    let int_arg = [arg(Expr {
        kind: ExprKind::Int(7),
        span: Span::new(1, 1),
    })];
    let string_arg = [arg(Expr {
        kind: ExprKind::String("oops".to_string()),
        span: Span::new(1, 1),
    })];

    assert_eq!(
        infer_builtin_variant_call("Option", "Some", &int_arg, |_| Some(Type::named("int32"))),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("int32")]
        ))
    );
    assert_eq!(
        infer_builtin_variant_call("Option", "None", &[], |_| None),
        Some(Type::Named("Option".to_string(), vec![Type::Unit]))
    );
    assert_eq!(
        infer_builtin_variant_call("Result", "Ok", &int_arg, |_| Some(Type::named("int32"))),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::Unit],
        ))
    );
    assert_eq!(
        infer_builtin_variant_call("Result", "Err", &string_arg, |_| Some(Type::named(
            "String"
        ))),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::Unit, Type::named("String")],
        ))
    );
    assert_eq!(
        infer_builtin_variant_call("SendError", "Closed", &int_arg, |_| Some(Type::named(
            "int32"
        ))),
        Some(Type::Named(
            "SendError".to_string(),
            vec![Type::named("int32")],
        ))
    );
    assert_eq!(
        infer_builtin_variant_call("SendError", "Cancelled", &int_arg, |_| Some(Type::named(
            "int32"
        ))),
        Some(Type::Named(
            "SendError".to_string(),
            vec![Type::named("int32")],
        ))
    );
    assert_eq!(
        infer_builtin_variant_call("Option", "Missing", &[], |_| None),
        None
    );
}

#[test]
fn analysis_recovery_helpers_cover_placeholders_and_receiver_extraction() {
    let source = [
        "def main() -> int32:",
        "    counter = Counter(value=1)",
        "    counter.",
        "    return 0",
    ]
    .join("\n");
    assert_eq!(
        sanitize_member_completion_source(&source, 2, 12),
        [
            "def main() -> int32:",
            "    counter = Counter(value=1)",
            "    counter",
            "    return 0",
        ]
        .join("\n")
    );
    assert_eq!(
        replace_dangling_member_stmt_with_recovery_stmt(&source, 2),
        [
            "def main() -> int32:",
            "    counter = Counter(value=1)",
            "    return 0",
            "    return 0",
        ]
        .join("\n")
    );
    assert_eq!(
        replace_dangling_member_stmt_with_recovery_stmt("value.", 0),
        "pass"
    );
    assert_eq!(
        enclosing_function_return_placeholder(&source, 2),
        Some("return 0".to_string())
    );
    assert_eq!(
        enclosing_function_return_placeholder(
            "def main() -> bool:\n    if true:\n        value.",
            2
        ),
        Some("return false".to_string())
    );
    assert_eq!(
        placeholder_stmt_for_return_type("Option[String]"),
        Some("return Option.None".to_string())
    );
    assert_eq!(
        placeholder_stmt_for_return_type("String"),
        Some("return \"\"".to_string())
    );
    assert_eq!(placeholder_stmt_for_return_type("Counter"), None);

    let line = "    values[idx].clone().";
    assert_eq!(
        extract_receiver_before_dot(line, line.len()),
        Some("()".to_string())
    );
    assert_eq!(extract_receiver_ending_before(line, line.len()), Some("()"));
    let field_line = "    value.";
    assert_eq!(
        extract_receiver_before_dot(field_line, field_line.len()),
        Some("value".to_string())
    );
    let spaced_field_line = "    value   .   ";
    assert_eq!(
        extract_receiver_before_dot(spaced_field_line, spaced_field_line.len()),
        Some("value".to_string())
    );
    assert_eq!(extract_receiver_before_dot("      .   ", 10), None);
    assert_eq!(find_receiver_start("value.clone()", 10), Some(0));
    assert_eq!(find_receiver_start("(value.clone())", 13), Some(12));

    let stmts = vec![
        crate::ast::Stmt::Pass(PassStmt {
            span: Span::new(2, 5),
        }),
        crate::ast::Stmt::Return(ReturnStmt {
            value: Some(Expr {
                kind: ExprKind::Int(1),
                span: Span::new(4, 5),
            }),
            span: Span::new(4, 5),
        }),
    ];
    assert!(callable_contains_line(&stmts, 3));
    assert!(block_contains_line(&stmts, 4));
    assert_eq!(stmt_start_line(&stmts[0]), 2);
    assert_eq!(stmt_end_line(&stmts[1]), 4);
}

#[test]
fn analysis_builtin_completion_and_statement_helpers_cover_remaining_branches() {
    let vec_completions =
        builtin_member_completions(&Type::Named("Vec".to_string(), vec![Type::named("int32")]));
    assert!(vec_completions.iter().any(|item| item.name == "push"));
    assert!(vec_completions.iter().any(|item| item.name == "reverse"));

    let map_entry_completions = builtin_member_completions(&Type::Named(
        "MapEntry".to_string(),
        vec![Type::named("String"), Type::named("int32")],
    ));
    assert!(map_entry_completions.iter().any(|item| item.name == "key"));
    assert!(map_entry_completions
        .iter()
        .any(|item| item.name == "value"));

    let queue_completions = builtin_member_completions(&Type::Named(
        "Queue".to_string(),
        vec![Type::named("int32")],
    ));
    assert!(queue_completions.iter().any(|item| item.name == "put"));
    assert!(queue_completions.iter().any(|item| item.name == "get"));
    let task_completions =
        builtin_member_completions(&Type::Named("Task".to_string(), vec![Type::named("int32")]));
    assert!(task_completions.iter().any(|item| item.name == "result"));

    let program = checked_program("def main():\n    pass\n");
    let builder = AnalysisBuilder::new("", &program, Vec::new());
    let method_decl = function_decl("tick", "None");
    let method_info = MethodInfo {
        decl: method_decl.clone(),
        signature: FunctionSignature {
            params: vec![Type::named("int32")],
            param_passings: vec![ReceiverKind::Value],
            return_type: Type::Unit,
            return_passing: ReceiverKind::Value,
            return_borrow_source: None,
        },
        type_param_bounds: Default::default(),
    };
    let method_scope = builder.method_scope("Counter", &method_decl, &method_info);
    assert_eq!(
        method_scope
            .get("self")
            .expect("method scope should include self")
            .definition,
        range_from_span(method_decl.span, method_decl.name.len())
    );
    let vec_receiver = Type::Named("Vec".to_string(), vec![Type::named("int32")]);
    assert_eq!(
        builder
            .resolve_member_type(&vec_receiver, "clone")
            .expect("Vec.clone should resolve")
            .ty,
        Some(vec_receiver.clone())
    );
    let map_receiver = Type::Named(
        "Map".to_string(),
        vec![Type::named("String"), Type::named("int32")],
    );
    assert_eq!(
        builder
            .resolve_member_type(&map_receiver, "clone")
            .expect("Map.clone should resolve")
            .ty,
        Some(map_receiver.clone())
    );
    let set_receiver = Type::Named("Set".to_string(), vec![Type::named("String")]);
    assert_eq!(
        builder
            .resolve_member_type(&set_receiver, "clone")
            .expect("Set.clone should resolve")
            .ty,
        Some(set_receiver.clone())
    );

    assert_eq!(
        builtin_function_return_type("range"),
        Some(Type::named("Range"))
    );
    assert_eq!(builtin_function_return_type("print"), Some(Type::Unit));
    assert_eq!(builtin_function_return_type("TaskGroup"), None);
    assert_eq!(
        builtin_function_return_type("cancelled"),
        Some(Type::named("bool"))
    );
    assert_eq!(builtin_function_return_type("after"), None);
    assert_eq!(builtin_function_return_type("wait_any"), None);
    assert_eq!(builtin_function_return_type("wait_all"), None);
    assert_eq!(builtin_function_return_type("abs"), None);
    assert_eq!(builtin_function_return_type("min"), None);
    assert_eq!(builtin_function_return_type("max"), None);
    assert_eq!(builtin_function_return_type("sqrt"), None);
    assert_eq!(builtin_function_return_type("sleep"), Some(Type::Unit));
    assert_eq!(
        builtin_function_return_type("parse_int32"),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ))
    );
    assert_eq!(
        builtin_function_return_type("parse_int64"),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("int64"), Type::named("String")],
        ))
    );
    assert_eq!(
        builtin_function_return_type("parse_float64"),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("float64"), Type::named("String")],
        ))
    );

    let if_stmt = crate::ast::Stmt::If(crate::ast::IfStmt {
        branches: vec![crate::ast::IfBranch {
            condition: Expr {
                kind: ExprKind::Bool(true),
                span: Span::new(2, 8),
            },
            body: vec![crate::ast::Stmt::Pass(PassStmt {
                span: Span::new(3, 9),
            })],
            span: Span::new(2, 5),
        }],
        else_body: Some(vec![crate::ast::Stmt::Return(ReturnStmt {
            value: None,
            span: Span::new(5, 5),
        })]),
        span: Span::new(2, 5),
    });
    let match_stmt = crate::ast::Stmt::Match(crate::ast::MatchStmt {
        scrutinee: Expr {
            kind: ExprKind::Name("status".to_string()),
            span: Span::new(6, 11),
        },
        borrow_mode: None,
        arms: vec![crate::ast::MatchArm {
            pattern: crate::ast::Pattern::Wildcard(Span::new(7, 9)),
            body: vec![crate::ast::Stmt::Pass(PassStmt {
                span: Span::new(8, 9),
            })],
            span: Span::new(7, 9),
        }],
        span: Span::new(6, 5),
    });
    let for_stmt = crate::ast::Stmt::For(crate::ast::ForStmt {
        binding: "value".to_string(),
        iterable: Expr {
            kind: ExprKind::Name("values".to_string()),
            span: Span::new(9, 14),
        },
        borrow_mode: None,
        body: vec![crate::ast::Stmt::Pass(PassStmt {
            span: Span::new(10, 9),
        })],
        span: Span::new(9, 5),
    });
    let with_stmt = crate::ast::Stmt::With(crate::ast::WithStmt {
        binding: "resource".to_string(),
        value: Expr {
            kind: ExprKind::Name("resource".to_string()),
            span: Span::new(11, 10),
        },
        body: vec![crate::ast::Stmt::Pass(PassStmt {
            span: Span::new(12, 9),
        })],
        span: Span::new(11, 5),
    });
    let helper_with_stmt = crate::ast::Stmt::With(crate::ast::WithStmt {
        binding: "group".to_string(),
        value: Expr {
            kind: ExprKind::Name("group".to_string()),
            span: Span::new(13, 10),
        },
        body: vec![crate::ast::Stmt::Pass(PassStmt {
            span: Span::new(14, 9),
        })],
        span: Span::new(13, 5),
    });
    let while_stmt = crate::ast::Stmt::While(crate::ast::WhileStmt {
        condition: Expr {
            kind: ExprKind::Bool(true),
            span: Span::new(15, 11),
        },
        body: vec![crate::ast::Stmt::Pass(PassStmt {
            span: Span::new(16, 9),
        })],
        span: Span::new(15, 5),
    });
    let stmts = vec![
        if_stmt,
        match_stmt,
        for_stmt,
        with_stmt,
        helper_with_stmt,
        while_stmt,
    ];
    assert_eq!(stmt_end_line(&stmts[0]), 5);
    let empty_else_stmt = crate::ast::Stmt::If(crate::ast::IfStmt {
        branches: vec![crate::ast::IfBranch {
            condition: Expr {
                kind: ExprKind::Bool(true),
                span: Span::new(20, 8),
            },
            body: vec![crate::ast::Stmt::Pass(PassStmt {
                span: Span::new(21, 9),
            })],
            span: Span::new(20, 5),
        }],
        else_body: Some(Vec::new()),
        span: Span::new(20, 5),
    });
    assert_eq!(stmt_end_line(&empty_else_stmt), 21);
    let no_else_stmt = crate::ast::Stmt::If(crate::ast::IfStmt {
        branches: vec![crate::ast::IfBranch {
            condition: Expr {
                kind: ExprKind::Bool(false),
                span: Span::new(22, 8),
            },
            body: vec![crate::ast::Stmt::Pass(PassStmt {
                span: Span::new(23, 9),
            })],
            span: Span::new(22, 5),
        }],
        else_body: None,
        span: Span::new(22, 5),
    });
    assert_eq!(stmt_end_line(&no_else_stmt), 23);
    assert_eq!(stmt_end_line(&stmts[1]), 8);
    assert_eq!(stmt_end_line(&stmts[2]), 10);
    assert_eq!(stmt_end_line(&stmts[3]), 12);
    assert_eq!(stmt_end_line(&stmts[4]), 14);
    assert_eq!(stmt_end_line(&stmts[5]), 16);
    assert!(!block_contains_line(&[], 1));
    let break_stmt = crate::ast::Stmt::Break(crate::ast::BreakStmt {
        span: Span::new(17, 9),
    });
    let continue_stmt = crate::ast::Stmt::Continue(crate::ast::ContinueStmt {
        span: Span::new(18, 9),
    });
    let expr_stmt = crate::ast::Stmt::Expr(crate::ast::ExprStmt {
        expr: expr(ExprKind::Int(1)),
        span: Span::new(19, 9),
    });
    assert_eq!(stmt_start_line(&break_stmt), 17);
    assert_eq!(stmt_start_line(&continue_stmt), 18);
    assert_eq!(stmt_start_line(&expr_stmt), 19);
    assert_eq!(stmt_end_line(&break_stmt), 17);
    assert_eq!(stmt_end_line(&continue_stmt), 18);
    assert_eq!(stmt_end_line(&expr_stmt), 19);
    assert!(callable_contains_line(&stmts, 14));
    assert!(!block_contains_line(&stmts, 20));

    let scope_builder = AnalysisBuilder::new("", &program, Vec::new());
    let mut accumulated_scope = BTreeMap::new();
    scope_builder.accumulate_scope_from_stmts(&stmts[..1], 4, &mut accumulated_scope);
    scope_builder.accumulate_scope_from_stmts(
        std::slice::from_ref(&no_else_stmt),
        24,
        &mut accumulated_scope,
    );
    scope_builder.accumulate_scope_from_stmts(&stmts, 5, &mut accumulated_scope);
    scope_builder.accumulate_scope_from_stmts(&stmts, 6, &mut accumulated_scope);
    scope_builder.accumulate_scope_from_stmts(&stmts, 16, &mut accumulated_scope);

    let mut fallback_builder = AnalysisBuilder::new("", &program, Vec::new());
    let mut fallback_scope = BTreeMap::new();
    let fallback_assignment = AssignStmt {
        mutable: false,
        target: AssignTarget::Name("fresh".to_string()),
        annotation: Some(type_ref("int32")),
        op: None,
        value: expr(ExprKind::Int(1)),
        span: Span::new(30, 1),
    };
    fallback_builder.bind_assignment(&fallback_assignment, &mut fallback_scope);
    assert_eq!(
        fallback_scope
            .get("fresh")
            .expect("fresh binding should be inserted")
            .definition,
        range_from_span(Span::new(30, 1), "fresh".len())
    );
    let reassignment = AssignStmt {
        mutable: false,
        target: AssignTarget::Name("fresh".to_string()),
        annotation: None,
        op: None,
        value: expr(ExprKind::Int(2)),
        span: Span::new(31, 1),
    };
    fallback_builder.visit_assign(&reassignment, &mut fallback_scope);
    let reassignment_range = range_from_span(Span::new(31, 1), "fresh".len());
    assert!(fallback_builder
        .output
        .occurrences
        .iter()
        .any(|occurrence| {
            occurrence.hover.contains("fresh: int32")
                && occurrence.line == reassignment_range.line
                && occurrence.start_character == reassignment_range.start_character
                && occurrence.end_character == reassignment_range.end_character
        }));

    let mut scope = BTreeMap::new();
    let no_payload_arm = crate::ast::MatchArm {
        pattern: crate::ast::Pattern::Variant(VariantPattern {
            enum_name: Some("Option".to_string()),
            variant_name: "None".to_string(),
            subpatterns: Vec::new(),
            span: Span::new(21, 9),
        }),
        body: Vec::new(),
        span: Span::new(21, 9),
    };
    scope_builder.bind_match_arm_scope(
        &no_payload_arm,
        Some(&Type::Named(
            "Option".to_string(),
            vec![Type::named("int32")],
        )),
        &mut scope,
    );
    assert!(scope.is_empty());
    let non_binding_payload_arm = crate::ast::MatchArm {
        pattern: crate::ast::Pattern::Variant(VariantPattern {
            enum_name: Some("Option".to_string()),
            variant_name: "Some".to_string(),
            subpatterns: vec![crate::ast::Pattern::Wildcard(Span::new(22, 19))],
            span: Span::new(22, 9),
        }),
        body: Vec::new(),
        span: Span::new(22, 9),
    };
    scope_builder.bind_match_arm_scope(
        &non_binding_payload_arm,
        Some(&Type::Named(
            "Option".to_string(),
            vec![Type::named("int32")],
        )),
        &mut scope,
    );
    assert!(scope.is_empty());

    assert_eq!(extract_receiver_ending_before("", 0), None);
    assert_eq!(extract_receiver_ending_before("value", 5), None);
    assert_eq!(extract_receiver_ending_before(".field", 1), None);
    assert_eq!(
        extract_receiver_ending_before("(value + other).field", 16),
        Some("(value + other)")
    );
    assert_eq!(
        extract_receiver_ending_before("value.   ", "value.   ".len()),
        Some("value")
    );
    assert_eq!(
        extract_receiver_ending_before("((value)).field", 10),
        Some("((value))")
    );
    assert_eq!(find_receiver_start("value).field", 5), None);
    assert_eq!(
        sanitize_member_completion_source("def main():\n    value", 20, 0),
        "def main():\n    value"
    );
    assert_eq!(
        sanitize_member_completion_source("def main():\n    value", 1, 0),
        "def main():\n    value"
    );
    assert_eq!(
        sanitize_member_completion_source("def main():\n    value", 1, 10),
        "def main():\n    value"
    );
    assert_eq!(
        replace_dangling_member_stmt_with_recovery_stmt("def main():\n    value.", 20),
        "def main():\n    value."
    );
    assert_eq!(enclosing_function_return_placeholder("value.", 0), None);
    assert_eq!(
        enclosing_function_return_placeholder("def main() -> int32:\n    value.", 10),
        None
    );
    assert_eq!(placeholder_stmt_for_return_type("Custom"), None);
}

#[test]
fn analysis_builtin_member_types_cover_io_network_and_process_surfaces() {
    let program = checked_program("def main():\n    pass\n");
    let builder = AnalysisBuilder::new("", &program, Vec::new());
    let named = |name: &str| Type::Named(name.to_string(), Vec::new());
    let option = |payload: Type| Type::Named("Option".to_string(), vec![payload]);
    let result = |ok: Type, err: Type| Type::Named("Result".to_string(), vec![ok, err]);
    let vec_of = |payload: Type| Type::Named("Vec".to_string(), vec![payload]);
    let string = Type::named("String");
    let uint8 = Type::named("uint8");
    let io_error = named("io.Error");
    let process_error = named("process.Error");

    let assert_member_type = |receiver: &str, field: &str, expected: Type| {
        let member = builder
            .resolve_member_type(&named(receiver), field)
            .unwrap_or_else(|| panic!("expected builtin member {receiver}.{field}"));
        assert!(
            member.hover.contains(field),
            "hover for {receiver}.{field} should mention the member name"
        );
        assert_eq!(member.ty, Some(expected), "{receiver}.{field}");
    };

    assert_member_type("String", "len", Type::named("int32"));
    assert_member_type("String", "byte_len", Type::named("int32"));
    assert_member_type("process.Child", "stdin", option(named("process.Pipe")));
    assert_member_type("process.Child", "stdout", option(named("process.Pipe")));
    assert_member_type("process.Child", "stderr", option(named("process.Pipe")));
    assert_member_type("process.Child", "wait", named("process.Wait"));
    assert_member_type(
        "process.Child",
        "wait_or_none",
        result(option(named("process.ExitStatus")), process_error.clone()),
    );
    assert_member_type(
        "process.Child",
        "wait_ok",
        result(named("process.ExitStatus"), process_error.clone()),
    );
    assert_member_type(
        "process.Child",
        "kill",
        result(Type::Unit, process_error.clone()),
    );
    assert_member_type(
        "process.Child",
        "terminate",
        result(Type::Unit, process_error.clone()),
    );
    assert_member_type("process.Child", "close", Type::Unit);

    assert_member_type(
        "process.Pipe",
        "read_all",
        result(string.clone(), process_error.clone()),
    );
    assert_member_type(
        "process.Pipe",
        "read_line",
        result(option(string.clone()), process_error.clone()),
    );
    assert_member_type(
        "process.Pipe",
        "read_bytes",
        result(option(vec_of(uint8.clone())), process_error.clone()),
    );
    for field in ["write_all", "write_bytes", "flush"] {
        assert_member_type(
            "process.Pipe",
            field,
            result(Type::Unit, process_error.clone()),
        );
    }
    assert_member_type("process.Pipe", "close", Type::Unit);

    assert_member_type("process.Completed", "status", named("process.ExitStatus"));
    assert_member_type("process.Completed", "success", Type::named("bool"));
    assert_member_type("process.Completed", "stdout", string.clone());
    assert_member_type("process.Completed", "stderr", string.clone());
    assert_member_type("process.Completed", "stdout_bytes", vec_of(uint8.clone()));
    assert_member_type("process.Completed", "stderr_bytes", vec_of(uint8.clone()));
    assert_member_type(
        "process.Completed",
        "check",
        result(Type::Unit, process_error.clone()),
    );

    for field in ["start", "stop"] {
        assert_member_type(
            "process.Supervisor",
            field,
            result(Type::Unit, process_error.clone()),
        );
    }
    assert_member_type(
        "process.Supervisor",
        "wait",
        named("process.SupervisorWait"),
    );
    assert_member_type(
        "process.Supervisor",
        "wait_or_none",
        result(
            option(named("process.SupervisorEvent")),
            process_error.clone(),
        ),
    );
    assert_member_type("process.Supervisor", "is_empty", Type::named("bool"));
    assert_member_type("process.Supervisor", "close", Type::Unit);

    assert_member_type(
        "fs.File",
        "read_all",
        result(string.clone(), io_error.clone()),
    );
    assert_member_type(
        "fs.File",
        "read_bytes",
        result(vec_of(uint8.clone()), io_error.clone()),
    );
    for field in ["write_all", "write_bytes", "flush"] {
        assert_member_type("fs.File", field, result(Type::Unit, io_error.clone()));
    }
    assert_member_type("fs.File", "close", Type::Unit);

    assert_member_type(
        "net.TcpListener",
        "accept",
        result(named("net.TcpStream"), io_error.clone()),
    );
    assert_member_type(
        "net.TcpListener",
        "local_addr",
        result(string.clone(), io_error.clone()),
    );
    assert_member_type("net.TcpListener", "close", Type::Unit);
    for field in ["read_all", "local_addr", "peer_addr"] {
        assert_member_type(
            "net.TcpStream",
            field,
            result(string.clone(), io_error.clone()),
        );
    }
    assert_member_type(
        "net.TcpStream",
        "read_line",
        result(option(string.clone()), io_error.clone()),
    );
    assert_member_type(
        "net.TcpStream",
        "read_bytes",
        result(option(vec_of(uint8.clone())), io_error.clone()),
    );
    assert_member_type(
        "net.TcpStream",
        "read_exact",
        result(vec_of(uint8.clone()), io_error.clone()),
    );
    for field in [
        "write_all",
        "write_bytes",
        "flush",
        "shutdown_read",
        "shutdown_write",
        "shutdown_both",
    ] {
        assert_member_type("net.TcpStream", field, result(Type::Unit, io_error.clone()));
    }
    assert_member_type("net.TcpStream", "close", Type::Unit);

    for field in ["send_text", "send_bytes"] {
        assert_member_type("net.UdpSocket", field, result(Type::Unit, io_error.clone()));
    }
    assert_member_type(
        "net.UdpSocket",
        "recv",
        result(option(vec_of(uint8.clone())), io_error.clone()),
    );
    assert_member_type(
        "net.UdpSocket",
        "recv_from",
        result(option(named("net.UdpDatagram")), io_error.clone()),
    );
    for field in ["local_addr", "peer_addr"] {
        assert_member_type(
            "net.UdpSocket",
            field,
            result(string.clone(), io_error.clone()),
        );
    }
    assert_member_type("net.UdpSocket", "close", Type::Unit);
    assert_member_type("net.UdpDatagram", "address", string.clone());
    assert_member_type("net.UdpDatagram", "bytes", vec_of(uint8.clone()));
    assert_member_type(
        "net.UdpDatagram",
        "text",
        result(string.clone(), io_error.clone()),
    );

    assert_member_type(
        "net.HttpListener",
        "accept",
        result(named("net.HttpExchange"), io_error.clone()),
    );
    assert_member_type(
        "net.HttpListener",
        "local_addr",
        result(string.clone(), io_error.clone()),
    );
    assert_member_type("net.HttpListener", "close", Type::Unit);
    assert_member_type("net.HttpExchange", "method", string.clone());
    assert_member_type("net.HttpExchange", "path", string.clone());
    assert_member_type(
        "net.HttpExchange",
        "headers",
        Type::Named("Map".to_string(), vec![string.clone(), string.clone()]),
    );
    assert_member_type(
        "net.HttpExchange",
        "body_text",
        result(string.clone(), io_error.clone()),
    );
    assert_member_type("net.HttpExchange", "body_bytes", vec_of(uint8.clone()));
    for field in ["respond_text", "respond_bytes"] {
        assert_member_type(
            "net.HttpExchange",
            field,
            result(Type::Unit, io_error.clone()),
        );
    }
    assert_member_type("net.HttpResponse", "status", Type::named("int32"));
    assert_member_type("net.HttpResponse", "reason", string.clone());
    assert_member_type(
        "net.HttpResponse",
        "headers",
        Type::Named("Map".to_string(), vec![string.clone(), string.clone()]),
    );
    assert_member_type(
        "net.HttpResponse",
        "text",
        result(string.clone(), io_error.clone()),
    );
    assert_member_type("net.HttpResponse", "bytes", vec_of(uint8.clone()));

    assert_member_type(
        "net.WebSocketListener",
        "accept",
        result(named("net.WebSocket"), io_error.clone()),
    );
    assert_member_type(
        "net.WebSocketListener",
        "local_addr",
        result(string.clone(), io_error.clone()),
    );
    for field in ["send_text", "send_bytes"] {
        assert_member_type("net.WebSocket", field, result(Type::Unit, io_error.clone()));
    }
    assert_member_type(
        "net.WebSocket",
        "recv_text",
        result(option(string.clone()), io_error.clone()),
    );
    assert_member_type(
        "net.WebSocket",
        "recv_bytes",
        result(option(vec_of(uint8.clone())), io_error.clone()),
    );
    assert_member_type("net.WebSocket", "close", Type::Unit);

    assert_member_type(
        "net.UnixListener",
        "accept",
        result(named("net.UnixStream"), io_error.clone()),
    );
    assert_member_type("net.UnixListener", "close", Type::Unit);
    assert_member_type(
        "net.UnixStream",
        "read_line",
        result(option(string.clone()), io_error.clone()),
    );
    assert_member_type(
        "net.UnixStream",
        "read_exact",
        result(vec_of(uint8.clone()), io_error.clone()),
    );
    assert_member_type(
        "net.UnixStream",
        "write_all",
        result(Type::Unit, io_error.clone()),
    );
    assert_member_type("net.UnixStream", "close", Type::Unit);

    assert_member_type(
        "net.TlsListener",
        "accept",
        result(named("net.TlsStream"), io_error.clone()),
    );
    assert_member_type(
        "net.TlsListener",
        "local_addr",
        result(string.clone(), io_error.clone()),
    );
    assert_member_type("net.TlsListener", "close", Type::Unit);
    assert_member_type(
        "net.TlsStream",
        "read_line",
        result(option(string.clone()), io_error.clone()),
    );
    assert_member_type(
        "net.TlsStream",
        "read_exact",
        result(vec_of(uint8.clone()), io_error.clone()),
    );
    assert_member_type(
        "net.TlsStream",
        "write_all",
        result(Type::Unit, io_error.clone()),
    );
    assert_member_type("net.TlsStream", "close", Type::Unit);
}

#[test]
fn path_aware_analysis_handles_large_repo_scratch_corpus_without_panicking() {
    run_with_large_stack(|| {
        let repo_root = repo_root();
        let corpus_dirs = [repo_root.join("test_edge"), repo_root.join("test_recheck")];
        let mut file_count = 0usize;
        let mut symbol_total = 0usize;

        for dir in corpus_dirs {
            for path in collect_aurora_files(&dir) {
                file_count += 1;
                let source = fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("failed to read {}: {}", path.display(), error));
                let output = analyze_path_source(&path, &source);
                symbol_total += output.symbols.len();
            }
        }

        assert!(file_count >= 800, "expected large scratch corpus");
        assert!(
            symbol_total > 0,
            "expected scratch corpus analysis to produce some symbols"
        );
    });
}
