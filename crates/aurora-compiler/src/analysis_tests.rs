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
    recover_checked_program_after_parse_error_with, recover_checked_program_after_position,
    replace_dangling_member_stmt_with_recovery_stmt, sanitize_member_completion_source,
    stmt_end_line, stmt_start_line, AnalysisBuilder,
};
use crate::ast::{
    Argument, ClassDecl, Expr, ExprKind, FunctionDecl, Item, PassStmt, ReceiverKind, ReturnStmt,
    TypeRef, VariantPattern,
};
use crate::diag::{Diagnostic, Span};
use crate::sema::{ClassInfo, EnumInfo, EnumVariantInfo, FieldInfo, TraitBound, Type};
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
            passing: ReceiverKind::Value,
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

    assert_eq!(insert.detail, "insert(index: int32, value: T) -> bool");
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
        Some(Type::named("int32"))
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
fn analysis_completion_and_inference_helpers_cover_builtin_collection_and_enum_surfaces() {
    let source = [
        "trait Show:",
        "    def show(borrow self) -> String",
        "",
        "class User:",
        "    label: String",
        "",
        "    def greet(borrow self) -> String:",
        "        return self.label",
        "",
        "impl Show for User:",
        "    def show(borrow self) -> String:",
        "        return self.label",
        "",
        "enum Status:",
        "    Ready",
        "    Failed(String)",
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

    let task_group_member_names = builder
        .member_completions(&Type::named("TaskGroup"))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(task_group_member_names.contains(&"start".to_string()));
    assert!(task_group_member_names.contains(&"start_soon".to_string()));

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
        Some(Type::Named("Vec".to_string(), vec![Type::named("int32")]))
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
            vec![Type::named("String"), Type::named("int32")],
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
            vec![Type::named("int32")]
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
        "def inspect(status: Status, value: Option[int32]) -> int32:",
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
    let import_range = builder
        .find_imported_module_range("pkg.types")
        .expect("import range should fall back to current file");
    assert_eq!(import_range.file_path.as_deref(), Some("/tmp/main.au"));
    assert_eq!(import_range.line, 0);

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

    let enum_names = builder
        .member_completions(&Type::named("Status"))
        .into_iter()
        .map(|completion| completion.name)
        .collect::<Vec<_>>();
    assert!(enum_names.contains(&"Ready".to_string()));
    assert!(enum_names.contains(&"Failed".to_string()));
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
            "        return self.label",
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
        occurrence.hover.contains("method name() -> String")
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
        occurrence.line == 8
            && occurrence.hover.contains("variant Busy")
            && occurrence.definition.is_some()
    }));
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
            .contains("variant Some(String) -> Option")
    );

    let option_variants = builtin_enum_variant_completions("Option");
    assert!(option_variants.iter().any(|item| item.name == "Some"));
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
        "render(int32) -> bool"
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
        enclosing_function_return_placeholder(&source, 2),
        Some("return 0".to_string())
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

    assert_eq!(
        builtin_function_return_type("range"),
        Some(Type::named("Range"))
    );
    assert_eq!(builtin_function_return_type("TaskGroup"), None);
    assert_eq!(
        builtin_function_return_type("cancelled"),
        Some(Type::named("bool"))
    );
    assert_eq!(builtin_function_return_type("after"), None);
    assert_eq!(builtin_function_return_type("wait_any"), None);
    assert_eq!(builtin_function_return_type("wait_all"), None);
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
    assert_eq!(stmt_end_line(&stmts[1]), 8);
    assert_eq!(stmt_end_line(&stmts[2]), 10);
    assert_eq!(stmt_end_line(&stmts[3]), 12);
    assert_eq!(stmt_end_line(&stmts[4]), 14);
    assert_eq!(stmt_end_line(&stmts[5]), 16);
    assert!(callable_contains_line(&stmts, 14));
    assert!(!block_contains_line(&stmts, 20));
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
