use crate::{check_source, parse_source, run_source};

#[test]
fn contextual_suites_parse_and_inactive_names_are_not_resolved() {
    let source = include_str!("../tests/fixtures/parse-pass/conditional_declarations.au");
    assert!(parse_source(source).is_ok());
    let source = include_str!("../tests/fixtures/check-pass/conditional_inactive_names.au");
    check_source(source).unwrap();
}

#[test]
fn computed_predicate_selects_one_definition() {
    let source = include_str!("../tests/fixtures/run-pass/conditional_computed_predicate.au");
    assert_eq!(run_source(source).unwrap().stdout, "16\n");
}

#[test]
fn malformed_inactive_body_keeps_its_source_location() {
    let error =
        parse_source("import build\nif build.freestanding:\n    def broken(:\n        pass\n")
            .unwrap_err();
    let ordinary =
        parse_source("import build\nif condition:\n    def broken(:\n        pass\n").unwrap_err();
    assert_eq!(error.span, ordinary.span);
    assert_eq!(error.code, ordinary.code);
}

#[test]
fn compile_time_condition_requires_a_boolean() {
    let source = include_str!("../tests/fixtures/check-fail/conditional_invalid_predicate.au");
    assert_eq!(check_source(source).unwrap_err().code, "AU2071");
}

#[test]
fn imported_const_predicates_can_guard_imports() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/check-path-pass/conditional_imported.au");
    let program = crate::check_path(&path).unwrap();
    assert_eq!(program.module.inactive_lines, [5..6, 10..11]);
}

#[test]
fn runtime_arms_keep_their_lexical_bindings() {
    use crate::ast::{Item, Stmt};
    let module = parse_source(concat!(
        "from build import hosted as active\n",
        "def choose(active: bool) -> int64:\n",
        "    if active:\n        return 1\n",
        "    else:\n        return 0\n",
        "def main():\n",
        "    def nested(active: bool) -> int64:\n",
        "        if active:\n            return 3\n",
        "        return 4\n",
        "    print(choose(false))\n"
    ))
    .unwrap();
    let Item::Function(choose) = &module.items[0] else {
        panic!("function")
    };
    let Stmt::If(branch) = &choose.body[0] else {
        panic!("runtime conditional")
    };
    assert_eq!(branch.branches.len(), 1);
    assert!(module.inactive_lines.is_empty());
}

#[test]
fn runtime_if_with_a_compile_time_elif_keeps_only_reachable_suites() {
    let source = concat!(
        "import build\n",
        "def label(value: bool) -> str:\n",
        "    if value:\n        return \"runtime\"\n",
        "    elif build.hosted:\n        return \"host\"\n",
        "    elif build.cfg[\"unavailable\"] == \"yes\":\n        return bad()\n",
        "    else:\n        return another_bad_call()\n",
        "def main():\n    print(label(true))\n    print(label(false))\n",
    );
    let module = parse_source(source).unwrap();
    assert_eq!(module.inactive_lines, [7..8, 9..10]);
    assert_eq!(run_source(source).unwrap().stdout, "runtime\nhost\n");
}

#[test]
fn syntax_only_parses_inactive_types_without_evaluating_the_guard() {
    let source = "import build\nclass Header:\n    if build.os:\n        word: MissingType\n    else:\n        flag: bool\n";
    let syntax = crate::parser::parse_all_syntax(source).unwrap();
    let crate::ast::Item::Class(header) = &syntax.items[0] else {
        panic!("class")
    };
    assert_eq!(header.fields.len(), 2);
    assert_eq!(parse_source(source).unwrap_err().code, "AU2071");
    let invalid =
        "import build\nif build.freestanding:\n    def missing_arrow(x int64):\n        pass\n";
    assert_eq!(parse_source(invalid).unwrap_err().span.unwrap().line, 3);
}

#[test]
fn unreachable_nested_declarations_never_evaluate_their_conditions() {
    let source = concat!(
        "import build\n",
        "if build.freestanding:\n",
        "    if never_available:\n",
        "        extern \"C\" def foreign_only() -> NativeOnly\n",
        "else:\n",
        "    public def answer() -> int64:\n        return 17\n",
        "def main():\n    print(answer())\n",
    );
    assert_eq!(run_source(source).unwrap().stdout, "17\n");
}

#[test]
fn file_predicates_can_call_a_function_from_an_import() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/check-path-pass/conditional_imported.au");
    let source = concat!(
        "from conditional_support.features import enough\n",
        "const selected = enough(7)\n",
        "if selected:\n    available = 9\n",
        "else:\n    import missing\n",
        "def main():\n    print(available)\n",
    );
    let module = crate::parse_path_with_source(&path, source).unwrap();
    assert_eq!(module.inactive_lines, [5..6]);
    assert_eq!(
        crate::run_path_with_source(&path, source).unwrap().stdout,
        "9\n"
    );
}

#[test]
fn computed_guard_selects_targets_in_called_function_bodies() {
    let source = concat!(
        "import build\n",
        "def probe() -> bool:\n",
        "    if build.hosted:\n        return true\n",
        "    else:\n        return absent_native_call()\n",
        "const ready = probe()\n",
        "if ready:\n    def value() -> int64:\n        return 19\n",
        "def main():\n    print(value())\n",
    );
    assert_eq!(run_source(source).unwrap().stdout, "19\n");
}

#[test]
fn recursive_predicate_selection_reports_a_cycle() {
    let source = concat!(
        "def probe() -> bool:\n",
        "    if ready:\n        return true\n",
        "    return false\n",
        "const ready = probe()\n",
        "if ready:\n    def selected() -> bool:\n        return true\n",
    );
    let error = check_source(source).unwrap_err();
    assert_eq!(error.code, "AU2071");
    assert!(error.message.contains("cyclic"));
}

#[test]
fn discarded_script_statements_do_not_end_the_module_constant_section() {
    let source = concat!(
        "import build\n",
        "if build.freestanding:\n    missing_script_entry()\n",
        "answer = 42\n",
        "def main():\n    print(answer)\n",
    );
    assert_eq!(run_source(source).unwrap().stdout, "42\n");
    let active = concat!(
        "import build\n",
        "if build.hosted:\n    print(1)\n",
        "answer = 42\n",
        "print(answer)\n",
    );
    let module = parse_source(active).unwrap();
    assert!(module.constants.is_empty());
    assert_eq!(run_source(active).unwrap().stdout, "1\n42\n");
}

#[test]
fn a_static_fact_mixed_with_a_runtime_operand_keeps_script_control_flow() {
    let source = "import build\nmut flag = true\nif build.hosted and flag:\n    print(1)\n";
    assert_eq!(run_source(source).unwrap().stdout, "1\n");
}

#[test]
fn selected_scopes_end_views_at_their_last_use() {
    let source = include_str!("../tests/fixtures/run-pass/conditional_view_scope.au");
    assert_eq!(run_source(source).unwrap().stdout, "old\nnew\n");
}

#[test]
fn nested_declaration_suites_can_define_marked_bindings() {
    let source = concat!(
        "import build\n",
        "if build.hosted:\n",
        "    if build.freestanding:\n        const arguments = [\"bare\"]\n",
        "    else:\n        const arguments = [\"host\"]\n",
        "else:\n    const arguments = [\"other\"]\n",
        "def main():\n    print(arguments[0])\n",
    );
    assert_eq!(run_source(source).unwrap().stdout, "host\n");
}
