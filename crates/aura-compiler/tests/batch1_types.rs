//! Public semantic identity contracts, pinned before union/alias implementation.
use aura_compiler::{check_source, sema::Type};

fn parameter(source_type: &str) -> Type {
    let source = format!("def accept(value: {source_type}):\n    pass\n");
    check_source(&source).expect("type must resolve").functions["accept"]
        .signature
        .params[0]
        .clone()
}

#[test]
fn normalization_flattens_deduplicates_and_orders_none_last() {
    let first = parameter("None | str | int | int64 | (str | None)");
    let reordered = parameter("int64 | str | None");
    assert_eq!(first, reordered);
    assert_eq!(first.to_string(), "int64 | str | None");
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&reordered).unwrap()
    );
}

#[test]
fn singleton_normalization_has_member_identity() {
    assert_eq!(parameter("int | int64"), Type::named("int64"));
    assert_eq!(parameter("None | None"), Type::Unit);
    assert_eq!(
        parameter("(str | str, int64 | int)"),
        Type::Tuple(vec![Type::named("str"), Type::named("int64")])
    );
}

#[test]
fn unions_normalize_inside_containers_without_erasing_constructors() {
    assert_eq!(
        parameter("list[None | int | int64]"),
        parameter("list[int64 | None]")
    );
    assert_ne!(
        parameter("list[int64] | list[str]"),
        parameter("list[int64 | str]")
    );
}

#[test]
fn alias_forward_references_and_generic_targets_expand_transparently() {
    let program = check_source("type Values[T] = list[Optional[T]]\ntype Optional[T] = T | None\ndef accept(value: Values[int64]):\n    pass\n").expect("forward generic alias");
    assert_eq!(
        program.functions["accept"].signature.params[0],
        parameter("list[int64 | None]")
    );
}

#[test]
fn unused_alias_targets_are_checked() {
    let error = check_source("type Broken = Missing\n")
        .err()
        .expect("unused aliases are still checked declarations");
    assert_eq!(error.code, "AU2001");
    assert_eq!(error.message, "unknown type `Missing`");
    assert_eq!(error.span, Some(aura_compiler::Span::new(1, 15)));
}

#[test]
fn aliases_share_the_item_namespace() {
    let error = check_source("type Choice = int64\nclass Choice:\n    pass\n")
        .err()
        .expect("duplicate item");
    assert!(error.message.contains("duplicate item `Choice`"), "{error}");
}

#[test]
fn alias_type_parameters_shadow_alias_names() {
    let program = check_source(
        "type T = str\ntype Box[T] = list[T]\ndef accept(value: Box[int64]):\n    pass\n",
    )
    .expect("lexical generic parameter");
    assert_eq!(
        program.functions["accept"].signature.params[0],
        Type::Named("list".into(), vec![Type::named("int64")])
    );
}

#[test]
fn foundation_diagnostic_codes_are_reserved_without_reusing_existing_codes() {
    use aura_compiler::diag::DIAGNOSTIC_CODE_REGISTRY;
    for code in ["AU2010", "AU2011", "AU2012", "AU2013", "AU2014", "AU2015"] {
        assert_eq!(
            DIAGNOSTIC_CODE_REGISTRY
                .iter()
                .filter(|entry| entry.code == code)
                .count(),
            1,
            "{code} must be reserved exactly once"
        );
    }
}

#[test]
fn imported_aliases_expand_in_their_defining_module() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/check-path-pass");
    for name in [
        "alias_imported_named",
        "alias_imported_renamed",
        "alias_imported_qualified",
        "alias_imported_generic",
        "alias_imported_identity",
    ] {
        aura_compiler::check_path(&root.join(format!("{name}.au")))
            .unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}

#[test]
fn callable_member_keys_keep_written_names_and_default_promises() {
    assert_ne!(
        parameter("def(left: int64) -> int64 | str"),
        parameter("def(right: int64) -> int64 | str")
    );
    assert_ne!(
        parameter("def(value: int64 = ...) -> int64 | str"),
        parameter("def(value: int64) -> int64 | str")
    );
}

#[test]
fn function_return_union_display_preserves_precedence() {
    assert_eq!(
        parameter("def() -> (int64 | str)").to_string(),
        "def() -> (int64 | str)"
    );
}

#[test]
fn public_alias_cannot_conceal_a_private_nominal_type() {
    let error = check_source("class Secret:\n    pass\npublic type Visible = list[Secret]\n")
        .err()
        .expect("public expansion must be accessible");
    assert_eq!(error.code, "AU2005");
    assert_eq!(
        error.message,
        "public type alias `Visible` exposes private type `Secret`"
    );
    assert_eq!(error.span, Some(aura_compiler::Span::new(3, 28)));
}

#[test]
fn private_alias_in_public_signature_cannot_conceal_a_private_type() {
    let error = check_source("class Secret:\n    pass\ntype Hidden = Secret\npublic def accept(value: Hidden):\n    pass\n").err().expect("public alias use must be accessible");
    assert_eq!(error.code, "AU2005");
    assert_eq!(
        error.message,
        "public signature uses alias `Hidden` exposing private type `Secret`"
    );
    assert_eq!(error.span, Some(aura_compiler::Span::new(4, 26)));
}

#[test]
fn alias_bounds_are_checked_even_when_the_target_erases_the_parameter() {
    let source = "trait Named:\n    def name(self) -> str\nclass Missing:\n    pass\ntype Marker[T: Named] = int64\ndef accept(value: Marker[Missing]):\n    pass\n";
    let error = check_source(source)
        .err()
        .expect("specializing an alias checks its bounds");
    assert_eq!(error.code, "AU2002");
    assert_eq!(
        error.message,
        "type `Missing` does not implement trait `Named`"
    );
    assert_eq!(error.span, Some(aura_compiler::Span::new(6, 19)));
}

#[test]
fn unused_alias_bounds_are_resolved() {
    let error = check_source("type Marker[T: Missing] = int64\n")
        .err()
        .expect("unknown alias bound");
    assert_eq!(error.code, "AU2001");
    assert_eq!(error.message, "unknown trait `Missing`");
}

#[test]
fn aliases_lower_as_their_expansion_and_expose_existing_constructors() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-pass");
    for name in [
        "alias_scalar_annotation",
        "alias_singleton_union_annotation",
        "alias_nominal_constructor",
        "alias_generic_constructor",
    ] {
        let output = aura_compiler::run_path(&root.join(format!("{name}.au")))
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(output.stdout, "42\n", "{name}");
    }
}

#[test]
fn alias_bounds_apply_to_local_annotations_and_generic_proofs() {
    let prefix = "trait Named:\n    def name(self) -> str\nclass Missing:\n    pass\ntype Marker[T: Named] = int64\n";
    let error = check_source(&format!(
        "{prefix}def body():\n    value: Marker[Missing] = 1\n"
    ))
    .err()
    .expect("local specialization must check bounds");
    assert_eq!(
        error.message,
        "type `Missing` does not implement trait `Named`"
    );
    check_source(&format!(
        "{prefix}def accepted[T: Named](value: Marker[T]):\n    pass\n"
    ))
    .expect("declared generic bound is proof");
    let error = check_source(&format!(
        "{prefix}def rejected[T](value: Marker[T]):\n    pass\n"
    ))
    .err()
    .expect("unconstrained parameter is not proof");
    assert_eq!(
        error.message,
        "type parameter `T` does not satisfy trait bound `Named`"
    );
}

#[test]
fn exported_alias_bounds_keep_the_defining_trait_identity() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/check-path-pass");
    aura_compiler::check_path(&root.join("alias_imported_bounds.au"))
        .expect("imported bound is qualified");
}

#[test]
fn public_generic_alias_specialization_cannot_hide_private_type_arguments() {
    let error = check_source("class Secret:\n    pass\ntype Identity[T] = T\npublic def accept(value: Identity[Secret]):\n    pass\n").err().expect("specialized expansion must be public");
    assert_eq!(error.code, "AU2005");
    assert_eq!(
        error.message,
        "public signature uses alias `Identity` exposing private type `Secret`"
    );
}

#[test]
fn same_spelling_trait_in_consumer_does_not_satisfy_imported_alias_bound() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/check-path-pass/alias_support/wrong_bound.au");
    let error = aura_compiler::check_path(&path)
        .err()
        .expect("different defining traits are distinct");
    assert!(
        error
            .message
            .contains("does not implement trait `types.Named`"),
        "{error}"
    );
}

#[test]
fn callable_member_identity_retains_keyword_only_boundary() {
    assert_ne!(
        parameter("def(value: int64) -> int64 | str"),
        parameter("def(*, value: int64) -> int64 | str")
    );
}

#[test]
fn aliases_retain_imported_body_and_member_constructor_behavior() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-pass");
    for name in [
        "alias_module_rename_generic",
        "alias_imported_renamed_module_body",
        "alias_module_rename",
        "alias_nested_inferred_constructor",
        "alias_imported_private_body",
        "alias_imported_constructor",
        "alias_qualified_constructor",
        "alias_enum_member",
        "alias_associated_member",
        "alias_fixed_generic_constructor",
        "alias_inferred_generic_constructor",
    ] {
        let output = aura_compiler::run_path(&root.join(format!("{name}.au")))
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(output.stdout, "42\n", "{name}");
    }
}

#[test]
fn alias_constructor_specializations_enforce_alias_bounds() {
    let prefix = "trait Named:\n    def name(self) -> str\nclass Missing:\n    pass\nclass Box[T]:\n    value: T\ntype Container[T: Named] = Box[T]\n";
    for constructor in ["Container[Missing]", "Container"] {
        let error = check_source(&format!(
            "{prefix}def main():\n    value = {constructor}(value=Missing())\n"
        ))
        .err()
        .expect("constructor specialization checks the alias bound");
        assert_eq!(error.code, "AU2002", "{constructor}: {error}");
        assert!(
            error.message.contains("does not implement trait `Named`"),
            "{constructor}: {error}"
        );
    }
}

#[test]
fn alias_constructor_inference_preserves_nested_and_repeated_parameters() {
    let prefix =
        "class Pair[A, B]:\n    first: A\n    second: B\ntype Nested[T] = Pair[list[T], T]\n";
    check_source(&format!(
        "{prefix}def main():\n    value = Nested(first=[42], second=42)\n"
    ))
    .expect("infer nested alias parameter");
    let error = check_source(&format!(
        "{prefix}def main():\n    value = Nested(first=[42], second=\"wrong\")\n"
    ))
    .err()
    .expect("repeated parameter must agree");
    assert_eq!(error.code, "AU2002");
}

#[test]
fn unions_reject_incomplete_and_non_value_members_with_the_union_diagnostic() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/check-fail");
    for name in [
        "union_member_bare_list",
        "union_member_bare_class",
        "union_member_trait",
    ] {
        let source = std::fs::read_to_string(root.join(format!("{name}.au"))).unwrap();
        let error = check_source(&source)
            .err()
            .expect("member is not a complete value type");
        assert_eq!(error.code, "AU2010", "{name}: {error}");
        assert_eq!(error.span.unwrap().column, 15);
    }
}

#[test]
fn alias_diagnostics_keep_the_written_name_and_expansion() {
    let prefix = "type Count = int64\n";
    let error = check_source(&format!(
        "{prefix}def accept(value: Count):\n    pass\ndef main():\n    accept(value=\"wrong\")\n"
    ))
    .err()
    .expect("argument mismatch");
    assert!(
        error
            .message
            .contains("expected Count (= int64), found str"),
        "{error}"
    );
    let error = check_source(&format!(
        "{prefix}def main():\n    value: Count = \"wrong\"\n"
    ))
    .err()
    .expect("annotation mismatch");
    assert!(error.message.contains("Count (= int64)"), "{error}");
}
