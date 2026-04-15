use super::{
    absolutize, analyze_path_source, check_path, check_path_with_source, check_source,
    emit_host_native_object, exported_binding, exported_namespace, find_type_namespace_path,
    import_exists_from_root, infer_package_root, insert_namespace_import, is_builtin_export_type,
    local_item_exists, logical_module_name, lower_path_to_mir, lower_path_with_source_to_mir,
    lower_source_to_mir, parse_source, qualify_export_type, qualify_export_type_ref, run_mir,
    run_path, run_path_via_mir, run_path_with_source, run_path_with_source_via_mir,
    run_serialized_mir, run_source, run_source_via_mir, Value,
};
use crate::ast::TypeRef;
use crate::diag::Span;
use std::collections::BTreeMap;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const POINT_SOURCE: &str = include_str!("../../../examples/point.au");
const BASIC_ADDITION_SOURCE: &str = include_str!("../../../examples/basic_addition.au");
const TOP_LEVEL_ADDITION_SOURCE: &str = include_str!("../../../examples/top_level_addition.au");
const CONTROL_FLOW_SOURCE: &str = include_str!("../../../examples/control_flow.au");
const EXAMPLE_CASES: &[(&str, &str)] = &[
    (
        "examples/basics/top_level_script.au",
        include_str!("../../../examples/basics/top_level_script.au"),
    ),
    (
        "examples/basics/main_function.au",
        include_str!("../../../examples/basics/main_function.au"),
    ),
    (
        "examples/basics/mutable_bindings.au",
        include_str!("../../../examples/basics/mutable_bindings.au"),
    ),
    (
        "examples/basics/default_arguments.au",
        include_str!("../../../examples/basics/default_arguments.au"),
    ),
    (
        "examples/basics/pass_keyword.au",
        include_str!("../../../examples/basics/pass_keyword.au"),
    ),
    (
        "examples/classes/point_distance.au",
        include_str!("../../../examples/classes/point_distance.au"),
    ),
    (
        "examples/classes/default_fields.au",
        include_str!("../../../examples/classes/default_fields.au"),
    ),
    (
        "examples/classes/methods.au",
        include_str!("../../../examples/classes/methods.au"),
    ),
    (
        "examples/control_flow/if_elif_else.au",
        include_str!("../../../examples/control_flow/if_elif_else.au"),
    ),
    (
        "examples/control_flow/for_range.au",
        include_str!("../../../examples/control_flow/for_range.au"),
    ),
    (
        "examples/control_flow/while_break_continue.au",
        include_str!("../../../examples/control_flow/while_break_continue.au"),
    ),
    (
        "examples/enums/result_match.au",
        include_str!("../../../examples/enums/result_match.au"),
    ),
    (
        "examples/enums/result_option.au",
        include_str!("../../../examples/enums/result_option.au"),
    ),
    (
        "examples/enums/explicit_type_args.au",
        include_str!("../../../examples/enums/explicit_type_args.au"),
    ),
    (
        "examples/generics/box_and_wrapper.au",
        include_str!("../../../examples/generics/box_and_wrapper.au"),
    ),
    (
        "examples/traits/greeter.au",
        include_str!("../../../examples/traits/greeter.au"),
    ),
    (
        "examples/traits/multiple_bounds.au",
        include_str!("../../../examples/traits/multiple_bounds.au"),
    ),
    (
        "examples/numbers/float_sqrt.au",
        include_str!("../../../examples/numbers/float_sqrt.au"),
    ),
    (
        "examples/numbers/float32_values.au",
        include_str!("../../../examples/numbers/float32_values.au"),
    ),
    (
        "examples/numbers/numeric_casts.au",
        include_str!("../../../examples/numbers/numeric_casts.au"),
    ),
    (
        "examples/strings/greeting.au",
        include_str!("../../../examples/strings/greeting.au"),
    ),
    (
        "examples/concurrency/task_group_select.au",
        include_str!("../../../examples/concurrency/task_group_select.au"),
    ),
    (
        "examples/concurrency/task_group_cancel.au",
        include_str!("../../../examples/concurrency/task_group_cancel.au"),
    ),
    (
        "examples/concurrency/select_timeout.au",
        include_str!("../../../examples/concurrency/select_timeout.au"),
    ),
    (
        "examples/concurrency/sleep_builtin.au",
        include_str!("../../../examples/concurrency/sleep_builtin.au"),
    ),
    (
        "examples/concurrency/send_result.au",
        include_str!("../../../examples/concurrency/send_result.au"),
    ),
    (
        "examples/concurrency/spawn_detached.au",
        include_str!("../../../examples/concurrency/spawn_detached.au"),
    ),
    (
        "examples/concurrency/select_send.au",
        include_str!("../../../examples/concurrency/select_send.au"),
    ),
    (
        "examples/enums/wildcard_match.au",
        include_str!("../../../examples/enums/wildcard_match.au"),
    ),
    (
        "examples/generics/generic_method_calls.au",
        include_str!("../../../examples/generics/generic_method_calls.au"),
    ),
    (
        "examples/generics/bounded_types.au",
        include_str!("../../../examples/generics/bounded_types.au"),
    ),
    (
        "examples/traits/marker_trait.au",
        include_str!("../../../examples/traits/marker_trait.au"),
    ),
    (
        "examples/traits/specialized_generic_impl.au",
        include_str!("../../../examples/traits/specialized_generic_impl.au"),
    ),
    (
        "examples/concurrency/minute_duration.au",
        include_str!("../../../examples/concurrency/minute_duration.au"),
    ),
    (
        "examples/traits/generic_dispatch_multiple_types.au",
        include_str!("../../../examples/traits/generic_dispatch_multiple_types.au"),
    ),
    (
        "examples/strings/string_methods.au",
        include_str!("../../../examples/strings/string_methods.au"),
    ),
    (
        "examples/numbers/numeric_builtins.au",
        include_str!("../../../examples/numbers/numeric_builtins.au"),
    ),
    (
        "examples/collections/map_basics.au",
        include_str!("../../../examples/collections/map_basics.au"),
    ),
    (
        "examples/collections/set_basics.au",
        include_str!("../../../examples/collections/set_basics.au"),
    ),
    (
        "examples/strings/string_parsing_and_formatting.au",
        include_str!("../../../examples/strings/string_parsing_and_formatting.au"),
    ),
    (
        "examples/traits/generic_trait_bounds.au",
        include_str!("../../../examples/traits/generic_trait_bounds.au"),
    ),
    (
        "examples/traits/operator_traits.au",
        include_str!("../../../examples/traits/operator_traits.au"),
    ),
];
const ADDITIONAL_EXAMPLE_CASES: &[(&str, &str, &str)] = &[
    (
        "examples/basic_addition.au",
        include_str!("../../../examples/basic_addition.au"),
        "16\n",
    ),
    (
        "examples/basics/borrow_parameters.au",
        include_str!("../../../examples/basics/borrow_parameters.au"),
        "41\n42\n42\n",
    ),
    (
        "examples/basics/named_arguments.au",
        include_str!("../../../examples/basics/named_arguments.au"),
        "hello, aurora\n7\n",
    ),
    (
        "examples/basics/named_builtin_arguments.au",
        include_str!("../../../examples/basics/named_builtin_arguments.au"),
        "10\n",
    ),
    (
        "examples/basics/none_values.au",
        include_str!("../../../examples/basics/none_values.au"),
        "1\n",
    ),
    (
        "examples/basics/simple_example.au",
        include_str!("../../../examples/basics/simple_example.au"),
        "Ayoola Olafenwa\n834.6\n",
    ),
    (
        "examples/classes/copy_class.au",
        include_str!("../../../examples/classes/copy_class.au"),
        "1\n2\n",
    ),
    (
        "examples/classes/indirect_recursive.au",
        include_str!("../../../examples/classes/indirect_recursive.au"),
        "2\n",
    ),
    (
        "examples/classes/mutating_methods.au",
        include_str!("../../../examples/classes/mutating_methods.au"),
        "6\n1\n",
    ),
    (
        "examples/collections/vec_basics.au",
        include_str!("../../../examples/collections/vec_basics.au"),
        "3\n1\n2\n2\n20\n1\n99\nfalse\n",
    ),
    (
        "examples/collections/vec_iteration.au",
        include_str!("../../../examples/collections/vec_iteration.au"),
        "Ada\nGrace\n2\n9\n",
    ),
    (
        "examples/collections/vec_polish.au",
        include_str!("../../../examples/collections/vec_polish.au"),
        "Ada\nGrace\ntrue\nfalse\n4\n1\n14\n13\n12\n11\ntrue\n100\ntrue\ntrue\n",
    ),
    (
        "examples/concurrency/channel_iteration.au",
        include_str!("../../../examples/concurrency/channel_iteration.au"),
        "1\n2\n",
    ),
    (
        "examples/concurrency/channels_spawn.au",
        include_str!("../../../examples/concurrency/channels_spawn.au"),
        "2\n4\n",
    ),
    (
        "examples/concurrency/select_timeout_named.au",
        include_str!("../../../examples/concurrency/select_timeout_named.au"),
        "timeout\n",
    ),
    (
        "examples/control_flow.au",
        include_str!("../../../examples/control_flow.au"),
        "ok\n",
    ),
    (
        "examples/control_flow/boolean_logic.au",
        include_str!("../../../examples/control_flow/boolean_logic.au"),
        "ready\ntrue\n",
    ),
    (
        "examples/control_flow/match_literals.au",
        include_str!("../../../examples/control_flow/match_literals.au"),
        "negative\nzero\nmany\nyes\nno\nrepo\nother\n",
    ),
    (
        "examples/enums/match_borrow.au",
        include_str!("../../../examples/enums/match_borrow.au"),
        "ok\n",
    ),
    (
        "examples/error_handling/try_result.au",
        include_str!("../../../examples/error_handling/try_result.au"),
        "6\ndivision by zero\n",
    ),
    (
        "examples/generics/generic_constructor_specialization.au",
        include_str!("../../../examples/generics/generic_constructor_specialization.au"),
        "42\n",
    ),
    (
        "examples/numbers/uint128_values.au",
        include_str!("../../../examples/numbers/uint128_values.au"),
        "340282366920938463463374607431768211455\n340282366920938463463374607431768211455\n",
    ),
    (
        "examples/numbers/unary_minus.au",
        include_str!("../../../examples/numbers/unary_minus.au"),
        "-5\n-3.5\n2\n",
    ),
    (
        "examples/point.au",
        include_str!("../../../examples/point.au"),
        "5.0\n",
    ),
    (
        "examples/simple_addition.au",
        include_str!("../../../examples/simple_addition.au"),
        "156\n",
    ),
    (
        "examples/strings/borrow_str.au",
        include_str!("../../../examples/strings/borrow_str.au"),
        "Hello, Aurora\n",
    ),
    (
        "examples/strings/f_strings.au",
        include_str!("../../../examples/strings/f_strings.au"),
        "Hello, Aurora 42\n",
    ),
    (
        "examples/strings/string_clone.au",
        include_str!("../../../examples/strings/string_clone.au"),
        "aurora\n",
    ),
    (
        "examples/top_level_addition.au",
        include_str!("../../../examples/top_level_addition.au"),
        "16\n",
    ),
    (
        "examples/traits/generic_trait_impl.au",
        include_str!("../../../examples/traits/generic_trait_impl.au"),
        "11\n",
    ),
    (
        "examples/traits/specialized_trait_dispatch.au",
        include_str!("../../../examples/traits/specialized_trait_dispatch.au"),
        "7\nhi\n",
    ),
    (
        "examples/traits/trait_associated_factory.au",
        include_str!("../../../examples/traits/trait_associated_factory.au"),
        "7\n",
    ),
];

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

fn zero_exit_value() -> Value {
    Value::Int(crate::integer::IntegerValue::zero())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("compiler crate should live under repo root")
        .to_path_buf()
}

fn type_ref(name: &str) -> TypeRef {
    TypeRef {
        name: name.to_string(),
        args: vec![],
        indirect: false,
        span: crate::diag::Span::new(1, 1),
    }
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

fn collect_aurora_files_recursive(dir: &std::path::Path) -> Vec<PathBuf> {
    fn visit(dir: &std::path::Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("failed to read {}: {}", dir.display(), error))
        {
            let path = entry
                .unwrap_or_else(|error| {
                    panic!("failed to read entry under {}: {}", dir.display(), error)
                })
                .path();
            if path.is_dir() {
                visit(&path, files);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) == Some("au") {
                files.push(path);
            }
        }
    }

    let mut files = Vec::new();
    visit(dir, &mut files);
    files.sort();
    files
}

fn should_execute_runtime_corpus_case(path: &std::path::Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    ![
        "infinite_loop",
        "large_loop",
        "recursive_deep",
        "deep_recursion",
        "sleep",
    ]
    .iter()
    .any(|needle| name.contains(needle))
}

#[test]
fn path_wrapper_functions_cover_success_and_loader_error_paths() {
    let temp = TempDir::new("aurora-lib-coverage");
    let main_path = temp.path().join("main.au");
    fs::write(&main_path, "def main():\n    print(1)\n").expect("failed to write main file");

    check_path(&main_path).expect("check_path should succeed");
    check_path_with_source(&main_path, "def main():\n    print(2)\n")
        .expect("check_path_with_source should succeed");

    let path_output = run_path(&main_path).expect("run_path should succeed");
    assert_eq!(path_output.stdout, "1\n");
    let override_output = run_path_with_source(&main_path, "def main():\n    print(2)\n")
        .expect("run_path_with_source should succeed");
    assert_eq!(override_output.stdout, "2\n");

    let mir_output = run_path_via_mir(&main_path).expect("run_path_via_mir should succeed");
    assert_eq!(mir_output.stdout, "1\n");
    let override_mir_output =
        run_path_with_source_via_mir(&main_path, "def main():\n    print(3)\n")
            .expect("run_path_with_source_via_mir should succeed");
    assert_eq!(override_mir_output.stdout, "3\n");

    lower_path_to_mir(&main_path).expect("lower_path_to_mir should succeed");
    lower_path_with_source_to_mir(&main_path, "def main():\n    print(4)\n")
        .expect("lower_path_with_source_to_mir should succeed");

    let cyclic_dir = TempDir::new("aurora-lib-cycle");
    let a_path = cyclic_dir.path().join("a.au");
    let b_path = cyclic_dir.path().join("b.au");
    fs::write(&a_path, "import b\n\ndef main():\n    pass\n").expect("write a");
    fs::write(&b_path, "import a\n\ndef helper():\n    pass\n").expect("write b");
    let cyclic = check_path(&a_path).expect_err("cyclic imports should fail");
    assert!(cyclic.message.contains("cyclic import involving"));
}

#[test]
fn module_loader_helper_functions_cover_namespace_and_export_paths() {
    let temp = TempDir::new("aurora-lib-helpers");
    let pkg_dir = temp.path().join("pkg");
    fs::create_dir_all(&pkg_dir).expect("failed to create pkg dir");
    let user_path = pkg_dir.join("user.au");
    let named_path = pkg_dir.join("named.au");

    fs::write(
        &named_path,
        "public trait Named:\n    def name(self) -> String\n",
    )
    .expect("write named module");
    fs::write(
        &user_path,
        [
            "from pkg.named import Named",
            "",
            "public class Box[T]:",
            "    value: T",
            "    public def read(borrow self) -> T:",
            "        return self.value",
            "",
            "class Hidden:",
            "    value: int32",
            "",
            "public enum Flag[T]:",
            "    Ready",
            "    Value(T)",
            "",
            "enum Secret:",
            "    Hidden",
            "",
            "public trait Show[T]:",
            "    def render(self, other: T) -> String",
            "",
            "trait HiddenTrait:",
            "    def hide(self)",
            "",
            "public def wrap(value: Box[int32]) -> Box[int32]:",
            "    return value",
            "",
            "def hidden() -> int32:",
            "    return 0",
            "",
            "impl[T] Show[T] for Box[T]:",
            "    def render(self, other: T) -> String:",
            "        return \"ok\"",
        ]
        .join("\n"),
    )
    .expect("write user module");

    let relative_path = pkg_dir
        .strip_prefix(std::env::current_dir().expect("cwd"))
        .unwrap_or(&pkg_dir)
        .join("user.au");
    assert!(absolutize(&relative_path).is_absolute());
    assert_eq!(absolutize(&user_path), user_path);

    let inferred_root =
        infer_package_root(&user_path, Some(&fs::read_to_string(&user_path).unwrap()))
            .expect("package root should infer");
    assert_eq!(inferred_root, temp.path().to_path_buf());
    assert!(import_exists_from_root(
        temp.path(),
        &["pkg".to_string(), "named".to_string()]
    ));
    assert_eq!(logical_module_name(temp.path(), &user_path), "pkg.user");
    assert!(is_builtin_export_type("String"));
    assert!(!is_builtin_export_type("Box"));

    let mut program = check_path(&user_path).expect("user module should check");
    assert!(local_item_exists(&program, "Box"));
    assert!(!local_item_exists(&program, "missing"));

    let imported_named = check_path(&named_path).expect("named module should check");
    let remote_namespace =
        exported_namespace(&["pkg".to_string(), "named".to_string()], &imported_named);
    let mut remote_only_namespace = remote_namespace.clone();
    remote_only_namespace.classes.insert(
        "Remote".to_string(),
        program.classes.get("Box").expect("box info").clone(),
    );
    remote_only_namespace.all_classes.insert(
        "Remote".to_string(),
        program.classes.get("Box").expect("box info").clone(),
    );
    program
        .imported_modules
        .insert("named".to_string(), remote_only_namespace.clone());
    program.module_name = "pkg.user".to_string();
    program.source_path = Some(user_path.display().to_string());

    let qualified_local = qualify_export_type(
        &program,
        &crate::sema::Type::Named("Box".to_string(), vec![crate::sema::Type::named("int32")]),
    );
    assert_eq!(
        qualified_local,
        crate::sema::Type::Named(
            "pkg.user.Box".to_string(),
            vec![crate::sema::Type::named("int32")]
        )
    );

    let qualified_imported = qualify_export_type(&program, &crate::sema::Type::named("Remote"));
    assert_eq!(
        qualified_imported,
        crate::sema::Type::named("pkg.named.Remote")
    );

    let mut ambiguous_modules = BTreeMap::new();
    let mut first = remote_namespace.clone();
    first.path = "pkg.named".to_string();
    let mut second = remote_namespace.clone();
    second.path = "pkg.alt".to_string();
    ambiguous_modules.insert(first.name.clone(), first);
    ambiguous_modules.insert("alt".to_string(), second);
    let mut found = None;
    let mut ambiguous = false;
    find_type_namespace_path(&ambiguous_modules, "Named", &mut found, &mut ambiguous);
    assert!(found.is_some());
    assert!(ambiguous);

    let qualified_ref = qualify_export_type_ref(
        &program,
        &TypeRef {
            name: "Box".to_string(),
            args: vec![type_ref("int32")],
            indirect: false,
            span: crate::diag::Span::new(1, 1),
        },
    );
    assert_eq!(qualified_ref.name, "pkg.user.Box");
    assert_eq!(qualified_ref.args[0].name, "int32");
    assert_eq!(
        qualify_export_type_ref(&program, &type_ref("str")).name,
        "str"
    );

    match exported_binding(&program, "wrap").expect("public function export") {
        crate::sema::ImportedBinding::Function(info) => {
            assert_eq!(info.decl.return_type.name, "pkg.user.Box");
        }
        other => panic!("expected function binding, found {other:?}"),
    }
    assert!(exported_binding(&program, "hidden").is_none());

    let namespace = exported_namespace(&["pkg".to_string(), "user".to_string()], &program);
    assert!(namespace.functions.contains_key("wrap"));
    assert!(namespace.classes.contains_key("Box"));
    assert!(namespace.enums.contains_key("Flag"));
    assert!(namespace.traits.contains_key("Show"));
    assert!(!namespace.functions.contains_key("hidden"));
    assert!(!namespace.classes.contains_key("Hidden"));
    assert_eq!(namespace.path, "pkg.user");

    let mut bindings = BTreeMap::new();
    insert_namespace_import(
        &mut bindings,
        &[],
        namespace.clone(),
        crate::diag::Span::new(1, 1),
    )
    .expect("empty namespace import should be ignored");
    insert_namespace_import(
        &mut bindings,
        &["pkg".to_string()],
        namespace.clone(),
        crate::diag::Span::new(1, 1),
    )
    .expect("single-segment namespace import should work");
    insert_namespace_import(
        &mut bindings,
        &["pkg".to_string(), "user".to_string()],
        namespace.clone(),
        crate::diag::Span::new(1, 1),
    )
    .expect("nested namespace import should work");
    let root = match bindings.get("pkg").expect("pkg binding") {
        crate::sema::ImportedBinding::Module(root) => root,
        other => panic!("expected module binding, found {other:?}"),
    };
    assert!(root.modules.contains_key("user"));

    bindings.insert(
        "pkg".to_string(),
        crate::sema::ImportedBinding::Function(
            program.functions.get("wrap").expect("wrap info").clone(),
        ),
    );
    let duplicate = insert_namespace_import(
        &mut bindings,
        &["pkg".to_string(), "other".to_string()],
        namespace,
        crate::diag::Span::new(1, 1),
    )
    .expect_err("non-module root bindings should reject namespace imports");
    assert!(duplicate.message.contains("duplicate import binding `pkg`"));
}

#[test]
fn module_loader_reports_import_resolution_and_export_errors() {
    let temp = TempDir::new("aurora-lib-import-errors");
    let pkg_dir = temp.path().join("pkg");
    fs::create_dir_all(&pkg_dir).expect("failed to create pkg dir");
    let module_path = pkg_dir.join("mod.au");
    fs::write(
        &module_path,
        [
            "def hidden() -> int32:",
            "    return 0",
            "",
            "public class Box:",
            "    value: int32",
            "",
            "public enum Flag:",
            "    Ready",
            "",
            "public trait Show:",
            "    def render(self) -> String",
        ]
        .join("\n"),
    )
    .expect("write module");

    let private_main = temp.path().join("private.au");
    fs::write(&private_main, "from pkg.mod import hidden\n").expect("write private main");
    let private_error = check_path(&private_main).expect_err("private imports should fail");
    assert!(private_error
        .message
        .contains("item `hidden` is private in module `pkg.mod`"));

    let missing_main = temp.path().join("missing.au");
    fs::write(&missing_main, "from pkg.mod import Missing\n").expect("write missing main");
    let missing_error = check_path(&missing_main).expect_err("missing exports should fail");
    assert!(missing_error
        .message
        .contains("module `pkg.mod` has no export named `Missing`"));

    let duplicate_main = temp.path().join("duplicate.au");
    fs::write(
        &duplicate_main,
        "from pkg.mod import Box\nfrom pkg.mod import Box\n",
    )
    .expect("write duplicate main");
    let duplicate_error =
        check_path(&duplicate_main).expect_err("duplicate import bindings should fail");
    assert!(duplicate_error
        .message
        .contains("duplicate import binding `Box`"));

    let unresolved_main = temp.path().join("unresolved.au");
    fs::write(&unresolved_main, "import pkg.missing\n").expect("write unresolved main");
    let unresolved_error = check_path(&unresolved_main).expect_err("missing modules should fail");
    assert!(unresolved_error
        .message
        .contains("cannot resolve module `pkg.missing`"));

    let fallback_root = infer_package_root(&duplicate_main, Some("not: valid: aurora"))
        .expect("invalid override should fall back to the entry dir");
    assert_eq!(fallback_root, temp.path().to_path_buf());

    let program = check_path(&module_path).expect("module should check");
    assert!(matches!(
        exported_binding(&program, "Box"),
        Some(crate::sema::ImportedBinding::Class(_))
    ));
    assert!(matches!(
        exported_binding(&program, "Flag"),
        Some(crate::sema::ImportedBinding::Enum(_))
    ));
    assert!(matches!(
        exported_binding(&program, "Show"),
        Some(crate::sema::ImportedBinding::Trait(_))
    ));
}

#[test]
fn parses_the_point_milestone() {
    let module = parse_source(POINT_SOURCE).expect("point program should parse");
    assert_eq!(module.items.len(), 3);
    assert_eq!(module.top_level_stmts.len(), 0);
}

#[test]
fn type_checks_the_point_milestone() {
    check_source(POINT_SOURCE).expect("point program should type-check");
}

#[test]
fn runs_the_point_milestone() {
    let output = run_source(POINT_SOURCE).expect("point program should run");
    assert_eq!(output.stdout, "5.0\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_the_point_milestone() {
    let output = run_source_via_mir(POINT_SOURCE).expect("point program should run via MIR");
    assert_eq!(output.stdout, "5.0\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn omitted_none_return_type_is_allowed() {
    let module = parse_source(BASIC_ADDITION_SOURCE).expect("basic addition should parse");
    assert_eq!(module.items.len(), 1);
    assert_eq!(module.top_level_stmts.len(), 0);

    let output = run_source(BASIC_ADDITION_SOURCE).expect("basic addition should run");
    assert_eq!(output.stdout, "16\n");
    assert_eq!(output.value, Value::Unit);
}

#[test]
fn top_level_scripts_run_without_main() {
    let module = parse_source(TOP_LEVEL_ADDITION_SOURCE).expect("top-level addition should parse");
    assert_eq!(module.items.len(), 0);
    assert_eq!(module.top_level_stmts.len(), 4);

    let output = run_source(TOP_LEVEL_ADDITION_SOURCE).expect("top-level addition should run");
    assert_eq!(output.stdout, "16\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn control_flow_example_runs() {
    check_source(CONTROL_FLOW_SOURCE).expect("control flow example should type-check");
    let output = run_source(CONTROL_FLOW_SOURCE).expect("control flow example should run");
    assert_eq!(output.stdout, "ok\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_class_methods_example() {
    let source = include_str!("../../../examples/classes/methods.au");
    let output = run_source_via_mir(source).expect("methods example should run via MIR");
    assert_eq!(output.stdout, "4\n8\n0\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_enum_match_example() {
    let source = include_str!("../../../examples/enums/result_match.au");
    let output = run_source_via_mir(source).expect("enum match example should run via MIR");
    assert_eq!(output.stdout, "42\nbad\n0\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_try_example_natively() {
    let source = include_str!("../../../examples/error_handling/try_result.au");
    let mir = lower_source_to_mir(source).expect("try example should lower to MIR");
    let output = run_mir(&mir).expect("try example should run directly through MIR");
    assert_eq!(output.stdout, "6\ndivision by zero\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn backend_path_runs_try_example_natively() {
    let source = include_str!("../../../examples/error_handling/try_result.au");
    let output = run_source_via_mir(source).expect("try example should run through backend path");
    assert_eq!(output.stdout, "6\ndivision by zero\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn backend_path_runs_with_example_natively() {
    let source = include_str!("../../../examples/resources/with_resource.au");
    let output = run_source_via_mir(source).expect("with example should run through backend path");
    assert_eq!(output.stdout, "demo\nclosed demo\ndone\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_with_example_natively() {
    let source = include_str!("../../../examples/resources/with_resource.au");
    let mir = lower_source_to_mir(source).expect("with example should lower to MIR");
    let output = run_mir(&mir).expect("with example should run directly through MIR");
    assert_eq!(output.stdout, "demo\nclosed demo\ndone\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_channels_example_natively() {
    let source = include_str!("../../../examples/concurrency/channels_spawn.au");
    let mir = lower_source_to_mir(source).expect("channels example should lower to MIR");
    let output = run_mir(&mir).expect("channels example should run directly through MIR");
    assert_eq!(output.stdout, "2\n4\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_send_result_example_natively() {
    let source = include_str!("../../../examples/concurrency/send_result.au");
    let mir = lower_source_to_mir(source).expect("send_result example should lower to MIR");
    let output = run_mir(&mir).expect("send_result example should run directly through MIR");
    assert_eq!(output.stdout, "7\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_spawn_detached_example_natively() {
    let source = include_str!("../../../examples/concurrency/spawn_detached.au");
    let mir = lower_source_to_mir(source).expect("spawn_detached example should lower to MIR");
    let output = run_mir(&mir).expect("spawn_detached example should run directly through MIR");
    assert_eq!(output.stdout, "9\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_select_timeout_example_natively() {
    let source = include_str!("../../../examples/concurrency/select_timeout.au");
    let mir = lower_source_to_mir(source).expect("select_timeout example should lower to MIR");
    let output = run_mir(&mir).expect("select_timeout example should run directly through MIR");
    assert_eq!(output.stdout, "timeout\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_select_send_example_natively() {
    let source = include_str!("../../../examples/concurrency/select_send.au");
    let mir = lower_source_to_mir(source).expect("select_send example should lower to MIR");
    let output = run_mir(&mir).expect("select_send example should run directly through MIR");
    assert_eq!(output.stdout, "sent\n4\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_task_group_select_example_natively() {
    let source = include_str!("../../../examples/concurrency/task_group_select.au");
    let mir = lower_source_to_mir(source).expect("task_group_select example should lower to MIR");
    let output = run_mir(&mir).expect("task_group_select example should run directly through MIR");
    assert_eq!(output.stdout, "3\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_runtime_runs_task_group_cancel_example_natively() {
    let source = include_str!("../../../examples/concurrency/task_group_cancel.au");
    let mir = lower_source_to_mir(source).expect("task_group_cancel example should lower to MIR");
    let output = run_mir(&mir).expect("task_group_cancel example should run directly through MIR");
    assert_eq!(output.stdout, "0\n1\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn serialized_mir_runner_executes_point_example() {
    let source = include_str!("../../../examples/point.au");
    let mir = lower_source_to_mir(source).expect("point example should lower to MIR");
    let mir_json = serde_json::to_vec(&mir).expect("MIR should serialize to JSON bytes");
    let output = run_serialized_mir(&mir_json, "/virtual/point.au", source)
        .expect("serialized MIR runner should execute point example");
    assert_eq!(output.stdout, "5.0\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn serialized_mir_runner_reports_invalid_embedded_mir() {
    let error = run_serialized_mir(b"{not json", "/virtual/bad.au", "print(value=1)\n")
        .expect_err("invalid embedded MIR should return a diagnostic");
    assert!(
        error.message.contains("failed to deserialize embedded MIR"),
        "unexpected diagnostic: {}",
        error
    );
}

#[test]
fn path_with_source_mir_lowering_resolves_local_module_imports() {
    let temp = TempDir::new("aurora-compiler-lower-path-source");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/math.au"),
        "public def double(value: int32) -> int32:\n    return value * 2\n",
    )
    .expect("failed to write helper module");
    let main_path = temp.path().join("main.au");
    let source = "import helpers.math\n\ndef main() -> int32:\n    print(helpers.math.double(value=5))\n    return 0\n";
    let mir = lower_path_with_source_to_mir(&main_path, source)
        .expect("path-aware MIR lowering should resolve local imports");
    let output = run_mir(&mir).expect("path-aware MIR lowering should produce runnable MIR");
    assert_eq!(output.stdout, "10\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn imported_function_return_types_keep_members_visible_across_modules() {
    let temp = TempDir::new("aurora-compiler-imported-return-members");
    fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
    fs::write(
        temp.path().join("helpers/counter.au"),
        [
            "public class Counter:",
            "    public value: int32",
            "",
            "public def make_counter() -> Counter:",
            "    return Counter(value=41)",
            "",
        ]
        .join("\n"),
    )
    .expect("failed to write helper module");
    let main_path = temp.path().join("main.au");
    fs::write(
        &main_path,
        [
            "from helpers.counter import make_counter",
            "",
            "def main() -> int32:",
            "    counter = make_counter()",
            "    print(counter.value)",
            "    return 0",
            "",
        ]
        .join("\n"),
    )
    .expect("failed to write main module");

    let checked = check_path(&main_path)
        .expect("return type members from imported functions should stay visible");
    assert!(
        checked.functions.get("main").is_some(),
        "main should still type-check"
    );

    let output = run_path(&main_path).expect("module program should run");
    assert_eq!(output.stdout, "41\n");
    assert_eq!(output.value, zero_exit_value());

    let mir_output = run_path_via_mir(&main_path).expect("module program should run via MIR");
    assert_eq!(mir_output.stdout, "41\n");
    assert_eq!(mir_output.value, zero_exit_value());
}

#[test]
fn broad_scratch_corpus_checks_analysis_and_mir_lowering_do_not_panic() {
    let repo_root = repo_root();
    let corpus_dirs = [repo_root.join("test_edge"), repo_root.join("test_recheck")];

    let mut file_count = 0usize;
    let mut checked_ok = 0usize;
    let mut lowered_ok = 0usize;
    let mut emitted_ok = 0usize;
    let mut emission_panics = 0usize;

    for dir in corpus_dirs {
        for path in collect_aurora_files(&dir) {
            file_count += 1;
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {}", path.display(), error));

            let _ = analyze_path_source(&path, &source);

            if check_path(&path).is_ok() {
                checked_ok += 1;
            }

            if let Ok(mir) = lower_path_to_mir(&path) {
                lowered_ok += 1;
                match catch_unwind(AssertUnwindSafe(|| emit_host_native_object(&mir))) {
                    Ok(Ok(_)) => emitted_ok += 1,
                    Ok(Err(_)) => {}
                    Err(_) => {
                        emission_panics += 1;
                        eprintln!("direct backend panicked for {}", path.display());
                    }
                }
            }
        }
    }

    assert!(file_count >= 800, "expected large scratch corpus");
    assert!(checked_ok > 0, "expected some scratch files to type-check");
    assert!(
        lowered_ok > 0,
        "expected some scratch files to lower to MIR"
    );
    assert!(
        emitted_ok > 0,
        "expected some scratch files to emit native direct objects"
    );
    assert!(
        emission_panics < lowered_ok,
        "expected most lowered scratch files to avoid direct backend panics"
    );
}

#[test]
fn broad_scratch_corpus_runtime_paths_do_not_panic() {
    let repo_root = repo_root();
    let corpus_dirs = [repo_root.join("test_edge"), repo_root.join("test_recheck")];

    let mut runnable = 0usize;
    let mut run_completed = 0usize;
    let mut run_mir_completed = 0usize;
    let mut run_panics = 0usize;
    let mut run_mir_panics = 0usize;

    for dir in corpus_dirs {
        for path in collect_aurora_files(&dir) {
            if !should_execute_runtime_corpus_case(&path) {
                continue;
            }
            if check_path(&path).is_err() {
                continue;
            }
            runnable += 1;
            if runnable % 50 == 0 {
                eprintln!(
                    "runtime corpus progress: processed {} runnable files (current: {})",
                    runnable,
                    path.display()
                );
            }

            match catch_unwind(AssertUnwindSafe(|| run_path(&path))) {
                Ok(Ok(_)) | Ok(Err(_)) => run_completed += 1,
                Err(_) => {
                    run_panics += 1;
                    eprintln!("interpreter panicked for {}", path.display());
                }
            }

            match catch_unwind(AssertUnwindSafe(|| run_path_via_mir(&path))) {
                Ok(Ok(_)) | Ok(Err(_)) => run_mir_completed += 1,
                Err(_) => {
                    run_mir_panics += 1;
                    eprintln!("MIR runtime panicked for {}", path.display());
                }
            }
        }
    }

    assert!(runnable > 0, "expected runnable scratch programs");
    assert!(
        run_completed > 0 && run_mir_completed > 0,
        "expected runtime corpus to exercise both execution paths"
    );
    assert!(
        run_panics < runnable && run_mir_panics < runnable,
        "expected most runtime corpus files to avoid execution panics"
    );
}

#[test]
fn maintained_example_tree_public_paths_do_not_panic() {
    let repo_root = repo_root();
    let examples_dir = repo_root.join("examples");

    let mut file_count = 0usize;
    let mut checked_ok = 0usize;
    let mut lowered_ok = 0usize;
    let mut emitted_ok = 0usize;
    let mut emission_panics = 0usize;
    let mut run_completed = 0usize;
    let mut run_mir_completed = 0usize;
    let mut run_panics = 0usize;
    let mut run_mir_panics = 0usize;

    for path in collect_aurora_files_recursive(&examples_dir) {
        file_count += 1;
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {}", path.display(), error));

        let analysis = analyze_path_source(&path, &source);
        assert!(
            analysis
                .diagnostics
                .iter()
                .all(|diag| !diag.message.contains("internal")),
            "analysis should not report internal diagnostics for {}: {:?}",
            path.display(),
            analysis.diagnostics
        );

        let _ = crate::analysis::complete_path_source(&path, &source, 0, 0, None);

        if check_path(&path).is_ok() {
            checked_ok += 1;
        }

        if let Ok(mir) = lower_path_to_mir(&path) {
            lowered_ok += 1;
            match catch_unwind(AssertUnwindSafe(|| emit_host_native_object(&mir))) {
                Ok(Ok(_)) => emitted_ok += 1,
                Ok(Err(_)) => {}
                Err(_) => {
                    emission_panics += 1;
                    eprintln!("direct backend panicked for example {}", path.display());
                }
            }
        }

        match catch_unwind(AssertUnwindSafe(|| run_path(&path))) {
            Ok(Ok(_)) | Ok(Err(_)) => run_completed += 1,
            Err(_) => {
                run_panics += 1;
                eprintln!("interpreter panicked for example {}", path.display());
            }
        }

        match catch_unwind(AssertUnwindSafe(|| run_path_via_mir(&path))) {
            Ok(Ok(_)) | Ok(Err(_)) => run_mir_completed += 1,
            Err(_) => {
                run_mir_panics += 1;
                eprintln!("MIR runtime panicked for example {}", path.display());
            }
        }
    }

    assert!(
        file_count >= 80,
        "expected maintained example tree to stay broad"
    );
    assert!(
        checked_ok > 0,
        "expected some maintained examples to type-check"
    );
    assert!(
        lowered_ok > 0,
        "expected some maintained examples to lower to MIR"
    );
    assert!(
        emitted_ok > 0,
        "expected some maintained examples to emit native direct objects"
    );
    assert!(
        run_completed > 0 && run_mir_completed > 0,
        "expected maintained examples to exercise both runtime paths"
    );
    assert!(
        emission_panics < lowered_ok,
        "expected most lowered maintained examples to avoid direct backend panics"
    );
    assert!(
        run_panics < file_count && run_mir_panics < file_count,
        "expected most maintained examples to avoid runtime panics"
    );
}

#[test]
fn backend_path_runs_channels_example_natively() {
    let source = include_str!("../../../examples/concurrency/channels_spawn.au");
    let output =
        run_source_via_mir(source).expect("channels example should run through backend path");
    assert_eq!(output.stdout, "2\n4\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn mir_lowering_creates_blocks_for_control_flow() {
    let mir = lower_source_to_mir(CONTROL_FLOW_SOURCE).expect("control flow MIR should lower");
    let script = mir
        .top_level
        .expect("top-level script MIR should be present for control flow example");

    assert!(script.blocks.len() >= 4);
    assert!(script
        .blocks
        .iter()
        .any(|block| block.label.contains("while_cond")));
    assert!(script
        .blocks
        .iter()
        .any(|block| block.label.contains("if_then")));
}

#[test]
fn categorized_examples_type_check() {
    for (path, source) in EXAMPLE_CASES {
        check_source(source).unwrap_or_else(|error| {
            panic!("{} should type-check: {}", path, error);
        });
    }
}

#[test]
fn categorized_examples_run_with_expected_output() {
    let cases = [
            (
                "examples/basics/top_level_script.au",
                EXAMPLE_CASES[0].1,
                "156\n",
            ),
            (
                "examples/basics/main_function.au",
                EXAMPLE_CASES[1].1,
                "16\n",
            ),
            (
                "examples/basics/mutable_bindings.au",
                EXAMPLE_CASES[2].1,
                "5\n",
            ),
            (
                "examples/basics/default_arguments.au",
                EXAMPLE_CASES[3].1,
                "hello world\nhello aurora\n6\n12\n",
            ),
            ("examples/basics/pass_keyword.au", EXAMPLE_CASES[4].1, "0\n"),
            (
                "examples/classes/point_distance.au",
                EXAMPLE_CASES[5].1,
                "5.0\n",
            ),
            (
                "examples/classes/default_fields.au",
                EXAMPLE_CASES[6].1,
                "localhost\n8080\n",
            ),
            (
                "examples/classes/methods.au",
                EXAMPLE_CASES[7].1,
                "4\n8\n0\n",
            ),
            (
                "examples/control_flow/if_elif_else.au",
                EXAMPLE_CASES[8].1,
                "high\n",
            ),
            (
                "examples/control_flow/for_range.au",
                EXAMPLE_CASES[9].1,
                "7\n",
            ),
            (
                "examples/control_flow/while_break_continue.au",
                EXAMPLE_CASES[10].1,
                "ok\n",
            ),
            (
                "examples/enums/result_match.au",
                EXAMPLE_CASES[11].1,
                "42\nbad\n0\n",
            ),
            (
                "examples/enums/result_option.au",
                EXAMPLE_CASES[12].1,
                "4\ndivision by zero\n7\n",
            ),
            (
                "examples/enums/explicit_type_args.au",
                EXAMPLE_CASES[13].1,
                "7\nbad\n",
            ),
            (
                "examples/generics/box_and_wrapper.au",
                EXAMPLE_CASES[14].1,
                "7\nok\n",
            ),
            (
                "examples/traits/greeter.au",
                EXAMPLE_CASES[15].1,
                "hello aurora\nhello aurora\n",
            ),
            (
                "examples/traits/multiple_bounds.au",
                EXAMPLE_CASES[16].1,
                "9\n",
            ),
            (
                "examples/numbers/float_sqrt.au",
                EXAMPLE_CASES[17].1,
                "9.0\n",
            ),
            (
                "examples/numbers/float32_values.au",
                EXAMPLE_CASES[18].1,
                "3.25\n2.0\n5.0\n",
            ),
            (
                "examples/numbers/numeric_casts.au",
                EXAMPLE_CASES[19].1,
                "7\n3.0\n1.25\n2.0\n",
            ),
            (
                "examples/strings/greeting.au",
                EXAMPLE_CASES[20].1,
                "hello, aurora\n",
            ),
            (
                "examples/concurrency/task_group_select.au",
                EXAMPLE_CASES[21].1,
                "3\n",
            ),
            (
                "examples/concurrency/task_group_cancel.au",
                EXAMPLE_CASES[22].1,
                "0\n1\n",
            ),
            (
                "examples/concurrency/select_timeout.au",
                EXAMPLE_CASES[23].1,
                "timeout\n",
            ),
            (
                "examples/concurrency/sleep_builtin.au",
                EXAMPLE_CASES[24].1,
                "start\nend\n",
            ),
            (
                "examples/concurrency/send_result.au",
                EXAMPLE_CASES[25].1,
                "7\n",
            ),
            (
                "examples/concurrency/spawn_detached.au",
                EXAMPLE_CASES[26].1,
                "9\n",
            ),
            (
                "examples/concurrency/select_send.au",
                EXAMPLE_CASES[27].1,
                "sent\n4\n",
            ),
            (
                "examples/enums/wildcard_match.au",
                EXAMPLE_CASES[28].1,
                "2\n",
            ),
            (
                "examples/generics/generic_method_calls.au",
                EXAMPLE_CASES[29].1,
                "7\n",
            ),
            (
                "examples/generics/bounded_types.au",
                EXAMPLE_CASES[30].1,
                "aurora\nempty\n",
            ),
            (
                "examples/traits/marker_trait.au",
                EXAMPLE_CASES[31].1,
                "1\n",
            ),
            (
                "examples/traits/specialized_generic_impl.au",
                EXAMPLE_CASES[32].1,
                "hello\n",
            ),
            (
                "examples/concurrency/minute_duration.au",
                EXAMPLE_CASES[33].1,
                "120000ms\n",
            ),
            (
                "examples/traits/generic_dispatch_multiple_types.au",
                EXAMPLE_CASES[34].1,
                "dog\ncat\n",
            ),
            (
                "examples/strings/string_methods.au",
                EXAMPLE_CASES[35].1,
                "15\ntrue\ntrue\ntrue\naurora repo\n2\naurora\nrepo\naurora lang\naurora repo\nAURORA REPO\nrepo\nnone\naurora\nnone\n11\n",
            ),
            (
                "examples/numbers/numeric_builtins.au",
                EXAMPLE_CASES[36].1,
                "7\n3.5\n2\n12\n9.0\n9.0\n",
            ),
            (
                "examples/collections/map_basics.au",
                EXAMPLE_CASES[37].1,
                "3\ntrue\n1\n1\n5\naurora\n3\n3\n3\n3\ntrue\n",
            ),
            (
                "examples/collections/set_basics.au",
                EXAMPLE_CASES[38].1,
                "3\ntrue\nfalse\ntrue\ntrue\n9\ntrue\ntrue\n1\n",
            ),
            (
                "examples/strings/string_parsing_and_formatting.au",
                EXAMPLE_CASES[39].1,
                "42\n-9000000000\n3.5\ntrue\naurora-lang-tests\ntrue\n12\n4\n9\n3.0\n",
            ),
            (
                "examples/traits/generic_trait_bounds.au",
                EXAMPLE_CASES[40].1,
                "20\n",
            ),
            (
                "examples/traits/operator_traits.au",
                EXAMPLE_CASES[41].1,
                "6\n8\n-6\n-8\n",
            ),
        ];

    for (path, source, expected_stdout) in cases {
        let output = run_source(source).unwrap_or_else(|error| {
            panic!("{} should run: {}", path, error);
        });
        assert_eq!(
            output.stdout, expected_stdout,
            "unexpected stdout for {}",
            path
        );
    }
}

#[test]
fn categorized_examples_run_through_backend_path_with_expected_output() {
    let cases = [
            (
                "examples/basics/top_level_script.au",
                EXAMPLE_CASES[0].1,
                "156\n",
            ),
            (
                "examples/basics/main_function.au",
                EXAMPLE_CASES[1].1,
                "16\n",
            ),
            (
                "examples/basics/mutable_bindings.au",
                EXAMPLE_CASES[2].1,
                "5\n",
            ),
            (
                "examples/basics/default_arguments.au",
                EXAMPLE_CASES[3].1,
                "hello world\nhello aurora\n6\n12\n",
            ),
            ("examples/basics/pass_keyword.au", EXAMPLE_CASES[4].1, "0\n"),
            (
                "examples/classes/point_distance.au",
                EXAMPLE_CASES[5].1,
                "5.0\n",
            ),
            (
                "examples/classes/default_fields.au",
                EXAMPLE_CASES[6].1,
                "localhost\n8080\n",
            ),
            (
                "examples/classes/methods.au",
                EXAMPLE_CASES[7].1,
                "4\n8\n0\n",
            ),
            (
                "examples/control_flow/if_elif_else.au",
                EXAMPLE_CASES[8].1,
                "high\n",
            ),
            (
                "examples/control_flow/for_range.au",
                EXAMPLE_CASES[9].1,
                "7\n",
            ),
            (
                "examples/control_flow/while_break_continue.au",
                EXAMPLE_CASES[10].1,
                "ok\n",
            ),
            (
                "examples/enums/result_match.au",
                EXAMPLE_CASES[11].1,
                "42\nbad\n0\n",
            ),
            (
                "examples/enums/result_option.au",
                EXAMPLE_CASES[12].1,
                "4\ndivision by zero\n7\n",
            ),
            (
                "examples/enums/explicit_type_args.au",
                EXAMPLE_CASES[13].1,
                "7\nbad\n",
            ),
            (
                "examples/generics/box_and_wrapper.au",
                EXAMPLE_CASES[14].1,
                "7\nok\n",
            ),
            (
                "examples/traits/greeter.au",
                EXAMPLE_CASES[15].1,
                "hello aurora\nhello aurora\n",
            ),
            (
                "examples/traits/multiple_bounds.au",
                EXAMPLE_CASES[16].1,
                "9\n",
            ),
            (
                "examples/numbers/float_sqrt.au",
                EXAMPLE_CASES[17].1,
                "9.0\n",
            ),
            (
                "examples/numbers/float32_values.au",
                EXAMPLE_CASES[18].1,
                "3.25\n2.0\n5.0\n",
            ),
            (
                "examples/numbers/numeric_casts.au",
                EXAMPLE_CASES[19].1,
                "7\n3.0\n1.25\n2.0\n",
            ),
            (
                "examples/strings/greeting.au",
                EXAMPLE_CASES[20].1,
                "hello, aurora\n",
            ),
            (
                "examples/concurrency/task_group_select.au",
                EXAMPLE_CASES[21].1,
                "3\n",
            ),
            (
                "examples/concurrency/task_group_cancel.au",
                EXAMPLE_CASES[22].1,
                "0\n1\n",
            ),
            (
                "examples/concurrency/select_timeout.au",
                EXAMPLE_CASES[23].1,
                "timeout\n",
            ),
            (
                "examples/concurrency/sleep_builtin.au",
                EXAMPLE_CASES[24].1,
                "start\nend\n",
            ),
            (
                "examples/concurrency/send_result.au",
                EXAMPLE_CASES[25].1,
                "7\n",
            ),
            (
                "examples/concurrency/spawn_detached.au",
                EXAMPLE_CASES[26].1,
                "9\n",
            ),
            (
                "examples/concurrency/select_send.au",
                EXAMPLE_CASES[27].1,
                "sent\n4\n",
            ),
            (
                "examples/enums/wildcard_match.au",
                EXAMPLE_CASES[28].1,
                "2\n",
            ),
            (
                "examples/generics/generic_method_calls.au",
                EXAMPLE_CASES[29].1,
                "7\n",
            ),
            (
                "examples/generics/bounded_types.au",
                EXAMPLE_CASES[30].1,
                "aurora\nempty\n",
            ),
            (
                "examples/traits/marker_trait.au",
                EXAMPLE_CASES[31].1,
                "1\n",
            ),
            (
                "examples/traits/specialized_generic_impl.au",
                EXAMPLE_CASES[32].1,
                "hello\n",
            ),
            (
                "examples/concurrency/minute_duration.au",
                EXAMPLE_CASES[33].1,
                "120000ms\n",
            ),
            (
                "examples/traits/generic_dispatch_multiple_types.au",
                EXAMPLE_CASES[34].1,
                "dog\ncat\n",
            ),
            (
                "examples/strings/string_methods.au",
                EXAMPLE_CASES[35].1,
                "15\ntrue\ntrue\ntrue\naurora repo\n2\naurora\nrepo\naurora lang\naurora repo\nAURORA REPO\nrepo\nnone\naurora\nnone\n11\n",
            ),
            (
                "examples/numbers/numeric_builtins.au",
                EXAMPLE_CASES[36].1,
                "7\n3.5\n2\n12\n9.0\n9.0\n",
            ),
            (
                "examples/collections/map_basics.au",
                EXAMPLE_CASES[37].1,
                "3\ntrue\n1\n1\n5\naurora\n3\n3\n3\n3\ntrue\n",
            ),
            (
                "examples/collections/set_basics.au",
                EXAMPLE_CASES[38].1,
                "3\ntrue\nfalse\ntrue\ntrue\n9\ntrue\ntrue\n1\n",
            ),
            (
                "examples/strings/string_parsing_and_formatting.au",
                EXAMPLE_CASES[39].1,
                "42\n-9000000000\n3.5\ntrue\naurora-lang-tests\ntrue\n12\n4\n9\n3.0\n",
            ),
            (
                "examples/traits/generic_trait_bounds.au",
                EXAMPLE_CASES[40].1,
                "20\n",
            ),
            (
                "examples/traits/operator_traits.au",
                EXAMPLE_CASES[41].1,
                "6\n8\n-6\n-8\n",
            ),
        ];

    for (path, source, expected_stdout) in cases {
        let output = run_source_via_mir(source).unwrap_or_else(|error| {
            panic!("{} should run through backend path: {}", path, error);
        });
        assert_eq!(
            output.stdout, expected_stdout,
            "unexpected backend-path stdout for {}",
            path
        );
    }
}

#[test]
fn additional_categorized_examples_type_check() {
    for (path, source, _) in ADDITIONAL_EXAMPLE_CASES {
        check_source(source).unwrap_or_else(|error| {
            panic!("{} should type-check: {}", path, error);
        });
    }
}

#[test]
fn additional_categorized_examples_run_with_expected_output() {
    for (path, source, expected_stdout) in ADDITIONAL_EXAMPLE_CASES {
        let output = run_source(source).unwrap_or_else(|error| {
            panic!("{} should run: {}", path, error);
        });
        assert_eq!(
            output.stdout, *expected_stdout,
            "unexpected stdout for {}",
            path
        );
    }
}

#[test]
fn additional_categorized_examples_run_through_backend_path_with_expected_output() {
    for (path, source, expected_stdout) in ADDITIONAL_EXAMPLE_CASES {
        let output = run_source_via_mir(source).unwrap_or_else(|error| {
            panic!("{} should run through backend path: {}", path, error);
        });
        assert_eq!(
            output.stdout, *expected_stdout,
            "unexpected backend-path stdout for {}",
            path
        );
    }
}

#[test]
fn runtime_member_surface_matrix_runs_consistently_in_interpreter_and_mir() {
    let source = r#"
def worker(value: int32) -> int32:
    return value + 1

def main() -> int32:
    text = "  Aurora Repo  "
    trimmed = text.trim()
    print(trimmed.len())
    print(trimmed.contains("Repo"))
    print(trimmed.starts_with("Aurora"))
    print(trimmed.ends_with("Repo"))
    print(trimmed.replace("Repo", "Lang"))
    print(trimmed.to_lower())
    print(trimmed.to_upper())
    words = trimmed.split(" ")
    print("/".join(words))
    match trimmed.strip_prefix("Aurora "):
        case Some(rest):
            print(rest)
        case None:
            print("missing")
    match trimmed.strip_suffix(" Repo"):
        case Some(rest):
            print(rest)
        case None:
            print("missing")

    mut numbers = [1, 2]
    print(numbers.len())
    print(numbers.is_empty())
    mut clone_numbers = numbers.clone()
    clone_numbers.push(3)
    print(clone_numbers.pop())
    print(clone_numbers.get(0))
    print(clone_numbers[1])
    print(clone_numbers.set(0, 9))
    clone_numbers[1] = 8
    print(clone_numbers.remove(0))
    print(clone_numbers.swap(0, 0))
    print(clone_numbers.contains(8))
    print(clone_numbers.insert(1, 7))
    clone_numbers.reverse()
    clone_numbers.extend([5, 6])
    clone_numbers.clear()
    print(clone_numbers.is_empty())

    mut counts = {"a": 1}
    print(counts.len())
    print(counts.is_empty())
    copy_counts = counts.clone()
    print(copy_counts.get("a"))
    print(copy_counts["a"])
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
    print(seen.len())
    print(seen.is_empty())
    copy_seen = seen.clone()
    print(copy_seen.contains("x"))
    print(seen.insert("y"))
    print(seen.remove("x"))
    print(seen.contains("y"))

    jobs: Channel[int32] = channel()
    jobs_copy = jobs.clone()
    print(jobs_copy.send(1))
    print(jobs.recv())
    jobs.close()

    task = spawn worker(4)
    task_copy = task.clone()
    print(task_copy.join())

    with task_group() as group:
        group.cancel()

    return 0
"#;

    let interpreted = run_source(source).expect("runtime member matrix should run");
    let mir = run_source_via_mir(source).expect("runtime member matrix should run via MIR");

    assert_eq!(mir.value, interpreted.value);
    assert_eq!(mir.stdout, interpreted.stdout);
}

#[test]
fn runtime_call_writeback_and_cleanup_surface_runs_consistently_in_interpreter_and_mir() {
    let source = r#"
class Counter:
    value: int32

class Resource:
    closed: bool = false
    def close(borrow mut self):
        self.closed = true

def bump(counter: borrow mut Counter, amount: int32 = 2) -> int32:
    counter.value += amount
    return counter.value

def copy_into(source: borrow Counter, target: borrow mut Counter):
    target.value = source.value

def worker(value: int32) -> int32:
    return value + 1

def main() -> int32:
    mut first = Counter(value=1)
    mut second = Counter(value=0)
    print(bump(counter=first))
    copy_into(source=first, target=second)
    print(second.value)

    mut total = 0
    for i in range(stop=3):
        total += i
    print(total)

    print(abs(-3))
    print(min(8, 2))
    print(max(8, 2))
    print(parse_int32("12"))
    print(cancelled())

    jobs: Channel[int32] = channel()
    print(jobs.send(7))
    select:
        case value = jobs.recv():
            print(value)
        case after(duration=1ms):
            print(99)
    jobs.close()

    sleep(0ms)

    with Resource() as resource:
        print(resource.closed)

    task = spawn worker(4)
    print(task.join())

    with task_group() as group:
        group.cancel()

    return second.value
"#;

    let interpreted =
        run_source(source).expect("writeback/cleanup matrix should run in interpreter");
    let mir =
        run_source_via_mir(source).expect("writeback/cleanup matrix should run via MIR runtime");

    assert_eq!(mir.value, interpreted.value);
    assert_eq!(mir.stdout, interpreted.stdout);
}

#[test]
fn additional_module_examples_run_with_expected_output() {
    let cases = [
        ("examples/modules/namespace_import_types.au", "4\ntrue\n1\n"),
        ("examples/modules/trait_impl_imports.au", "Ada\nAda\n"),
    ];

    for (relative_path, expected_stdout) in cases {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(relative_path);
        check_path(&path).unwrap_or_else(|error| {
            panic!("{} should type-check: {}", relative_path, error);
        });
        let output = run_path(&path).unwrap_or_else(|error| {
            panic!("{} should run: {}", relative_path, error);
        });
        assert_eq!(
            output.stdout, expected_stdout,
            "unexpected stdout for {}",
            relative_path
        );
    }
}

#[test]
fn additional_module_examples_run_through_backend_path_with_expected_output() {
    let cases = [
        ("examples/modules/namespace_import_types.au", "4\ntrue\n1\n"),
        ("examples/modules/trait_impl_imports.au", "Ada\nAda\n"),
    ];

    for (relative_path, expected_stdout) in cases {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(relative_path);
        let output = run_path_via_mir(&path).unwrap_or_else(|error| {
            panic!(
                "{} should run through backend path: {}",
                relative_path, error
            );
        });
        assert_eq!(
            output.stdout, expected_stdout,
            "unexpected backend-path stdout for {}",
            relative_path
        );
    }
}

#[test]
fn module_example_runs_with_expected_output() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/modules/simple_import.au");
    let output = run_path(&path).expect("module example should run");
    assert_eq!(output.stdout, "10\n2\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn module_example_runs_through_backend_path_with_expected_output() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/modules/simple_import.au");
    let output = run_path_via_mir(&path).expect("module example should run via MIR");
    assert_eq!(output.stdout, "10\n2\n");
    assert_eq!(output.value, zero_exit_value());
}

#[test]
fn lib_helper_paths_cover_relative_paths_missing_reads_and_import_qualification() {
    let relative = Path::new("examples/basics/main_function.au");
    assert_eq!(
        absolutize(relative),
        std::env::current_dir()
            .expect("cwd should be available")
            .join(relative)
    );

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic enough")
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("aurora-lib-coverage-{}", unique));
    fs::create_dir_all(&temp_dir).expect("temp dir should be created");

    let read_error = check_path(&temp_dir).expect_err("directories should not be readable");
    assert!(read_error.message.contains("failed to read"));

    let imported = check_source("public class Imported:\n    value: int32\n")
        .expect("imported module should type-check");
    let mut program =
        check_source("def main() -> None:\n    pass\n").expect("program should type-check");
    program.imported_modules.insert(
        "dep".to_string(),
        exported_namespace(&["dep".to_string()], &imported),
    );

    let qualified = qualify_export_type(
        &program,
        &crate::sema::Type::Named("Imported".to_string(), Vec::new()),
    );
    assert_eq!(
        qualified,
        crate::sema::Type::Named("dep.Imported".to_string(), Vec::new())
    );

    let qualified_ref = qualify_export_type_ref(
        &program,
        &TypeRef {
            name: "Imported".to_string(),
            args: Vec::new(),
            indirect: false,
            span: Span::new(1, 1),
        },
    );
    assert_eq!(qualified_ref.name, "dep.Imported");

    let unknown = qualify_export_type(
        &program,
        &crate::sema::Type::Named("Unknown".to_string(), Vec::new()),
    );
    assert_eq!(
        unknown,
        crate::sema::Type::Named("Unknown".to_string(), Vec::new())
    );
    assert_eq!(
        qualify_export_type(&program, &crate::sema::Type::Module("pkg.dep".to_string())),
        crate::sema::Type::Module("pkg.dep".to_string())
    );
    assert_eq!(
        qualify_export_type(&program, &crate::sema::Type::Unit),
        crate::sema::Type::Unit
    );

    let bounds = BTreeMap::from([(
        "T".to_string(),
        vec![TypeRef {
            name: "Imported".to_string(),
            args: Vec::new(),
            indirect: false,
            span: Span::new(2, 3),
        }],
    )]);
    let qualified_bounds = super::qualify_export_bounds(&program, &bounds);
    assert_eq!(qualified_bounds["T"][0].name, "dep.Imported");
}
