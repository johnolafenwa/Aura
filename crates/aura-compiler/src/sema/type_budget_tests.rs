use std::collections::HashMap;

use super::type_budget::ExpansionBudget;
use super::Type;
use crate::diag::Span;

const TEST_SPAN: Span = Span {
    line: 7,
    column: 11,
};

#[test]
fn substitution_preflight_counts_the_prospective_tree_without_cloning() {
    let budget = ExpansionBudget::with_limits(8, 8, 8);
    let template = Type::Tuple(vec![
        Type::TypeParam("T".to_string()),
        Type::Named("list".to_string(), vec![Type::TypeParam("T".to_string())]),
    ]);
    let substitutions = HashMap::from([(
        "T".to_string(),
        Type::Tuple(vec![Type::Unit, Type::named("int64")]),
    )]);

    budget
        .check_substitution(&template, &substitutions, TEST_SPAN)
        .expect("the prospective result has exactly eight nodes");
    assert_eq!(budget.aggregate_nodes(), 8);
}

#[test]
fn replacement_type_parameters_are_not_resubstituted() {
    let budget = ExpansionBudget::with_limits(3, 3, 3);
    let template = Type::TypeParam("T".to_string());
    let substitutions = HashMap::from([(
        "T".to_string(),
        Type::Named("list".to_string(), vec![Type::TypeParam("T".to_string())]),
    )]);

    budget
        .check_substitution(&template, &substitutions, TEST_SPAN)
        .expect("the replacement is cloned as-is and has two nodes");
    assert_eq!(budget.aggregate_nodes(), 2);
}

#[test]
fn self_referential_substitution_value_terminates_as_one_node() {
    let budget = ExpansionBudget::with_limits(1, 1, 1);
    let template = Type::TypeParam("T".to_string());
    let substitutions = HashMap::from([("T".to_string(), Type::TypeParam("T".to_string()))]);

    budget
        .check_substitution(&template, &substitutions, TEST_SPAN)
        .expect("a replacement TypeParam is not recursively substituted");
    assert_eq!(budget.aggregate_nodes(), 1);
}

#[test]
fn per_result_failure_does_not_charge_the_shared_aggregate() {
    let budget = ExpansionBudget::with_limits(3, 8, 20);
    let template = Type::Tuple(vec![Type::Unit, Type::Unit, Type::Unit]);

    let error = budget
        .check_substitution(&template, &HashMap::new(), TEST_SPAN)
        .expect_err("four result nodes must exceed the injected limit");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        "type expansion exceeds per-result node limit of 3"
    );
    assert_eq!(error.span, Some(TEST_SPAN));
    assert_eq!(budget.aggregate_nodes(), 0);
}

#[test]
fn iterative_preflight_rejects_excessive_structural_depth() {
    let budget = ExpansionBudget::with_limits(20, 3, 20);
    let template = Type::Named(
        "list".to_string(),
        vec![Type::Named(
            "list".to_string(),
            vec![Type::Named("list".to_string(), vec![Type::Unit])],
        )],
    );

    let error = budget
        .check_substitution(&template, &HashMap::new(), TEST_SPAN)
        .expect_err("a four-level tree must exceed the injected depth limit");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        "type expansion exceeds recursion depth limit of 3"
    );
    assert_eq!(budget.aggregate_nodes(), 0);
}

#[test]
fn aggregate_limit_is_checked_before_committing_the_next_charge() {
    let budget = ExpansionBudget::with_limits(4, 4, 4);
    let template = Type::Tuple(vec![Type::Unit]);

    budget
        .check_substitution(&template, &HashMap::new(), TEST_SPAN)
        .unwrap();
    budget
        .check_substitution(&template, &HashMap::new(), TEST_SPAN)
        .unwrap();
    let error = budget
        .check_substitution(&template, &HashMap::new(), TEST_SPAN)
        .expect_err("the third two-node result must exceed the aggregate limit");

    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        "type expansion exceeds aggregate node limit of 4"
    );
    assert_eq!(budget.aggregate_nodes(), 4);
}

#[test]
fn depth_guard_restores_capacity_on_drop_and_after_rejection() {
    let budget = ExpansionBudget::with_limits(8, 2, 20);
    let outer = budget.enter(TEST_SPAN).unwrap();
    let inner = budget.enter(TEST_SPAN).unwrap();

    let error = budget
        .enter(TEST_SPAN)
        .map(|_| ())
        .expect_err("a third active frame must exceed the injected depth limit");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        "type expansion exceeds recursion depth limit of 2"
    );

    drop(inner);
    let replacement_inner = budget
        .enter(TEST_SPAN)
        .expect("dropping a guard must restore one expansion slot");
    drop(replacement_inner);
    drop(outer);
    let _root = budget
        .enter(TEST_SPAN)
        .expect("dropping all guards must restore the root slot");
}

#[test]
fn function_and_closure_metadata_are_part_of_the_budgeted_tree() {
    use super::{ClosureCallKind, ClosureCapture, ClosureCaptureMode, FunctionParamContract};
    use crate::ast::ReceiverKind;

    let leaf_param = FunctionParamContract {
        name: "value".to_string(),
        ty: Type::Unit,
        passing: ReceiverKind::Borrow,
        has_default: false,
        default_erased: false,
        keyword_only: false,
    };
    let template = Type::Closure {
        params: Box::new(vec![leaf_param.clone()]),
        return_type: Box::new(Type::Function {
            params: vec![leaf_param],
            return_type: Box::new(Type::Unit),
        }),
        captures: Box::new(vec![ClosureCapture {
            name: "saved".to_string(),
            ty: Type::Unit,
            mode: ClosureCaptureMode::Move,
            span: TEST_SPAN,
        }]),
        call_kind: ClosureCallKind::Repeatable,
    };
    let budget = ExpansionBudget::with_limits(5, 8, 20);

    let error = budget
        .check_substitution(&template, &HashMap::new(), TEST_SPAN)
        .expect_err("closure parameters, captures, and nested signatures must all count");
    assert_eq!(
        error.message,
        "type expansion exceeds per-result node limit of 5"
    );
}

#[test]
fn canonical_key_preflight_counts_json_escaping_before_allocation() {
    let budget = ExpansionBudget::with_key_limits(8, 8, 40);
    let ty = Type::Named("quoted\"and\\escaped".to_string(), Vec::new());

    let error = budget
        .check_canonical_key(&ty, "module", &Default::default(), TEST_SPAN)
        .expect_err("escaped nominal metadata must count against the byte limit");
    assert_eq!(error.code, "AU2999");
    assert_eq!(error.message, "canonical type key exceeds byte limit of 40");
    assert_eq!(error.span, Some(TEST_SPAN));
}

#[test]
fn canonical_key_preflight_has_independent_node_and_depth_limits() {
    let three_nodes = Type::Tuple(vec![Type::Unit, Type::Unit]);
    let node_error = ExpansionBudget::with_key_limits(2, 8, 1_000)
        .check_canonical_key(&three_nodes, "", &Default::default(), TEST_SPAN)
        .expect_err("three key nodes must exceed the injected node limit");
    assert_eq!(
        node_error.message,
        "canonical type key exceeds node limit of 2"
    );

    let two_levels = Type::Named("list".to_string(), vec![Type::Unit]);
    let depth_error = ExpansionBudget::with_key_limits(8, 1, 1_000)
        .check_canonical_key(&two_levels, "", &Default::default(), TEST_SPAN)
        .expect_err("a nested key must exceed the injected depth limit");
    assert_eq!(
        depth_error.message,
        "canonical type key exceeds depth limit of 1"
    );
}

#[test]
fn canonical_key_preflight_accepts_bounded_callable_metadata() {
    use super::FunctionParamContract;
    use crate::ast::ReceiverKind;

    let ty = Type::Function {
        params: vec![FunctionParamContract {
            name: "value".to_string(),
            ty: Type::named("int64"),
            passing: ReceiverKind::BorrowMut,
            has_default: true,
            default_erased: false,
            keyword_only: false,
        }],
        return_type: Box::new(Type::Unit),
    };

    ExpansionBudget::with_key_limits(4, 4, 256)
        .check_canonical_key(&ty, "", &Default::default(), TEST_SPAN)
        .expect("small callable metadata must fit the injected key budget");
}

#[test]
fn union_key_preflight_uses_the_union_provenance_and_canonical_members() {
    use super::types::UnionType;

    let ty = Type::Union(Box::new(UnionType {
        members: vec![Type::Named("Alias".to_string(), Vec::new()), Type::Unit],
        keys: Vec::new(),
        module_name: "origin".to_string(),
    }));
    let canonical_names = std::collections::BTreeMap::from([(
        "Alias".to_string(),
        "different.OverlongReplacement".to_string(),
    )]);
    let exact_key_bytes = ty.canonical_key("consumer", &canonical_names).len();

    ExpansionBudget::with_key_limits(4, 4, exact_key_bytes)
        .check_canonical_key(&ty, "consumer", &canonical_names, TEST_SPAN)
        .expect("union members use stored defining-module provenance and no outer alias map");
}

#[test]
fn substitution_key_preflight_charges_each_prospective_replacement() {
    let template = Type::Tuple(vec![
        Type::TypeParam("T".to_string()),
        Type::TypeParam("T".to_string()),
    ]);
    let replacement = Type::Named("ANameThatIsLongWhenRepeated".to_string(), Vec::new());
    let substitutions = HashMap::from([("T".to_string(), replacement.clone())]);
    let prospective = Type::Tuple(vec![replacement.clone(), replacement]);
    let exact_key_bytes = prospective
        .canonical_key("consumer", &Default::default())
        .len();

    let error = ExpansionBudget::with_key_limits(8, 8, exact_key_bytes - 1)
        .check_substitution_key(
            &template,
            &substitutions,
            "consumer",
            &Default::default(),
            TEST_SPAN,
        )
        .expect_err("both qualified replacement names must count before substitution");
    assert_eq!(
        error.message,
        format!(
            "canonical type key exceeds byte limit of {}",
            exact_key_bytes - 1
        )
    );

    ExpansionBudget::with_key_limits(8, 8, exact_key_bytes)
        .check_substitution_key(
            &template,
            &substitutions,
            "consumer",
            &Default::default(),
            TEST_SPAN,
        )
        .expect("the estimator must accept the exact prospective key size");
}

#[test]
fn substitution_key_preflight_does_not_resubstitute_replacement_parameters() {
    let template = Type::TypeParam("T".to_string());
    let substitutions = HashMap::from([(
        "T".to_string(),
        Type::Named("list".to_string(), vec![Type::TypeParam("T".to_string())]),
    )]);
    let prospective = Type::Named("list".to_string(), vec![Type::TypeParam("T".to_string())]);
    let exact_key_bytes = prospective.canonical_key("", &Default::default()).len();

    ExpansionBudget::with_key_limits(3, 3, exact_key_bytes)
        .check_substitution_key(
            &template,
            &substitutions,
            "",
            &Default::default(),
            TEST_SPAN,
        )
        .expect("replacement TypeParams remain literal and terminate");
}

#[test]
fn substitution_inside_template_union_uses_the_callers_provenance() {
    let template = Type::normalize_union(
        vec![Type::TypeParam("T".to_string()), Type::Unit],
        "origin",
        &Default::default(),
    )
    .expect("the template union is normalized");
    let substitutions = HashMap::from([(
        "T".to_string(),
        Type::Named("CallSiteType".to_string(), Vec::new()),
    )]);
    let prospective =
        super::substitute_alias_type(&template, &substitutions, "consumer", &Default::default());
    let exact_key_bytes = prospective
        .canonical_key("consumer", &Default::default())
        .len();

    ExpansionBudget::with_key_limits(4, 4, exact_key_bytes)
        .check_substitution_key(
            &template,
            &substitutions,
            "consumer",
            &Default::default(),
            TEST_SPAN,
        )
        .expect("a replacement inside a template union is qualified at the call site");
}

#[test]
fn repeated_constructor_alias_expansions_share_aggregate_accounting() {
    use crate::ast::{TypeAliasDecl, TypeRef, TypeRefKind};

    let alias = super::AliasInfo {
        module_name: "demo".to_string(),
        decl: TypeAliasDecl {
            public: false,
            name: "Wrapped".to_string(),
            type_params: vec!["T".to_string()],
            type_param_bounds: Default::default(),
            target: TypeRef {
                kind: TypeRefKind::Named {
                    name: "Box".to_string(),
                    args: vec![TypeRef {
                        kind: TypeRefKind::Named {
                            name: "T".to_string(),
                            args: Vec::new(),
                        },
                        indirect: false,
                        span: TEST_SPAN,
                    }],
                },
                indirect: false,
                span: TEST_SPAN,
            },
            span: TEST_SPAN,
        },
        target: Type::Named("Box".to_string(), vec![Type::TypeParam("T".to_string())]),
        type_param_bounds: Default::default(),
    };
    let budget = ExpansionBudget::with_limits(4, 4, 3);
    let arguments = [Type::Unit];

    alias
        .constructor_callee(
            Some(&arguments),
            TEST_SPAN,
            "demo",
            &Default::default(),
            &budget,
        )
        .expect("the first two-node constructor expansion fits");
    let error = alias
        .constructor_callee(
            Some(&arguments),
            TEST_SPAN,
            "demo",
            &Default::default(),
            &budget,
        )
        .expect_err("the second expansion must observe the first aggregate charge");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        "type expansion exceeds aggregate node limit of 3"
    );
}

#[test]
fn checked_lowering_view_keeps_limits_without_recharging_semantic_aggregate() {
    let semantic = ExpansionBudget::with_limits(2, 3, 2);
    let two_nodes = Type::Tuple(vec![Type::Unit]);
    semantic
        .check_substitution(&two_nodes, &HashMap::new(), TEST_SPAN)
        .expect("semantic checking charges exactly two nodes");
    assert_eq!(semantic.aggregate_nodes(), 2);

    let lowering = semantic.for_checked_lowering();
    for _ in 0..3 {
        lowering
            .check_substitution(&two_nodes, &HashMap::new(), TEST_SPAN)
            .expect("already-checked lowering retains the per-result limit");
    }
    assert_eq!(semantic.aggregate_nodes(), 2);
    assert_eq!(lowering.aggregate_nodes(), 0);

    let three_nodes = Type::Tuple(vec![Type::Unit, Type::Unit]);
    let error = lowering
        .check_substitution(&three_nodes, &HashMap::new(), TEST_SPAN)
        .expect_err("checked lowering must still reject an oversized single result");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        "type expansion exceeds per-result node limit of 2"
    );
}

#[test]
fn canonical_key_shape_counts_closure_parameters_and_captures() {
    use super::callables::{
        ClosureCallKind, ClosureCapture, ClosureCaptureMode, FunctionParamContract,
    };
    use crate::ast::ReceiverKind;
    use std::collections::BTreeMap;

    let closure = Type::Closure {
        params: Box::new(vec![FunctionParamContract {
            keyword_only: true,
            name: "value".to_string(),
            ty: Type::named("int64"),
            passing: ReceiverKind::BorrowMut,
            has_default: true,
            default_erased: true,
        }]),
        return_type: Box::new(Type::Tuple(vec![Type::Unit, Type::named("str")])),
        captures: Box::new(vec![
            ClosureCapture {
                name: "shared".to_string(),
                ty: Type::Named("list".to_string(), vec![Type::named("int64")]),
                mode: ClosureCaptureMode::SharedView,
                span: TEST_SPAN,
            },
            ClosureCapture {
                name: "owned".to_string(),
                ty: Type::named("str"),
                mode: ClosureCaptureMode::Move,
                span: TEST_SPAN,
            },
            ClosureCapture {
                name: "copied".to_string(),
                ty: Type::named("int64"),
                mode: ClosureCaptureMode::Copy,
                span: TEST_SPAN,
            },
            ClosureCapture {
                name: "mutable".to_string(),
                ty: Type::named("int64"),
                mode: ClosureCaptureMode::MutableView,
                span: TEST_SPAN,
            },
        ]),
        call_kind: ClosureCallKind::MutableRepeatable,
    };
    for call_kind in [
        ClosureCallKind::Repeatable,
        ClosureCallKind::Consuming,
        ClosureCallKind::MutableRepeatable,
    ] {
        let Type::Closure {
            params,
            return_type,
            captures,
            ..
        } = closure.clone()
        else {
            unreachable!()
        };
        let variant = Type::Closure {
            params,
            return_type,
            captures,
            call_kind,
        };
        ExpansionBudget::default()
            .check_canonical_key(&variant, "main", &BTreeMap::new(), TEST_SPAN)
            .expect("a small closure key fits the default budget");
    }
}
