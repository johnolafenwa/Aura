use std::path::Path;

#[test]
fn trait_delegation_diagnostics() {
    for (name, code) in [
        ("missing", "AU2060"),
        ("type", "AU2061"),
        ("duplicate", "AU2062"),
        ("duplicate_generic", "AU2062"),
        ("view", "AU2063"),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/check-fail/trait_delegation_{name}.au"
        ));
        let source = std::fs::read_to_string(path).unwrap();
        let error = crate::check_source(&source).expect_err(name);
        assert_eq!(error.code, code, "{error}");
    }
}

#[test]
fn trait_delegation_calls_preserve_the_selected_contract() {
    for name in [
        "trait_delegation",
        "trait_delegation_identity",
        "trait_delegation_chain",
        "trait_delegation_parent_associated",
        "trait_delegation_methods",
        "trait_delegation_specialized",
        "trait_delegation_bound_specialized",
        "trait_delegation_open_specialization",
        "trait_delegation_generic_projection",
        "trait_delegation_generic_associated",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/run-pass/{name}.au"));
        let source = std::fs::read_to_string(&path).unwrap();
        let output = crate::run_source(&source).unwrap();
        assert_eq!(
            output.stdout,
            std::fs::read_to_string(path.with_extension("stdout")).unwrap()
        );
    }
}

#[test]
fn trait_delegation_requires_owned_field_declarations() {
    for declaration in [
        "view value: int64",
        "value: view int64",
        "value: view mut int64",
    ] {
        let source = format!("class Box:\n    {declaration}\n");
        assert_eq!(crate::parse_source(&source).unwrap_err().code, "AU2063");
    }
    assert!(crate::parse_source("class Box:\n    view: int64\n").is_ok());
}

#[test]
fn trait_delegation_imported_associated_types() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/run-pass/trait_delegation_assoc.au");
    let output = crate::run_path(&path).unwrap();
    assert_eq!(output.stdout, "7\n");
}

#[test]
fn trait_delegation_concrete_dispatch_is_static() {
    use crate::mir::{CallTarget, Instruction, MethodDispatch, Rvalue};

    let mir = crate::lower_source_to_mir(include_str!(
        "../../tests/fixtures/run-pass/trait_delegation_identity.au"
    ))
    .unwrap();
    let method = mir
        .functions
        .iter()
        .find(|function| function.name == "Left for Outer.pick")
        .unwrap();
    let calls = method
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::Call { callee, .. },
                ..
            } => Some(callee),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(matches!(calls.as_slice(), [CallTarget::TraitMember {
        dispatch: Some(MethodDispatch::Static { function }), ..
    }] if function == "Left for Inner.pick"));
}

#[test]
fn trait_delegation_open_args_resolve_in_every_native_body() {
    use crate::mir::{CallTarget, Instruction, MethodDispatch, Rvalue};

    let mir = crate::lower_source_to_mir(include_str!(
        "../../tests/fixtures/run-pass/trait_delegation_open_specialization.au"
    ))
    .unwrap();
    for name in [
        "Scale[T] for Car[E, T].scale",
        "Scale[T] for Car[E, T].scale[Engine, str]",
    ] {
        let method = mir
            .functions
            .iter()
            .find(|method| method.name == name)
            .unwrap();
        let [call] = method
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .filter_map(|i| match i {
                Instruction::Assign {
                    value:
                        Rvalue::Call {
                            callee: CallTarget::TraitMember { dispatch, .. },
                            ..
                        },
                    ..
                } => Some(dispatch),
                _ => None,
            })
            .collect::<Vec<_>>()[..]
        else {
            panic!("expected a forwarded call in {name}")
        };
        assert!(
            matches!(
                call,
                Some(MethodDispatch::Static { .. } | MethodDispatch::Candidates { .. })
            ),
            "{name} has no callee"
        );
    }
    crate::native_codegen::emit_host_object(&mir).expect("both bodies should emit machine code");
}

#[test]
fn trait_delegation_parent_witnesses_and_returned_views() {
    for name in ["qualified_associated", "parent_reuse", "returned_view"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/run-pass/trait_delegation_{name}.au"
        ));
        let output = crate::run_path(&path).unwrap();
        assert_eq!(
            output.stdout,
            std::fs::read_to_string(path.with_extension("stdout")).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn trait_delegation_parent_binding_must_match_the_field() {
    let source = include_str!("../../tests/fixtures/run-pass/trait_delegation_parent_reuse.au");
    let source = source.replace(
        "impl Read for Truck by engine:\n    type Output = int64",
        "impl Read for Truck by engine:\n    type Output = str",
    );
    let error = crate::check_source(&source).unwrap_err();
    assert_eq!(error.code, "AU2061");
    assert!(
        error.message.contains("does not implement `Source`"),
        "{error}"
    );
}

#[test]
fn trait_delegation_needs_an_independent_implementation() {
    let source = r#"
trait Named:
    def name(self) -> int64
class Link:
    next: indirect Link
impl Named for Link by next
"#;
    let error = crate::check_source(source).unwrap_err();
    assert_eq!(error.code, "AU2061");
    assert!(
        error.message.contains("field `next` of type `Link`"),
        "{error}"
    );
}

#[test]
fn trait_delegation_invalid_headers_preserve_trait_diagnostics() {
    let source = include_str!("../../tests/fixtures/check-pass/trait_delegation.au");
    let header = "impl Named for Car by engine";
    let extra_method =
        "impl Named for Car by engine:\n    def typo(self) -> int64:\n        return 0";
    let second_impl = concat!(
        "impl Named for Car:\n    def number(self) -> int64:\n        return 0\n\n",
        "impl Named for Car by engine",
    );
    for (declaration, expected) in [
        ("impl Missing for Car by engine", "unknown trait `Missing`"),
        (extra_method, "method `typo` is not part of trait `Named`"),
        (second_impl, "duplicate impl of trait `Named` for `Car`"),
    ] {
        let error = crate::check_source(&source.replace(header, declaration)).unwrap_err();
        assert_eq!(error.message, expected);
    }
}
