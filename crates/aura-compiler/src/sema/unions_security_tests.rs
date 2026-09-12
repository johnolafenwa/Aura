use super::*;

#[test]
fn nested_candidate_probes_share_nonrollback_compilation_work() {
    let type_names = TypeDefinitions::default();
    type_names.union_probe_work.set(65_534);
    let type_arities = BTreeMap::<String, usize>::new();
    let canonical_names = BTreeMap::<String, String>::new();
    let classes = BTreeMap::<String, ClassInfo>::new();
    let enums = BTreeMap::<String, EnumInfo>::new();
    let functions = BTreeMap::<String, FunctionInfo>::new();
    let constants = BTreeMap::<String, ConstantInfo>::new();
    let traits = BTreeMap::<String, TraitInfo>::new();
    let imported_modules = BTreeMap::<String, ModuleNamespace>::new();
    let module_registry = BTreeMap::<String, ModuleNamespace>::new();
    let checker = FunctionChecker::new(
        "test",
        &type_names,
        &type_arities,
        &canonical_names,
        &classes,
        &enums,
        &functions,
        &constants,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let span = crate::diag::Span::new(3, 7);
    let expression = Expr {
        kind: ExprKind::Tuple(vec![Expr {
            kind: ExprKind::Int(1),
            span,
        }]),
        span,
    };
    let inner = Type::normalize_union(
        vec![Type::named("int64"), Type::named("float64")],
        "test",
        &BTreeMap::new(),
    )
    .expect("two numeric members form a union");
    let outer = types::UnionType {
        members: vec![
            Type::Tuple(vec![inner]),
            Type::Tuple(vec![Type::normalize_union(
                vec![Type::named("int64"), Type::named("uint64")],
                "test",
                &BTreeMap::new(),
            )
            .expect("two integer members form a union")]),
        ],
        keys: vec!["first".to_string(), "second".to_string()],
        module_name: "test".to_string(),
    };

    let error = checker
        .type_union_injection(&expression, &mut HashMap::new(), &outer)
        .expect_err("the nested candidate must consume the remaining shared work");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        "union literal probe work exceeds compilation limit of 65536 candidates"
    );
    assert_eq!(error.span, Some(span));
    assert_eq!(type_names.union_probe_work.get(), 65_536);

    let repeated = checker
        .type_union_injection(&expression, &mut HashMap::new(), &outer)
        .expect_err("failed speculative probes must not roll back work");
    assert_eq!(repeated, error);
    assert_eq!(type_names.union_probe_work.get(), 65_536);
}
