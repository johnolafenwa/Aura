use super::*;
use crate::ast::{
    BreakStmt, ContinueStmt, FieldDecl, FormatPart, MapEntryExpr, PassStmt, ReturnStmt,
};
use crate::diag::Span;
use crate::integer::IntegerValue;
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn type_ref(name: &str) -> TypeRef {
    TypeRef {
        name: name.to_string(),
        args: Vec::new(),
        indirect: false,
        span: Span::new(1, 1),
    }
}

fn nested_type_ref(name: &str, args: Vec<TypeRef>) -> TypeRef {
    TypeRef {
        name: name.to_string(),
        args,
        indirect: false,
        span: Span::new(1, 1),
    }
}

fn expr(kind: ExprKind) -> Expr {
    Expr {
        kind,
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

fn named_arg(name: &str, value: Expr) -> Argument {
    Argument {
        name: Some(name.to_string()),
        span: value.span,
        value,
    }
}

fn function_decl(name: &str) -> FunctionDecl {
    FunctionDecl {
        public: true,
        name: name.to_string(),
        type_params: Vec::new(),
        type_param_bounds: BTreeMap::new(),
        receiver: None,
        params: Vec::new(),
        return_passing: ReceiverKind::Value,
        return_borrow_source: None,
        return_type: type_ref("None"),
        body: Vec::new(),
        span: Span::new(1, 1),
    }
}

fn trait_decl(name: &str, type_params: Vec<&str>) -> TraitDecl {
    TraitDecl {
        public: true,
        name: name.to_string(),
        type_params: type_params.into_iter().map(str::to_string).collect(),
        supertraits: Vec::new(),
        methods: Vec::new(),
        span: Span::new(1, 1),
    }
}

fn function_signature(params: Vec<Type>, return_type: Type) -> FunctionSignature {
    FunctionSignature {
        params,
        return_type,
        return_passing: ReceiverKind::Value,
        return_borrow_source: None,
    }
}

fn trait_info(name: &str, type_params: Vec<&str>) -> TraitInfo {
    TraitInfo {
        module_name: "<test>".to_string(),
        decl: trait_decl(name, type_params),
        supertraits: Vec::new(),
        methods: BTreeMap::new(),
    }
}

fn class_decl(name: &str, copy: bool, fields: Vec<FieldDecl>) -> ClassDecl {
    ClassDecl {
        public: true,
        copy,
        name: name.to_string(),
        type_params: Vec::new(),
        type_param_bounds: BTreeMap::new(),
        fields,
        methods: Vec::new(),
        span: Span::new(1, 1),
    }
}

fn field_decl(name: &str, ty: TypeRef, indirect: bool) -> FieldDecl {
    FieldDecl {
        public: true,
        name: name.to_string(),
        ty: TypeRef { indirect, ..ty },
        default: None,
        span: Span::new(1, 1),
    }
}

fn class_info(name: &str, copy: bool, field_specs: Vec<(&str, Type, bool)>) -> ClassInfo {
    let decl_fields = field_specs
        .iter()
        .map(|(field_name, ty, indirect)| {
            field_decl(
                field_name,
                match ty {
                    Type::Named(name, args) => TypeRef {
                        name: name.clone(),
                        args: args
                            .iter()
                            .map(|arg| match arg {
                                Type::Named(name, args) => TypeRef {
                                    name: name.clone(),
                                    args: args
                                        .iter()
                                        .map(|inner| match inner {
                                            Type::Named(name, args) => TypeRef {
                                                name: name.clone(),
                                                args: args
                                                    .iter()
                                                    .map(|_| type_ref("Unknown"))
                                                    .collect(),
                                                indirect: false,
                                                span: Span::new(1, 1),
                                            },
                                            Type::TypeParam(name) => type_ref(name),
                                            Type::Module(name) => type_ref(name),
                                            Type::Unit => type_ref("None"),
                                        })
                                        .collect(),
                                    indirect: false,
                                    span: Span::new(1, 1),
                                },
                                Type::TypeParam(name) => type_ref(name),
                                Type::Module(name) => type_ref(name),
                                Type::Unit => type_ref("None"),
                            })
                            .collect(),
                        indirect: false,
                        span: Span::new(1, 1),
                    },
                    Type::TypeParam(name) => type_ref(name),
                    Type::Module(name) => type_ref(name),
                    Type::Unit => type_ref("None"),
                },
                *indirect,
            )
        })
        .collect::<Vec<_>>();
    let fields = field_specs
        .into_iter()
        .map(|(field_name, ty, _)| {
            (
                field_name.to_string(),
                FieldInfo {
                    public: true,
                    ty,
                    span: Span::new(1, 1),
                },
            )
        })
        .collect();
    ClassInfo {
        module_name: "<test>".to_string(),
        decl: class_decl(name, copy, decl_fields),
        type_param_bounds: BTreeMap::new(),
        fields,
        methods: BTreeMap::new(),
    }
}

fn enum_info(name: &str, payload: Option<Type>) -> EnumInfo {
    let payload_fields = payload
        .as_ref()
        .map(|ty| crate::ast::EnumPayloadFieldDecl {
            name: None,
            ty: match ty {
                Type::Named(name, _) => type_ref(name),
                Type::TypeParam(name) => type_ref(name),
                Type::Module(name) => type_ref(name),
                Type::Unit => type_ref("None"),
            },
            span: Span::new(1, 1),
        })
        .into_iter()
        .collect::<Vec<_>>();
    let payload_infos = payload
        .into_iter()
        .map(|ty| EnumPayloadFieldInfo {
            name: None,
            ty,
            span: Span::new(1, 1),
        })
        .collect::<Vec<_>>();
    EnumInfo {
        module_name: "<test>".to_string(),
        decl: EnumDecl {
            public: true,
            name: name.to_string(),
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            variants: vec![crate::ast::EnumVariantDecl {
                name: "Value".to_string(),
                payloads: payload_fields,
                named_payloads: false,
                span: Span::new(1, 1),
            }],
            span: Span::new(1, 1),
        },
        type_param_bounds: BTreeMap::new(),
        variants: BTreeMap::from([(
            "Value".to_string(),
            EnumVariantInfo {
                payloads: payload_infos,
                named_payloads: false,
                span: Span::new(1, 1),
            },
        )]),
    }
}

fn namespace(path: &str) -> ModuleNamespace {
    ModuleNamespace {
        name: path.rsplit('.').next().unwrap_or(path).to_string(),
        path: path.to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::new(),
        all_enums: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    }
}

fn checker<'a>(
    module_name: &'a str,
    type_names: &'a BTreeMap<String, Span>,
    type_arities: &'a BTreeMap<String, usize>,
    classes: &'a BTreeMap<String, ClassInfo>,
    enums: &'a BTreeMap<String, EnumInfo>,
    functions: &'a BTreeMap<String, FunctionInfo>,
    traits: &'a BTreeMap<String, TraitInfo>,
    trait_impls: &'a [TraitImplInfo],
    imported_modules: &'a BTreeMap<String, ModuleNamespace>,
    module_registry: &'a BTreeMap<String, ModuleNamespace>,
) -> FunctionChecker<'a> {
    FunctionChecker::new(
        module_name,
        type_names,
        type_arities,
        classes,
        enums,
        functions,
        traits,
        trait_impls,
        imported_modules,
        module_registry,
    )
}

fn local_binding(
    ty: Type,
    assignable: bool,
    mutable_place: bool,
    passing: ReceiverKind,
    moved: bool,
    moved_fields: &[&str],
) -> LocalBinding {
    LocalBinding {
        ty,
        assignable,
        mutable_place,
        managed_resource: false,
        passing,
        borrow_origin: None,
        borrow_label: None,
        match_borrow_mut_place: None,
        stale_match_borrow_mut_place: None,
        moved,
        moved_fields: moved_fields
            .iter()
            .map(|field| (*field).to_string())
            .collect(),
        frozen_places: BTreeSet::new(),
    }
}

fn assign_stmt(
    target: AssignTarget,
    mutable: bool,
    annotation: Option<TypeRef>,
    op: Option<BinaryOp>,
    value: Expr,
) -> AssignStmt {
    AssignStmt {
        mutable,
        target,
        annotation,
        op,
        value,
        span: Span::new(1, 1),
    }
}

fn type_maps_from_program(program: &Program) -> (BTreeMap<String, Span>, BTreeMap<String, usize>) {
    let mut type_names = BTreeMap::new();
    let mut type_arities = BTreeMap::new();
    for (name, class_info) in &program.classes {
        type_names.insert(name.clone(), class_info.decl.span);
        type_arities.insert(name.clone(), class_info.decl.type_params.len());
    }
    for (name, enum_info) in &program.enums {
        type_names.insert(name.clone(), enum_info.decl.span);
        type_arities.insert(name.clone(), enum_info.decl.type_params.len());
    }
    for (name, trait_info) in &program.traits {
        type_names.insert(name.clone(), trait_info.decl.span);
        type_arities.insert(name.clone(), trait_info.decl.type_params.len());
    }
    (type_names, type_arities)
}

#[test]
fn checker_small_helper_utilities_cover_default_arg_and_recursive_type_paths() {
    let mut collected = BTreeSet::new();
    let type_names = BTreeMap::from([("Known".to_string(), Span::new(1, 1))]);
    collect_type_ref_type_params(&type_ref("T"), &type_names, &mut collected, true);
    collect_type_ref_type_params(
        &nested_type_ref("Vec", vec![type_ref("U")]),
        &type_names,
        &mut collected,
        false,
    );
    collect_type_ref_type_params(&type_ref("int32"), &type_names, &mut collected, true);
    collect_type_ref_type_params(&type_ref("Known"), &type_names, &mut collected, true);
    assert_eq!(
        collected,
        BTreeSet::from(["T".to_string(), "U".to_string()])
    );

    let param_names = vec!["left".to_string(), "right".to_string()];
    let binary_default = expr(ExprKind::Binary {
        op: BinaryOp::Add,
        left: Box::new(expr(ExprKind::Map(vec![MapEntryExpr {
            key: expr(ExprKind::String("key".to_string())),
            value: expr(ExprKind::Int(1)),
        }]))),
        right: Box::new(expr(ExprKind::Set(vec![expr(ExprKind::Index {
            object: Box::new(expr(ExprKind::Name("left".to_string()))),
            index: Box::new(expr(ExprKind::Int(0))),
        })]))),
    });
    assert_eq!(
        default_argument_references_param(&binary_default, &param_names),
        Some("left".to_string())
    );
    let fstring_default = expr(ExprKind::FString(vec![
        crate::ast::FormatPart::Literal("value=".to_string()),
        crate::ast::FormatPart::Expr(expr(ExprKind::Name("right".to_string()))),
    ]));
    assert_eq!(
        default_argument_references_param(&fstring_default, &param_names),
        Some("right".to_string())
    );
    assert_eq!(
        default_argument_references_param(&expr(ExprKind::Bool(true)), &param_names),
        None
    );

    assert_eq!(
        render_literal_pattern_key(&LiteralPatternKey::String("aurora".to_string())),
        "\"aurora\""
    );
    assert_eq!(
        render_literal_pattern_key(&LiteralPatternKey::Bool(false)),
        "false"
    );

    let string_ty = Type::named("String");
    let int_ty = Type::named("int32");
    let set_ty = Type::Named("Set".to_string(), vec![string_ty.clone()]);
    let map_ty = Type::Named("Map".to_string(), vec![string_ty.clone(), int_ty.clone()]);
    assert_eq!(set_element_type(&set_ty), Some(&string_ty));
    assert_eq!(map_key_value_types(&map_ty), Some((&string_ty, &int_ty)));

    assert!(Type::named("int32").is_copy());
    assert!(!Type::Named("Vec".to_string(), vec![Type::named("int32")]).is_copy());
    assert!(type_contains_named(
        &Type::Named(
            "Map".to_string(),
            vec![
                Type::named("String"),
                Type::Named("Vec".to_string(), vec![Type::named("Leaf")]),
            ],
        ),
        "Leaf"
    ));

    let classes = BTreeMap::from([
        (
            "Branch".to_string(),
            class_info("Branch", false, vec![("leaf", Type::named("Leaf"), false)]),
        ),
        (
            "Root".to_string(),
            class_info(
                "Root",
                false,
                vec![("branch", Type::named("Branch"), false)],
            ),
        ),
        (
            "RootIndirect".to_string(),
            class_info(
                "RootIndirect",
                false,
                vec![("branch", Type::named("Branch"), true)],
            ),
        ),
        (
            "Leaf".to_string(),
            class_info("Leaf", false, vec![("value", Type::named("int32"), false)]),
        ),
    ]);
    assert!(type_reaches_class_through_non_indirect_fields(
        &Type::named("Root"),
        "Leaf",
        &classes,
        &mut BTreeSet::new(),
    ));
    assert!(!type_reaches_class_through_non_indirect_fields(
        &Type::named("RootIndirect"),
        "Leaf",
        &classes,
        &mut BTreeSet::new(),
    ));
}

#[test]
fn checker_helper_paths_cover_explicit_type_args_and_pattern_unification_edges() {
    let program = crate::check_source(
            "class Box[T]:\n    value: T\n\ntrait Show:\n    def show(self) -> String\n\ndef main():\n    pass\n",
        )
        .expect("helper program should type check");
    let (type_names, type_arities) = type_maps_from_program(&program);
    let checker = FunctionChecker::new(
        &program.module_name,
        &type_names,
        &type_arities,
        &program.classes,
        &program.enums,
        &program.functions,
        &program.traits,
        &program.trait_impls,
        &program.imported_modules,
        &program.module_registry,
    );
    let span = Span::new(3, 4);

    let explicit = checker
        .explicit_type_substitutions(&["T".to_string()], &[type_ref("String")], span, "Box")
        .expect("single explicit type arg should lower");
    assert_eq!(
        explicit,
        HashMap::from([("T".to_string(), Type::named("String"))])
    );

    let explicit_arity = checker
        .explicit_type_substitutions(
            &["T".to_string()],
            &[type_ref("String"), type_ref("int32")],
            span,
            "Box",
        )
        .expect_err("mismatched explicit type arg counts should fail");
    assert!(explicit_arity
        .message
        .contains("Box expects 1 type argument"));

    checker
        .validate_integer_literal(7, &Type::named("String"), span)
        .expect("non-integer targets should skip integer literal bounds checks");
    checker
        .validate_integer_literal(127, &Type::named("int8"), span)
        .expect("in-range integers should validate");
    let overflow = checker
        .validate_integer_literal(128, &Type::named("int8"), span)
        .expect_err("overflowing integer literals should fail");
    assert!(overflow.message.contains("does not fit in `int8`"));

    let type_params = BTreeSet::from(["T".to_string()]);
    let mut module_substitutions = HashMap::new();
    assert!(type_pattern_matches(
        &Type::Module("helpers.math".to_string()),
        &Type::Module("helpers.math".to_string()),
        &type_params,
        &mut module_substitutions,
    ));
    assert!(type_pattern_matches(
        &Type::Unit,
        &Type::Unit,
        &type_params,
        &mut HashMap::new(),
    ));
    assert!(has_unresolved_type_params(&Type::Named(
        "Option".to_string(),
        vec![Type::TypeParam("T".to_string())],
    )));

    let unit_mismatch = unify_type_pattern(&Type::Unit, &Type::named("int32"), &mut HashMap::new())
        .expect_err("unit mismatches should report `None` diagnostics");
    assert!(unit_mismatch
        .message
        .contains("expected `None`, found `int32`"));

    let module_mismatch = unify_type_pattern(
        &Type::Module("helpers.math".to_string()),
        &Type::Module("helpers.other".to_string()),
        &mut HashMap::new(),
    )
    .expect_err("module mismatches should mention both paths");
    assert!(module_mismatch
        .message
        .contains("expected `module helpers.math`, found `module helpers.other`"));
}

#[test]
fn checker_expression_helper_paths_cover_collection_specialization_and_control_edges() {
    let program = crate::check_source(
            "class Counter:\n    value: int32\n\nclass Holder[T]:\n    value: T\n\nclass Flag:\n    value: bool\n\nenum Maybe[T]:\n    Value(T)\n    Empty\n\ndef work(value: int32) -> int32:\n    return value\n\ndef main():\n    pass\n",
        )
        .expect("helper program should type check");
    let (type_names, type_arities) = type_maps_from_program(&program);
    let mut not_decl = function_decl("not");
    not_decl.receiver = Some(ReceiverKind::Borrow);
    not_decl.return_type = type_ref("Out");
    let mut neg_decl = function_decl("neg");
    neg_decl.receiver = Some(ReceiverKind::Borrow);
    neg_decl.return_type = type_ref("Out");
    let traits = BTreeMap::from([
        (
            "Not".to_string(),
            TraitInfo {
                module_name: program.module_name.clone(),
                decl: trait_decl("Not", vec!["Out"]),
                supertraits: Vec::new(),
                methods: BTreeMap::from([(
                    "not".to_string(),
                    TraitMethodInfo {
                        decl: not_decl.clone(),
                        signature: function_signature(
                            Vec::new(),
                            Type::TypeParam("Out".to_string()),
                        ),
                        type_param_bounds: BTreeMap::new(),
                    },
                )]),
            },
        ),
        (
            "Neg".to_string(),
            TraitInfo {
                module_name: program.module_name.clone(),
                decl: trait_decl("Neg", vec!["Out"]),
                supertraits: Vec::new(),
                methods: BTreeMap::from([(
                    "neg".to_string(),
                    TraitMethodInfo {
                        decl: neg_decl.clone(),
                        signature: function_signature(
                            Vec::new(),
                            Type::TypeParam("Out".to_string()),
                        ),
                        type_param_bounds: BTreeMap::new(),
                    },
                )]),
            },
        ),
    ]);
    let trait_impls = vec![
        TraitImplInfo {
            module_name: program.module_name.clone(),
            decl: ImplDecl {
                type_params: Vec::new(),
                type_param_bounds: BTreeMap::new(),
                trait_name: "Not".to_string(),
                trait_args: vec![type_ref("Flag")],
                for_type: type_ref("Flag"),
                methods: vec![not_decl.clone()],
                span: Span::new(1, 1),
            },
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            trait_name: "Not".to_string(),
            trait_args: vec![Type::named("Flag")],
            for_type: Type::named("Flag"),
            methods: BTreeMap::from([(
                "not".to_string(),
                TraitImplMethodInfo {
                    decl: not_decl.clone(),
                    signature: function_signature(Vec::new(), Type::named("Flag")),
                    type_param_bounds: BTreeMap::new(),
                },
            )]),
        },
        TraitImplInfo {
            module_name: program.module_name.clone(),
            decl: ImplDecl {
                type_params: Vec::new(),
                type_param_bounds: BTreeMap::new(),
                trait_name: "Neg".to_string(),
                trait_args: vec![type_ref("Flag")],
                for_type: type_ref("Flag"),
                methods: vec![neg_decl.clone()],
                span: Span::new(1, 1),
            },
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            trait_name: "Neg".to_string(),
            trait_args: vec![Type::named("Flag")],
            for_type: Type::named("Flag"),
            methods: BTreeMap::from([(
                "neg".to_string(),
                TraitImplMethodInfo {
                    decl: neg_decl.clone(),
                    signature: function_signature(Vec::new(), Type::named("Flag")),
                    type_param_bounds: BTreeMap::new(),
                },
            )]),
        },
    ];
    let mut checker = FunctionChecker::new(
        &program.module_name,
        &type_names,
        &type_arities,
        &program.classes,
        &program.enums,
        &program.functions,
        &traits,
        &trait_impls,
        &program.imported_modules,
        &program.module_registry,
    );
    let vec_string = Type::Named("Vec".to_string(), vec![Type::named("String")]);
    let set_string = Type::Named("Set".to_string(), vec![Type::named("String")]);
    let map_string_string = Type::Named(
        "Map".to_string(),
        vec![Type::named("String"), Type::named("String")],
    );
    let option_int = Type::Named("Option".to_string(), vec![Type::named("int32")]);
    let result_int_string = Type::Named(
        "Result".to_string(),
        vec![Type::named("int32"), Type::named("String")],
    );
    let task_int = Type::Named("Task".to_string(), vec![Type::named("int32")]);
    let task_list_int = Type::Named("Vec".to_string(), vec![task_int.clone()]);
    let mut locals = HashMap::from([
        (
            "moved".to_string(),
            local_binding(
                Type::named("String"),
                true,
                true,
                ReceiverKind::Value,
                true,
                &[],
            ),
        ),
        (
            "partial".to_string(),
            local_binding(
                Type::named("Counter"),
                true,
                true,
                ReceiverKind::Value,
                false,
                &["value"],
            ),
        ),
        (
            "flag".to_string(),
            local_binding(
                Type::named("Flag"),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "words".to_string(),
            local_binding(
                vec_string.clone(),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "labels".to_string(),
            local_binding(
                set_string.clone(),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "scores".to_string(),
            local_binding(
                map_string_string.clone(),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "result_value".to_string(),
            local_binding(
                result_int_string.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "text".to_string(),
            local_binding(
                Type::named("String"),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "group".to_string(),
            local_binding(
                Type::named("TaskGroup"),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "task".to_string(),
            local_binding(
                task_int.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "tasks".to_string(),
            local_binding(
                task_list_int.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
    ]);

    assert_eq!(
        checker
            .type_of_expr_hint(
                &expr(ExprKind::Name("None".to_string())),
                &mut locals,
                Some(&option_int)
            )
            .expect("None should follow Option hints"),
        option_int
    );
    assert_eq!(
        checker
            .type_of_expr(&expr(ExprKind::Name("work".to_string())), &mut locals)
            .expect("functions should resolve to their return type"),
        Type::named("int32")
    );
    assert_eq!(
        checker
            .type_of_expr(&expr(ExprKind::Name("Counter".to_string())), &mut locals)
            .expect("classes should resolve to named types"),
        Type::named("Counter")
    );
    assert_eq!(
        checker
            .type_of_expr(&expr(ExprKind::Name("Maybe".to_string())), &mut locals)
            .expect("enums should resolve to named types"),
        Type::named("Maybe")
    );
    assert!(checker
        .type_of_expr(&expr(ExprKind::Name("moved".to_string())), &mut locals)
        .expect_err("moved bindings should fail")
        .message
        .contains("use of moved value"));
    assert!(checker
        .type_of_expr(&expr(ExprKind::Name("partial".to_string())), &mut locals)
        .expect_err("partially moved bindings should fail")
        .message
        .contains("partially moved"));
    assert!(checker
        .type_of_expr(&expr(ExprKind::Name("missing".to_string())), &mut locals)
        .expect_err("unknown names should fail")
        .message
        .contains("unknown name"));

    assert_eq!(
        checker
            .type_of_expr_hint(
                &expr(ExprKind::List(vec![
                    expr(ExprKind::String("left".to_string())),
                    expr(ExprKind::String("right".to_string())),
                ])),
                &mut locals,
                Some(&vec_string),
            )
            .expect("String lists should type check"),
        vec_string
    );
    assert!(checker
        .type_of_expr_hint(
            &expr(ExprKind::List(vec![
                expr(ExprKind::String("left".to_string())),
                expr(ExprKind::Int(1)),
            ])),
            &mut locals,
            Some(&Type::Named("Vec".to_string(), vec![Type::named("String")])),
        )
        .expect_err("heterogeneous lists should fail")
        .message
        .contains("list literal elements must all have type"));
    assert!(checker
        .type_of_expr(&expr(ExprKind::List(Vec::new())), &mut locals)
        .expect_err("empty lists require context")
        .message
        .contains("empty list literals require an expected `Vec[T]`"));

    assert_eq!(
        checker
            .type_of_expr_hint(
                &expr(ExprKind::Set(vec![
                    expr(ExprKind::String("left".to_string())),
                    expr(ExprKind::String("right".to_string())),
                ])),
                &mut locals,
                Some(&set_string),
            )
            .expect("String sets should type check"),
        set_string
    );
    assert!(checker
        .type_of_expr_hint(
            &expr(ExprKind::Set(vec![
                expr(ExprKind::String("left".to_string())),
                expr(ExprKind::Int(1)),
            ])),
            &mut locals,
            Some(&Type::Named("Set".to_string(), vec![Type::named("String")])),
        )
        .expect_err("heterogeneous sets should fail")
        .message
        .contains("set literal elements must all have type"));
    assert!(checker
        .type_of_expr(&expr(ExprKind::Set(Vec::new())), &mut locals)
        .expect_err("empty sets require context")
        .message
        .contains("empty set literals require an expected `Set[T]`"));

    assert_eq!(
        checker
            .type_of_expr_hint(
                &expr(ExprKind::Map(vec![MapEntryExpr {
                    key: expr(ExprKind::String("name".to_string())),
                    value: expr(ExprKind::String("aurora".to_string())),
                }])),
                &mut locals,
                Some(&map_string_string),
            )
            .expect("String maps should type check"),
        map_string_string
    );
    assert!(checker
        .type_of_expr_hint(
            &expr(ExprKind::Map(vec![MapEntryExpr {
                key: expr(ExprKind::Int(1)),
                value: expr(ExprKind::String("aurora".to_string())),
            }])),
            &mut locals,
            Some(&map_string_string),
        )
        .expect_err("mismatched map keys should fail")
        .message
        .contains("map literal keys must all have type"));
    assert!(checker
        .type_of_expr(&expr(ExprKind::Map(Vec::new())), &mut locals)
        .expect_err("empty maps require context")
        .message
        .contains("empty map literals require an expected `Map[K, V]`"));

    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Set".to_string()))),
                type_args: vec![type_ref("int32"), type_ref("String")],
            }),
            &mut locals,
        )
        .expect_err("Set arity mismatches should fail")
        .message
        .contains("type `Set` expects exactly one type argument"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Map".to_string()))),
                type_args: vec![type_ref("String")],
            }),
            &mut locals,
        )
        .expect_err("Map arity mismatches should fail")
        .message
        .contains("type `Map` expects exactly two type arguments"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Holder".to_string()))),
                type_args: vec![type_ref("int32"), type_ref("String")],
            }),
            &mut locals,
        )
        .expect_err("class arity mismatches should fail")
        .message
        .contains("class `Holder` expects 1 type argument"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Maybe".to_string()))),
                type_args: vec![type_ref("int32"), type_ref("String")],
            }),
            &mut locals,
        )
        .expect_err("enum arity mismatches should fail")
        .message
        .contains("enum `Maybe` expects 1 type argument"));
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("work".to_string()))),
                    type_args: vec![type_ref("int32")],
                }),
                &mut locals,
            )
            .expect("non-type specialization should fall back to the base expression"),
        Type::named("int32")
    );

    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Cast {
                expr: Box::new(expr(ExprKind::String("aurora".to_string()))),
                ty: type_ref("int32"),
            }),
            &mut locals,
        )
        .expect_err("casts should stay numeric-only")
        .message
        .contains("casts are only supported between numeric types"));

    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr(ExprKind::Bool(true))),
                }),
                &mut locals,
            )
            .expect("bool negation should type check"),
        Type::named("bool")
    );
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr(ExprKind::Name("flag".to_string()))),
                }),
                &mut locals,
            )
            .expect("trait-based unary not should resolve"),
        Type::named("Flag")
    );
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr(ExprKind::String("aurora".to_string()))),
            }),
            &mut locals,
        )
        .expect_err("unary not should reject non-bool non-trait types")
        .message
        .contains("`not` expects `bool`"));
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr(ExprKind::Name("flag".to_string()))),
                }),
                &mut locals,
            )
            .expect("trait-based unary neg should resolve"),
        Type::named("Flag")
    );
    assert_eq!(
        checker
            .type_of_expr_hint(
                &expr(ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr(ExprKind::Int(7))),
                }),
                &mut locals,
                Some(&Type::named("int64")),
            )
            .expect("negative integer literals should honor integer hints"),
        Type::named("int64")
    );
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expr(ExprKind::String("aurora".to_string()))),
            }),
            &mut locals,
        )
        .expect_err("unary neg should reject non-numeric non-trait types")
        .message
        .contains("unary `-` expects a numeric value"));

    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("group".to_string()))),
                    field: "start".to_string(),
                })),
                args: vec![arg(expr(ExprKind::Name("work".to_string())))],
            }),
            &mut locals,
        )
        .expect_err("TaskGroup.start should enforce callable arguments")
        .message
        .contains("missing required argument `value`"));
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Call {
                    callee: Box::new(expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("group".to_string()))),
                        field: "start".to_string(),
                    })),
                    args: vec![
                        arg(expr(ExprKind::Name("work".to_string()))),
                        arg(expr(ExprKind::Int(1))),
                    ],
                }),
                &mut locals,
            )
            .expect("task group start should type check"),
        Type::Named("Task".to_string(), vec![Type::named("int32")])
    );
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Call {
                    callee: Box::new(expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("group".to_string()))),
                        field: "start_soon".to_string(),
                    })),
                    args: vec![
                        arg(expr(ExprKind::Name("work".to_string()))),
                        arg(expr(ExprKind::Int(1))),
                    ],
                }),
                &mut locals,
            )
            .expect("start_soon should erase the Task handle"),
        Type::Unit
    );
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Call {
                    callee: Box::new(expr(ExprKind::Name("wait_any".to_string()))),
                    args: vec![arg(expr(ExprKind::Name("tasks".to_string())))],
                }),
                &mut locals,
            )
            .expect("wait_any should type check"),
        Type::Named("WaitAny".to_string(), vec![Type::named("int32")])
    );
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Call {
                    callee: Box::new(expr(ExprKind::Name("wait_all".to_string()))),
                    args: vec![arg(expr(ExprKind::Name("tasks".to_string())))],
                }),
                &mut locals,
            )
            .expect("wait_all should type check"),
        Type::Named("WaitAll".to_string(), vec![Type::named("int32")])
    );

    checker.current_return_type = Some(result_int_string.clone());
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                    "result_value".to_string(),
                ))))),
                &mut locals
            )
            .expect("matching Result try expressions should return the success type"),
        Type::named("int32")
    );
    checker.current_return_type = None;
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                "result_value".to_string(),
            ))))),
            &mut locals,
        )
        .expect_err("try outside functions should fail")
        .message
        .contains("only allowed inside a function body"));
    checker.current_return_type = Some(Type::named("int32"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                "result_value".to_string(),
            ))))),
            &mut locals,
        )
        .expect_err("non-Result returns should fail")
        .message
        .contains("enclosing function to return `Result`"));
    checker.current_return_type = Some(Type::Named(
        "Result".to_string(),
        vec![Type::named("int32"), Type::named("bool")],
    ));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                "result_value".to_string(),
            ))))),
            &mut locals,
        )
        .expect_err("mismatched error types should fail")
        .message
        .contains("does not match enclosing `Result` error type"));
    checker.current_return_type = Some(result_int_string.clone());
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Int(1))))),
            &mut locals,
        )
        .expect_err("try requires Result expressions")
        .message
        .contains("`try` requires a `Result[T, E]`"));

    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Binary {
                    op: BinaryOp::And,
                    left: Box::new(expr(ExprKind::Bool(true))),
                    right: Box::new(expr(ExprKind::Bool(false))),
                }),
                &mut locals,
            )
            .expect("boolean and should type check"),
        Type::named("bool")
    );
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Binary {
                op: BinaryOp::Add,
                left: Box::new(expr(ExprKind::Int(1))),
                right: Box::new(expr(ExprKind::Float(2.0))),
            }),
            &mut locals,
        )
        .expect_err("mixed numeric operands still reject after hinting")
        .message
        .contains("operands must match"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Binary {
                op: BinaryOp::Add,
                left: Box::new(expr(ExprKind::Float(1.0))),
                right: Box::new(expr(ExprKind::Int(2))),
            }),
            &mut locals,
        )
        .expect_err("mixed numeric operands still reject after hinting")
        .message
        .contains("operands must match"));

    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("partial".to_string()))),
                field: "value".to_string(),
            }),
            &mut locals,
        )
        .expect_err("moved fields should fail on member access")
        .message
        .contains("use of moved field"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Option".to_string()))),
                    type_args: vec![type_ref("int32")],
                })),
                field: "Some".to_string(),
            }),
            &mut locals,
        )
        .expect_err("payload variants still require construction")
        .message
        .contains("requires a payload"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Maybe".to_string()))),
                    type_args: vec![type_ref("int32"), type_ref("String")],
                })),
                field: "Empty".to_string(),
            }),
            &mut locals,
        )
        .expect_err("generic enum arity should be enforced on members too")
        .message
        .contains("enum `Maybe` expects 1 type argument"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Maybe".to_string()))),
                    type_args: vec![type_ref("int32")],
                })),
                field: "Value".to_string(),
            }),
            &mut locals,
        )
        .expect_err("payload variants should reject bare member access")
        .message
        .contains("requires a payload"));
    assert_eq!(
        checker
            .type_of_expr_hint(
                &expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("Maybe".to_string()))),
                    field: "Empty".to_string(),
                }),
                &mut locals,
                Some(&Type::Named(
                    "Maybe".to_string(),
                    vec![Type::named("int32")]
                )),
            )
            .expect("expected generic enum hints should flow into bare variants"),
        Type::Named("Maybe".to_string(), vec![Type::named("int32")])
    );
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("Maybe".to_string()))),
                field: "Empty".to_string(),
            }),
            &mut locals,
        )
        .expect_err("generic enum variants without hints should fail inference")
        .message
        .contains("cannot infer type parameter"));

    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Index {
                object: Box::new(expr(ExprKind::Name("words".to_string()))),
                index: Box::new(expr(ExprKind::String("zero".to_string()))),
            }),
            &mut locals,
        )
        .expect_err("vector indices should stay integer-only")
        .message
        .contains("vector indices must be integers"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Index {
                object: Box::new(expr(ExprKind::Name("scores".to_string()))),
                index: Box::new(expr(ExprKind::Int(0))),
            }),
            &mut locals,
        )
        .expect_err("map indices should honor key types")
        .message
        .contains("map keys must have type"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Index {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                index: Box::new(expr(ExprKind::Int(0))),
            }),
            &mut locals,
        )
        .expect_err("non-indexable values should fail")
        .message
        .contains("cannot index non-vector-or-map value"));

    let missing_specialized_variant = checker
        .type_of_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Maybe".to_string()))),
                    type_args: vec![type_ref("int32")],
                })),
                field: "Missing".to_string(),
            }),
            &mut HashMap::new(),
        )
        .expect_err("unknown specialized enum variants should fail");
    assert!(missing_specialized_variant
        .message
        .contains("enum `Maybe` has no variant `Missing`"));

    let specialized_payload_required = checker
        .type_of_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Maybe".to_string()))),
                    type_args: vec![type_ref("int32")],
                })),
                field: "Value".to_string(),
            }),
            &mut HashMap::new(),
        )
        .expect_err("payload variants should reject field-style access");
    assert!(specialized_payload_required
        .message
        .contains("variant `Value` of enum `Maybe` requires a payload"));

    let missing_variant = checker
        .type_of_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("Maybe".to_string()))),
                field: "Missing".to_string(),
            }),
            &mut HashMap::new(),
        )
        .expect_err("unknown enum variants should fail");
    assert!(missing_variant
        .message
        .contains("enum `Maybe` has no variant `Missing`"));

    let payload_required = checker
        .type_of_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("Maybe".to_string()))),
                field: "Value".to_string(),
            }),
            &mut HashMap::new(),
        )
        .expect_err("payload variants should reject field-style access");
    assert!(payload_required
        .message
        .contains("variant `Value` of enum `Maybe` requires a payload"));
}

#[test]
fn checker_assignment_helper_paths_cover_index_member_and_binding_edges() {
    let classes = BTreeMap::from([(
        "Counter".to_string(),
        class_info(
            "Counter",
            false,
            vec![
                ("value", Type::named("int32"), false),
                ("text", Type::named("String"), false),
            ],
        ),
    )]);
    let type_names = BTreeMap::from([("Counter".to_string(), Span::new(1, 1))]);
    let type_arities = BTreeMap::from([("Counter".to_string(), 0usize)]);
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let vec_int = Type::Named("Vec".to_string(), vec![Type::named("int32")]);
    let map_string_int = Type::Named(
        "Map".to_string(),
        vec![Type::named("String"), Type::named("int32")],
    );

    let mut locals = HashMap::from([(
        "nums".to_string(),
        local_binding(
            vec_int.clone(),
            true,
            false,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("nums".to_string()))),
                    index: Box::new(expr(ExprKind::Int(0))),
                },
                false,
                None,
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("immutable indexed places should fail")
        .message
        .contains("cannot assign through immutable place"));

    let mut locals = HashMap::from([(
        "nums".to_string(),
        local_binding(vec_int.clone(), true, true, ReceiverKind::Value, false, &[]),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("nums".to_string()))),
                    index: Box::new(expr(ExprKind::String("zero".to_string()))),
                },
                false,
                None,
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("vector indices should stay integer-only in assignments")
        .message
        .contains("vector indices must be integers"));

    let mut locals = HashMap::from([(
        "scores".to_string(),
        local_binding(
            map_string_int.clone(),
            true,
            true,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("scores".to_string()))),
                    index: Box::new(expr(ExprKind::Int(0))),
                },
                false,
                None,
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("map keys should honor their declared type")
        .message
        .contains("map keys must have type"));

    let mut locals = HashMap::from([(
        "text".to_string(),
        local_binding(
            Type::named("String"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("text".to_string()))),
                    index: Box::new(expr(ExprKind::Int(0))),
                },
                false,
                None,
                None,
                expr(ExprKind::String("x".to_string())),
            ),
            &mut locals,
        )
        .expect_err("non-indexable assignment targets should fail")
        .message
        .contains("cannot index non-vector-or-map value"));

    let mut locals = HashMap::from([(
        "nums".to_string(),
        local_binding(vec_int.clone(), true, true, ReceiverKind::Value, false, &[]),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("nums".to_string()))),
                    index: Box::new(expr(ExprKind::Int(0))),
                },
                false,
                None,
                None,
                expr(ExprKind::String("x".to_string())),
            ),
            &mut locals,
        )
        .expect_err("indexed assignment types should match")
        .message
        .contains("cannot assign value of type `String` to indexed element of type `int32`"));

    let mut locals = HashMap::from([(
        "counter".to_string(),
        local_binding(
            Type::named("Counter"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &["value"],
        ),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Member {
                    object: Box::new(expr(ExprKind::Name("counter".to_string()))),
                    field: "value".to_string(),
                },
                false,
                None,
                Some(BinaryOp::Add),
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("compound member writes should reject moved fields")
        .message
        .contains("cannot read moved field `value`"));

    let mut locals = HashMap::from([(
        "counter".to_string(),
        local_binding(
            Type::named("Counter"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &["value"],
        ),
    )]);
    checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Member {
                    object: Box::new(expr(ExprKind::Name("counter".to_string()))),
                    field: "value".to_string(),
                },
                false,
                None,
                None,
                expr(ExprKind::Int(7)),
            ),
            &mut locals,
        )
        .expect("plain member reassignment should clear moved field paths");
    assert!(locals
        .get("counter")
        .expect("counter binding should remain")
        .moved_fields
        .is_empty());

    let mut locals = HashMap::from([(
        "total".to_string(),
        local_binding(
            Type::named("int32"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Name("total".to_string()),
                true,
                None,
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("mut cannot redeclare existing bindings")
        .message
        .contains("`total` is already declared"));
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Name("total".to_string()),
                false,
                Some(type_ref("int32")),
                Some(BinaryOp::Add),
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("compound assignment annotations should fail")
        .message
        .contains("cannot include a type annotation"));

    let mut locals = HashMap::from([(
        "locked".to_string(),
        local_binding(
            Type::named("int32"),
            false,
            false,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Name("locked".to_string()),
                false,
                None,
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("immutable bindings should reject reassignment")
        .message
        .contains("cannot assign to immutable binding `locked`"));

    let mut locals = HashMap::from([(
        "moved".to_string(),
        local_binding(
            Type::named("int32"),
            true,
            true,
            ReceiverKind::Value,
            true,
            &[],
        ),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Name("moved".to_string()),
                false,
                None,
                Some(BinaryOp::Add),
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("compound assignment should reject moved bindings")
        .message
        .contains("cannot read moved value `moved`"));

    let mut locals = HashMap::from([(
        "typed".to_string(),
        local_binding(
            Type::named("int32"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Name("typed".to_string()),
                false,
                Some(type_ref("String")),
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("reassignment annotations should match the existing type")
        .message
        .contains("reassignment annotation for `typed` has type `String`, expected `int32`"));

    let mut locals = HashMap::from([(
        "typed".to_string(),
        local_binding(
            Type::named("int32"),
            true,
            true,
            ReceiverKind::Value,
            true,
            &["value"],
        ),
    )]);
    checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Name("typed".to_string()),
                false,
                None,
                None,
                expr(ExprKind::Int(3)),
            ),
            &mut locals,
        )
        .expect("plain reassignment should clear moved state");
    let typed = locals.get("typed").expect("typed binding should remain");
    assert!(!typed.moved);
    assert!(typed.moved_fields.is_empty());

    let mut locals = HashMap::from([(
        "pkg".to_string(),
        local_binding(
            Type::Module("pkg".to_string()),
            false,
            false,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Name("fresh".to_string()),
                false,
                None,
                Some(BinaryOp::Add),
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("compound assignment needs an existing binding")
        .message
        .contains("compound assignment requires an existing mutable binding `fresh`"));
    assert!(checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Name("fresh".to_string()),
                false,
                Some(type_ref("String")),
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("new bindings should honor their annotations")
        .message
        .contains("binding `fresh` has annotated type `String`, but value has type `int32`"));
    checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Name("fresh".to_string()),
                true,
                None,
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect("new mutable bindings should insert locals");
    let fresh = locals
        .get("fresh")
        .expect("fresh binding should be inserted");
    assert_eq!(fresh.ty, Type::named("int32"));
    assert!(fresh.assignable);
    assert!(fresh.mutable_place);

    let mut locals = HashMap::from([(
        "counter".to_string(),
        local_binding(
            Type::named("Counter"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    let member_type_mismatch = checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Member {
                    object: Box::new(expr(ExprKind::Name("counter".to_string()))),
                    field: "value".to_string(),
                },
                false,
                None,
                None,
                expr(ExprKind::String("oops".to_string())),
            ),
            &mut locals,
        )
        .expect_err("member assignments should enforce field types");
    assert!(member_type_mismatch.message.contains(
        "cannot assign value of type `String` to member `counter.value` of type `int32`"
    ));
}

#[test]
fn checker_call_surface_helpers_cover_builtin_constructors_and_builtin_calls() {
    let mut box_class = class_info(
        "Box",
        false,
        vec![("value", Type::TypeParam("T".to_string()), false)],
    );
    box_class.decl.type_params = vec!["T".to_string()];
    let mut phantom_class = class_info(
        "Phantom",
        false,
        vec![("value", Type::named("int32"), false)],
    );
    phantom_class.decl.type_params = vec!["T".to_string()];

    let classes = BTreeMap::from([
        ("Box".to_string(), box_class),
        ("Phantom".to_string(), phantom_class),
    ]);
    let type_names = BTreeMap::from([
        ("Box".to_string(), Span::new(1, 1)),
        ("Phantom".to_string(), Span::new(1, 1)),
    ]);
    let type_arities =
        BTreeMap::from([("Box".to_string(), 1usize), ("Phantom".to_string(), 1usize)]);
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let span = Span::new(1, 1);
    let channel_string = Type::Named("Queue".to_string(), vec![Type::named("String")]);
    let mut locals = HashMap::from([
        (
            "text".to_string(),
            local_binding(
                Type::named("String"),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "count".to_string(),
            local_binding(
                Type::named("int32"),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "tasks".to_string(),
            local_binding(
                Type::Named(
                    "Vec".to_string(),
                    vec![Type::Named("Task".to_string(), vec![Type::named("int32")])],
                ),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
    ]);

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Queue".to_string()))),
                type_args: vec![type_ref("String"), type_ref("int32")],
            }),
            &[],
            span,
            &mut locals,
            None,
        )
        .expect_err("Queue arity mismatches should fail")
        .message
        .contains("expects exactly one type argument"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Queue".to_string()))),
                type_args: vec![type_ref("String")],
            }),
            &[named_arg(
                "capacity",
                expr(ExprKind::String("large".to_string()))
            )],
            span,
            &mut locals,
            None,
        )
        .expect_err("Queue capacity should stay int32")
        .message
        .contains("field `capacity` expects `int32`"));
    assert_eq!(
        checker
            .type_of_call(
                &expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Queue".to_string()))),
                    type_args: vec![type_ref("String")],
                }),
                &[named_arg("capacity", expr(ExprKind::Int(4)))],
                span,
                &mut locals,
                None,
            )
            .expect("Queue[T](capacity=...) should type check"),
        channel_string
    );

    for (name, type_args, expected_fragment) in [
        (
            "Vec",
            vec![type_ref("int32"), type_ref("String")],
            "expects exactly one type argument",
        ),
        (
            "Set",
            vec![type_ref("int32"), type_ref("String")],
            "expects exactly one type argument",
        ),
    ] {
        assert!(checker
            .type_of_call(
                &expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name(name.to_string()))),
                    type_args,
                }),
                &[],
                span,
                &mut locals,
                None,
            )
            .expect_err("collection constructor arity mismatches should fail")
            .message
            .contains(expected_fragment));
    }
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Vec".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("Vec constructors stay argument-free")
        .message
        .contains("does not take constructor arguments"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Set".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("Set constructors stay argument-free")
        .message
        .contains("does not take constructor arguments"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Map".to_string()))),
                type_args: vec![type_ref("String")],
            }),
            &[],
            span,
            &mut locals,
            None,
        )
        .expect_err("Map arity mismatches should fail")
        .message
        .contains("expects exactly two type arguments"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Map".to_string()))),
                type_args: vec![type_ref("String"), type_ref("int32")],
            }),
            &[named_arg("value", expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("Map constructors stay argument-free")
        .message
        .contains("does not take constructor arguments"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("range".to_string())),
            &[arg(expr(ExprKind::String("bad".to_string())))],
            span,
            &mut locals,
            None,
        )
        .expect_err("range arguments stay int32-only")
        .message
        .contains("`range` arguments must have type `int32`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("wait_any".to_string())),
            &[arg(expr(ExprKind::Name("count".to_string())))],
            span,
            &mut locals,
            None,
        )
        .expect_err("wait_any() requires a Vec[Task[T]]")
        .message
        .contains("expects `Vec[Task[T]]`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("wait_all".to_string())),
            &[
                arg(expr(ExprKind::Name("tasks".to_string()))),
                named_arg("timeout", expr(ExprKind::Int(1))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("wait_all() timeout requires Duration")
        .message
        .contains("`wait_all(timeout=...)` expects `Duration`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("sleep".to_string())),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("sleep() requires Duration")
        .message
        .contains("expects a `Duration`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("abs".to_string())),
            &[arg(expr(ExprKind::String("bad".to_string())))],
            span,
            &mut locals,
            None,
        )
        .expect_err("abs() stays numeric-only")
        .message
        .contains("expects an integer or float value"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("min".to_string())),
            &[
                arg(expr(ExprKind::String("bad".to_string()))),
                arg(expr(ExprKind::Int(1)))
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("min() rejects non-numeric left operands")
        .message
        .contains("expects numeric arguments"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("max".to_string())),
            &[arg(expr(ExprKind::Int(1))), arg(expr(ExprKind::Float(1.0)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("min/max arguments must match")
        .message
        .contains("arguments must match"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("sqrt".to_string())),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("sqrt() is float-only")
        .message
        .contains("expects `float32` or `float64`"));
    for builtin in ["parse_int32", "parse_int64", "parse_float64"] {
        assert!(checker
            .type_of_call(
                &expr(ExprKind::Name(builtin.to_string())),
                &[arg(expr(ExprKind::Int(1)))],
                span,
                &mut locals,
                None,
            )
            .expect_err("parse helpers stay string-only")
            .message
            .contains("expects `String`"));
    }

    assert_eq!(
        checker
            .type_of_call(
                &expr(ExprKind::Name("Box".to_string())),
                &[arg(expr(ExprKind::Int(1)))],
                span,
                &mut locals,
                None,
            )
            .expect("positional class constructors should infer the field type"),
        Type::Named("Box".to_string(), vec![Type::named("int32")])
    );
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("Box".to_string())),
            &[named_arg("missing", expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("unknown constructor fields should fail")
        .message
        .contains("has no field named `missing`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("Box".to_string())),
            &[
                named_arg("value", expr(ExprKind::Int(1))),
                named_arg("value", expr(ExprKind::Int(2))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("duplicate constructor fields should fail")
        .message
        .contains("provided more than once"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("Box".to_string())),
            &[],
            span,
            &mut locals,
            Some(&Type::Named("Box".to_string(), vec![Type::named("int32")])),
        )
        .expect_err("required constructor fields should stay required")
        .message
        .contains("missing required field `value`"));
    assert_eq!(
        checker
            .type_of_call(
                &expr(ExprKind::Name("Box".to_string())),
                &[named_arg("value", expr(ExprKind::Int(1)))],
                span,
                &mut locals,
                None,
            )
            .expect("generic class constructors should infer type parameters from fields"),
        Type::Named("Box".to_string(), vec![Type::named("int32")])
    );
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Box".to_string()))),
                type_args: vec![type_ref("String")],
            }),
            &[named_arg("value", expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("explicit generic constructors should honor their field type")
        .message
        .contains("field `value` expects `String`, found `int32`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("Phantom".to_string())),
            &[named_arg("value", expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("unused generic parameters should still need inference")
        .message
        .contains("cannot infer type parameter `T` for class constructor `Phantom`"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("min".to_string())),
            &[
                arg(expr(ExprKind::Name("text".to_string()))),
                arg(expr(ExprKind::Name("text".to_string()))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("min should reject non-numeric matching arguments")
        .message
        .contains("`min` expects numeric arguments, found `String`"));
}

#[test]
fn checker_type_of_call_covers_associated_methods_generic_variants_and_private_fields() {
    let span = Span::new(1, 1);

    let mut widget = class_info(
        "Widget",
        false,
        vec![("value", Type::named("int32"), false)],
    );
    widget.methods.insert(
        "build".to_string(),
        MethodInfo {
            decl: {
                let mut decl = function_decl("build");
                decl.return_type = type_ref("int32");
                decl
            },
            signature: function_signature(Vec::new(), Type::named("int32")),
            type_param_bounds: BTreeMap::new(),
        },
    );
    widget.methods.insert(
        "touch".to_string(),
        MethodInfo {
            decl: {
                let mut decl = function_decl("touch");
                decl.receiver = Some(ReceiverKind::Borrow);
                decl.return_type = type_ref("int32");
                decl
            },
            signature: function_signature(Vec::new(), Type::named("int32")),
            type_param_bounds: BTreeMap::new(),
        },
    );

    let mut secret_box = class_info(
        "SecretBox",
        false,
        vec![("secret", Type::named("int32"), false)],
    );
    secret_box.module_name = "pkg.lib".to_string();
    secret_box.decl.fields[0].public = false;
    secret_box
        .fields
        .get_mut("secret")
        .expect("secret field should exist")
        .public = false;

    let mut status = enum_info("Status", Some(Type::TypeParam("T".to_string())));
    status.decl.type_params = vec!["T".to_string()];
    status.variants.insert(
        "Ready".to_string(),
        EnumVariantInfo {
            payloads: Vec::new(),
            named_payloads: false,
            span,
        },
    );
    status.decl.variants.push(crate::ast::EnumVariantDecl {
        name: "Ready".to_string(),
        payloads: Vec::new(),
        named_payloads: false,
        span,
    });

    let classes = BTreeMap::from([
        ("Widget".to_string(), widget),
        ("SecretBox".to_string(), secret_box),
    ]);
    let enums = BTreeMap::from([("Status".to_string(), status)]);
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let type_names = BTreeMap::from([
        ("Widget".to_string(), span),
        ("SecretBox".to_string(), span),
        ("Status".to_string(), span),
    ]);
    let type_arities = BTreeMap::from([
        ("Widget".to_string(), 0usize),
        ("SecretBox".to_string(), 0usize),
        ("Status".to_string(), 1usize),
    ]);
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let mut locals = HashMap::from([(
        "flag".to_string(),
        local_binding(
            Type::named("bool"),
            false,
            false,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);

    assert_eq!(
        checker
            .type_of_call(
                &expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("Widget".to_string()))),
                    field: "build".to_string(),
                }),
                &[],
                span,
                &mut locals,
                None,
            )
            .expect("associated class methods should type check"),
        Type::named("int32")
    );
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("Widget".to_string()))),
                field: "touch".to_string(),
            }),
            &[],
            span,
            &mut locals,
            None,
        )
        .expect_err("receiver methods should require instances when called from class names")
        .message
        .contains("requires an instance receiver"));

    let status_ready = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("Status".to_string()))),
        field: "Ready".to_string(),
    });
    let status_value = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("Status".to_string()))),
        field: "Value".to_string(),
    });
    let specialized_status_value = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Specialize {
            expr: Box::new(expr(ExprKind::Name("Status".to_string()))),
            type_args: vec![type_ref("int32")],
        })),
        field: "Value".to_string(),
    });

    assert_eq!(
        checker
            .type_of_call(
                &status_ready,
                &[],
                span,
                &mut locals,
                Some(&Type::Named(
                    "Status".to_string(),
                    vec![Type::named("int32")]
                )),
            )
            .expect("payload-free generic variants should follow expected types"),
        Type::Named("Status".to_string(), vec![Type::named("int32")])
    );
    assert!(checker
        .type_of_call(&status_ready, &[], span, &mut locals, None)
        .expect_err("generic payload-free variants should still need inference")
        .message
        .contains("cannot infer type parameter `T` for enum variant `Status.Ready`"));
    assert_eq!(
        checker
            .type_of_call(
                &status_value,
                &[named_arg("value", expr(ExprKind::Int(1)))],
                span,
                &mut locals,
                Some(&Type::Named(
                    "Status".to_string(),
                    vec![Type::named("int32")]
                )),
            )
            .expect("enum variant constructors should accept `value=` for single payload variants"),
        Type::Named("Status".to_string(), vec![Type::named("int32")])
    );
    assert!(checker
        .type_of_call(&specialized_status_value, &[], span, &mut locals, None,)
        .expect_err("payload variants require exactly one argument")
        .message
        .contains("payload"));
    assert!(checker
        .type_of_call(
            &specialized_status_value,
            &[arg(expr(ExprKind::Bool(true)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("payload variants should enforce specialized payload types")
        .message
        .contains("expects `int32`, found `bool`"));
    assert_eq!(
        checker
            .type_of_call(
                &specialized_status_value,
                &[arg(expr(ExprKind::Int(1)))],
                span,
                &mut locals,
                None,
            )
            .expect("specialized generic enum constructors should type check"),
        Type::Named("Status".to_string(), vec![Type::named("int32")])
    );

    let option_some = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Specialize {
            expr: Box::new(expr(ExprKind::Name("Option".to_string()))),
            type_args: vec![type_ref("int32")],
        })),
        field: "Some".to_string(),
    });
    let option_none = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Specialize {
            expr: Box::new(expr(ExprKind::Name("Option".to_string()))),
            type_args: vec![type_ref("int32")],
        })),
        field: "None".to_string(),
    });
    assert_eq!(
        checker
            .type_of_call(
                &option_some,
                &[named_arg("value", expr(ExprKind::Int(1)))],
                span,
                &mut locals,
                None,
            )
            .expect("builtin enum constructors should accept `value=`"),
        Type::Named("Option".to_string(), vec![Type::named("int32")])
    );
    assert!(checker
        .type_of_call(&option_some, &[], span, &mut locals, None)
        .expect_err("Option.Some still requires a payload")
        .message
        .contains("payload"));
    assert_eq!(
        checker
            .type_of_call(&option_none, &[], span, &mut locals, None)
            .expect("Option.None with explicit type args should type check"),
        Type::Named("Option".to_string(), vec![Type::named("int32")])
    );

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("SecretBox".to_string())),
            &[named_arg("secret", expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("external private constructor fields should be rejected")
        .message
        .contains("field `secret` is private on `SecretBox`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("SecretBox".to_string())),
            &[],
            span,
            &mut locals,
            None,
        )
        .expect_err("external private required fields should not be inferred")
        .message
        .contains("cannot initialize private field `secret` from another module"));
}

#[test]
fn checker_member_call_helpers_cover_string_map_set_and_channel_builtins() {
    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();
    let classes = BTreeMap::new();
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let span = Span::new(1, 1);
    let string_ty = Type::named("String");
    let vec_string = Type::Named("Vec".to_string(), vec![string_ty.clone()]);
    let map_string = Type::Named(
        "Map".to_string(),
        vec![string_ty.clone(), string_ty.clone()],
    );
    let set_string = Type::Named("Set".to_string(), vec![string_ty.clone()]);
    let channel_string = Type::Named("Queue".to_string(), vec![string_ty.clone()]);
    let mut locals = HashMap::from([
        (
            "text".to_string(),
            local_binding(
                string_ty.clone(),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "parts".to_string(),
            local_binding(
                vec_string.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "mapping".to_string(),
            local_binding(
                map_string.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "mutable_mapping".to_string(),
            local_binding(
                map_string.clone(),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "items".to_string(),
            local_binding(
                set_string.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "mutable_items".to_string(),
            local_binding(
                set_string.clone(),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "queue".to_string(),
            local_binding(
                channel_string.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
    ]);

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "join".to_string(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("join() expects Vec[String]")
        .message
        .contains("`join` expects `Vec[String]`"));
    assert_eq!(
        checker
            .type_of_call(
                &expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("text".to_string()))),
                    field: "join".to_string(),
                }),
                &[arg(expr(ExprKind::Name("parts".to_string())))],
                span,
                &mut locals,
                None,
            )
            .expect("join() should accept Vec[String]"),
        string_ty
    );
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "strip_prefix".to_string(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("strip_prefix() expects String")
        .message
        .contains("expects `String`"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "get".to_string(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("map.get() should enforce key type")
        .message
        .contains("`get` expects `String`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "set".to_string(),
            }),
            &[
                named_arg("key", expr(ExprKind::String("name".to_string()))),
                named_arg("value", expr(ExprKind::String("aurora".to_string()))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("map.set() requires a mutable receiver")
        .message
        .contains("requires a mutable receiver"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mutable_mapping".to_string()))),
                field: "set".to_string(),
            }),
            &[
                named_arg("key", expr(ExprKind::Int(1))),
                named_arg("value", expr(ExprKind::String("aurora".to_string()))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("map.set() should enforce key types")
        .message
        .contains("expects key type `String`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mutable_mapping".to_string()))),
                field: "set".to_string(),
            }),
            &[
                named_arg("key", expr(ExprKind::String("name".to_string()))),
                named_arg("value", expr(ExprKind::Int(1))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("map.set() should enforce value types")
        .message
        .contains("expects value type `String`"));
    assert_eq!(
        checker
            .type_of_call(
                &expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("mutable_mapping".to_string()))),
                    field: "set".to_string(),
                }),
                &[
                    named_arg("key", expr(ExprKind::Name("text".to_string()))),
                    named_arg("value", expr(ExprKind::String("aurora".to_string()))),
                ],
                span,
                &mut locals,
                None,
            )
            .expect("map.set() should type check on mutable maps"),
        Type::Named("Option".to_string(), vec![string_ty.clone()])
    );
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mutable_mapping".to_string()))),
                field: "remove".to_string(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("map.remove() should enforce key types")
        .message
        .contains("`remove` expects `String`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "contains_key".to_string(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("map.contains_key() should enforce key types")
        .message
        .contains("`contains_key` expects `String`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "clear".to_string(),
            }),
            &[],
            span,
            &mut locals,
            None,
        )
        .expect_err("map.clear() requires a mutable receiver")
        .message
        .contains("requires a mutable receiver"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mutable_mapping".to_string()))),
                field: "extend".to_string(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("map.extend() should enforce map types")
        .message
        .contains("`extend` expects `Map[String, String]`"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "contains".to_string(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("set.contains() should enforce element types")
        .message
        .contains("`contains` expects `String`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "insert".to_string(),
            }),
            &[arg(expr(ExprKind::String("aurora".to_string())))],
            span,
            &mut locals,
            None,
        )
        .expect_err("set.insert() requires a mutable receiver")
        .message
        .contains("requires a mutable receiver"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mutable_items".to_string()))),
                field: "insert".to_string(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("set.insert() should enforce element types")
        .message
        .contains("`insert` expects `String`"));
    assert_eq!(
        checker
            .type_of_call(
                &expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("mutable_items".to_string()))),
                    field: "remove".to_string(),
                }),
                &[arg(expr(ExprKind::String("aurora".to_string())))],
                span,
                &mut locals,
                None,
            )
            .expect("set.remove() should type check on mutable sets"),
        Type::named("bool")
    );

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "put".to_string(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("channel.put() should enforce payload types")
        .message
        .contains("`put` expects `String`"));
}

#[test]
fn checker_member_call_helpers_cover_successful_string_vec_map_and_runtime_surfaces() {
    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();
    let classes = BTreeMap::new();
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let span = Span::new(1, 1);
    let string_ty = Type::named("String");
    let int_ty = Type::named("int32");
    let float_ty = Type::named("float64");
    let bool_ty = Type::named("bool");
    let vec_int = Type::Named("Vec".to_string(), vec![int_ty.clone()]);
    let map_ty = Type::Named("Map".to_string(), vec![string_ty.clone(), int_ty.clone()]);
    let set_ty = Type::Named("Set".to_string(), vec![string_ty.clone()]);
    let channel_ty = Type::Named("Queue".to_string(), vec![string_ty.clone()]);
    let task_ty = Type::Named("Task".to_string(), vec![int_ty.clone()]);
    let mut locals = HashMap::from([
        (
            "number".to_string(),
            local_binding(
                int_ty.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "ratio".to_string(),
            local_binding(
                float_ty.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "flag".to_string(),
            local_binding(
                bool_ty.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "text".to_string(),
            local_binding(
                string_ty.clone(),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "values".to_string(),
            local_binding(vec_int.clone(), true, true, ReceiverKind::Value, false, &[]),
        ),
        (
            "immutable_values".to_string(),
            local_binding(
                vec_int.clone(),
                true,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "mapping".to_string(),
            local_binding(map_ty.clone(), true, true, ReceiverKind::Value, false, &[]),
        ),
        (
            "items".to_string(),
            local_binding(set_ty.clone(), true, true, ReceiverKind::Value, false, &[]),
        ),
        (
            "queue".to_string(),
            local_binding(
                channel_ty.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "task".to_string(),
            local_binding(
                task_ty.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "group".to_string(),
            local_binding(
                Type::named("TaskGroup"),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
    ]);

    for (callee, args, expected) in [
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("number".to_string()))),
                field: "to_string".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("ratio".to_string()))),
                field: "sqrt".to_string(),
            }),
            Vec::new(),
            float_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("flag".to_string()))),
                field: "to_string".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "split".to_string(),
            }),
            vec![arg(expr(ExprKind::String(",".to_string())))],
            Type::Named("Vec".to_string(), vec![string_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "replace".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("a".to_string()))),
                arg(expr(ExprKind::String("b".to_string()))),
            ],
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "to_lower".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "to_upper".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "strip_suffix".to_string(),
            }),
            vec![arg(expr(ExprKind::String("x".to_string())))],
            Type::Named("Option".to_string(), vec![string_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "trim".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "len".to_string(),
            }),
            Vec::new(),
            int_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "is_empty".to_string(),
            }),
            Vec::new(),
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "get".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(0)))],
            Type::Named("Option".to_string(), vec![int_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "set".to_string(),
            }),
            vec![
                named_arg("index", expr(ExprKind::Int(0))),
                named_arg("value", expr(ExprKind::Int(9))),
            ],
            Type::Named("Option".to_string(), vec![int_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "contains".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(9)))],
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "insert".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(0))), arg(expr(ExprKind::Int(9)))],
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "clear".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "keys".to_string(),
            }),
            Vec::new(),
            Type::Named("Vec".to_string(), vec![string_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "values".to_string(),
            }),
            Vec::new(),
            Type::Named("Vec".to_string(), vec![int_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "items".to_string(),
            }),
            Vec::new(),
            Type::Named(
                "Vec".to_string(),
                vec![Type::Named(
                    "MapEntry".to_string(),
                    vec![string_ty.clone(), int_ty.clone()],
                )],
            ),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "entries".to_string(),
            }),
            Vec::new(),
            Type::Named(
                "Vec".to_string(),
                vec![Type::Named(
                    "MapEntry".to_string(),
                    vec![string_ty.clone(), int_ty.clone()],
                )],
            ),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "contains_key".to_string(),
            }),
            vec![arg(expr(ExprKind::String("name".to_string())))],
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "clear".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "len".to_string(),
            }),
            Vec::new(),
            int_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "is_empty".to_string(),
            }),
            Vec::new(),
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "contains".to_string(),
            }),
            vec![arg(expr(ExprKind::String("name".to_string())))],
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "get".to_string(),
            }),
            Vec::new(),
            Type::Named("QueueReceive".to_string(), vec![string_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "put".to_string(),
            }),
            vec![arg(expr(ExprKind::String("ok".to_string())))],
            Type::Named(
                "Result".to_string(),
                vec![
                    Type::Unit,
                    Type::Named("SendError".to_string(), vec![string_ty.clone()]),
                ],
            ),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("task".to_string()))),
                field: "result".to_string(),
            }),
            Vec::new(),
            Type::Named("TaskResult".to_string(), vec![int_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("group".to_string()))),
                field: "cancel".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
    ] {
        assert_eq!(
            checker
                .type_of_call(&callee, &args, span, &mut locals, None)
                .expect("member call should type check"),
            expected
        );
    }

    for field in [
        "push", "pop", "set", "remove", "swap", "extend", "insert", "clear", "reverse",
    ] {
        let args = match field {
            "push" => vec![arg(expr(ExprKind::Int(1)))],
            "pop" | "clear" | "reverse" => Vec::new(),
            "set" => vec![
                named_arg("index", expr(ExprKind::Int(0))),
                named_arg("value", expr(ExprKind::Int(1))),
            ],
            "remove" => vec![arg(expr(ExprKind::Int(0)))],
            "swap" => vec![arg(expr(ExprKind::Int(0))), arg(expr(ExprKind::Int(1)))],
            "extend" => vec![arg(expr(ExprKind::Name("values".to_string())))],
            "insert" => vec![arg(expr(ExprKind::Int(0))), arg(expr(ExprKind::Int(1)))],
            _ => unreachable!(),
        };
        assert!(checker
            .type_of_call(
                &expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("immutable_values".to_string()))),
                    field: field.to_string(),
                }),
                &args,
                span,
                &mut locals,
                None,
            )
            .expect_err("mutable vector methods should reject immutable receivers")
            .message
            .contains("requires a mutable receiver"));
    }

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "set".to_string(),
            }),
            &[
                named_arg("index", expr(ExprKind::Int(0))),
                named_arg("value", expr(ExprKind::Bool(true))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("vec.set() should enforce element types")
        .message
        .contains("`set` expects `int32`"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "remove".to_string(),
            }),
            &[arg(expr(ExprKind::Bool(true)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("vec.remove() should enforce integer indices")
        .message
        .contains("vector indices must be integers"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "swap".to_string(),
            }),
            &[arg(expr(ExprKind::Int(0))), arg(expr(ExprKind::Bool(true)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("vec.swap() should enforce integer indices")
        .message
        .contains("vector indices must be integers"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "contains".to_string(),
            }),
            &[arg(expr(ExprKind::Bool(true)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("vec.contains() should enforce element types")
        .message
        .contains("`contains` expects `int32`"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "extend".to_string(),
            }),
            &[arg(expr(ExprKind::Bool(true)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("vec.extend() should enforce vector types")
        .message
        .contains("`extend` expects `Vec[int32]`"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "insert".to_string(),
            }),
            &[arg(expr(ExprKind::Int(0))), arg(expr(ExprKind::Bool(true)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("vec.insert() should enforce element types")
        .message
        .contains("`insert` expects `int32`"));
}

#[test]
fn copy_and_type_classifier_helpers_cover_builtin_and_user_types() {
    let classes = BTreeMap::from([
        (
            "Pair".to_string(),
            class_info(
                "Pair",
                true,
                vec![
                    ("left", Type::named("int32"), false),
                    ("right", Type::named("bool"), false),
                ],
            ),
        ),
        (
            "Owned".to_string(),
            class_info("Owned", false, vec![("name", Type::named("String"), false)]),
        ),
    ]);
    let enums = BTreeMap::from([
        (
            "MaybeInt".to_string(),
            enum_info("MaybeInt", Some(Type::named("int32"))),
        ),
        (
            "MaybeName".to_string(),
            enum_info("MaybeName", Some(Type::named("String"))),
        ),
    ]);

    assert!(is_builtin_copy_named_type("int32", &[]));
    assert!(!is_builtin_copy_named_type("String", &[]));
    assert!(type_is_copy_in_context(
        &Type::named("Pair"),
        &classes,
        &enums
    ));
    assert!(!type_is_copy_in_context(
        &Type::named("Owned"),
        &classes,
        &enums
    ));
    assert!(type_is_copy_in_context(
        &Type::Named("Option".to_string(), vec![Type::named("int32")]),
        &classes,
        &enums,
    ));
    assert!(!type_is_copy_in_context(
        &Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")]
        ),
        &classes,
        &enums,
    ));
    assert!(type_is_copy_in_context(
        &Type::named("MaybeInt"),
        &classes,
        &enums
    ));
    assert!(!type_is_copy_in_context(
        &Type::named("MaybeName"),
        &classes,
        &enums
    ));

    assert!(is_builtin_type("Vec"));
    assert!(is_integer_type(&Type::named("int64")));
    assert!(is_float_type(&Type::named("float64")));
    assert!(is_string_type(&Type::named("String")));
    assert!(is_numeric_type(&Type::named("float32")));
    assert!(Type::Unit.is_copy());
    assert!(!Type::Module("pkg.tools".to_string()).is_copy());
    assert!(!Type::TypeParam("T".to_string()).is_copy());
    assert!(Type::named("bool").is_copy());
    assert_eq!(Type::Unit.to_string(), "None");
    assert_eq!(
        Type::Module("pkg.tools".to_string()).to_string(),
        "module pkg.tools"
    );
    assert_eq!(
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        )
        .to_string(),
        "Map[String, int32]"
    );
}

#[test]
fn sema_helper_edges_cover_copy_defaults_literal_patterns_and_module_members() {
    let info = class_info(
        "Mixed",
        false,
        vec![
            (
                "module_field",
                Type::Module("pkg.helpers".to_string()),
                false,
            ),
            ("unit_field", Type::Unit, false),
            ("type_param_field", Type::TypeParam("T".to_string()), false),
        ],
    );
    assert_eq!(info.decl.fields[0].ty.name, "pkg.helpers");
    assert_eq!(info.decl.fields[1].ty.name, "None");
    assert_eq!(info.decl.fields[2].ty.name, "T");

    let classes = BTreeMap::from([(
        "Boxed".to_string(),
        class_info("Boxed", true, vec![("value", Type::named("int32"), false)]),
    )]);
    let enums = BTreeMap::from([
        ("Flag".to_string(), enum_info("Flag", None)),
        (
            "Payload".to_string(),
            enum_info("Payload", Some(Type::named("String"))),
        ),
    ]);
    assert!(!type_is_copy_in_context(
        &Type::Module("pkg".to_string()),
        &classes,
        &enums
    ));
    assert!(!type_is_copy_in_context(
        &Type::TypeParam("T".to_string()),
        &classes,
        &enums,
    ));
    assert!(type_is_copy_in_context(
        &Type::named("Flag"),
        &classes,
        &enums
    ));
    assert!(!type_is_copy_in_context(
        &Type::named("Payload"),
        &classes,
        &enums,
    ));
    assert_eq!(
        Type::Named(
            "Pair".to_string(),
            vec![Type::named("int32"), Type::named("bool")]
        )
        .to_string(),
        "Pair[int32, bool]"
    );

    let params = vec!["source".to_string(), "fallback".to_string()];
    let default_expr = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Name("build".to_string()))),
        args: vec![
            Argument {
                name: Some("value".to_string()),
                span: Span::new(1, 1),
                value: expr(ExprKind::Index {
                    object: Box::new(expr(ExprKind::Name("source".to_string()))),
                    index: Box::new(expr(ExprKind::Int(0))),
                }),
            },
            Argument {
                name: Some("fallback".to_string()),
                span: Span::new(1, 1),
                value: expr(ExprKind::Map(vec![MapEntryExpr {
                    key: expr(ExprKind::String("name".to_string())),
                    value: expr(ExprKind::FString(vec![
                        FormatPart::Literal("prefix".to_string()),
                        FormatPart::Expr(expr(ExprKind::Name("fallback".to_string()))),
                    ])),
                }])),
            },
        ],
    });
    assert_eq!(
        default_argument_references_param(&default_expr, &params),
        Some("source".to_string())
    );
    let wait_default = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Name("wait_all".to_string()))),
        args: vec![Argument {
            name: None,
            span: Span::new(1, 1),
            value: expr(ExprKind::Name("fallback".to_string())),
        }],
    });
    assert_eq!(
        default_argument_references_param(&wait_default, &params),
        Some("fallback".to_string())
    );

    let mut imported_modules = BTreeMap::new();
    let mut registry = BTreeMap::new();
    let mut root = namespace("pkg");
    root.modules
        .insert("helpers".to_string(), namespace("pkg.helpers"));
    root.functions.insert(
        "make".to_string(),
        FunctionInfo {
            module_name: "pkg".to_string(),
            decl: function_decl("make"),
            signature: function_signature(Vec::new(), Type::named("None")),
            type_param_bounds: BTreeMap::new(),
        },
    );
    root.classes.insert(
        "Widget".to_string(),
        class_info(
            "Widget",
            false,
            vec![("value", Type::named("int32"), false)],
        ),
    );
    root.enums
        .insert("Status".to_string(), enum_info("Status", None));
    imported_modules.insert("pkg".to_string(), root.clone());
    registry.insert("pkg".to_string(), root);

    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let checker = checker(
        "main",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &registry,
    );
    assert_eq!(
        checker.render_literal_pattern(&LiteralPattern {
            kind: LiteralPatternKind::String("aurora".to_string()),
            span: Span::new(1, 1),
        }),
        "\"aurora\""
    );
    assert_eq!(
        checker
            .resolve_member_type(&Type::Module("pkg".to_string()), "helpers", Span::new(1, 1))
            .expect("child module should resolve"),
        Type::Module("pkg.helpers".to_string())
    );
    assert!(checker
        .resolve_member_type(&Type::Module("pkg".to_string()), "make", Span::new(1, 1))
        .expect_err("functions require call syntax")
        .message
        .contains("must be called"));
    assert!(checker
        .resolve_member_type(&Type::Module("pkg".to_string()), "Widget", Span::new(1, 1))
        .expect_err("classes require construction")
        .message
        .contains("must be constructed"));
    assert_eq!(
        checker
            .resolve_member_type(&Type::Module("pkg".to_string()), "Status", Span::new(1, 1))
            .expect("module enums should resolve to enum types for qualified variant access"),
        Type::Named("Status".to_string(), Vec::new())
    );
    assert!(checker
        .resolve_member_type(&Type::Module("pkg".to_string()), "missing", Span::new(1, 1))
        .expect_err("missing member should fail")
        .message
        .contains("has no member"));
    assert!(checker
        .resolve_member_type(&Type::TypeParam("T".to_string()), "value", Span::new(1, 1))
        .expect_err("type params without traits cannot expose members")
        .message
        .contains("cannot access field"));
    assert!(checker
        .resolve_member_type(&Type::Unit, "value", Span::new(1, 1))
        .expect_err("unit has no members")
        .message
        .contains("cannot access field"));
}

#[test]
fn sema_render_and_builtin_enum_hint_helpers_cover_remaining_paths() {
    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();
    let classes = BTreeMap::new();
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let checker = checker(
        "<test>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );

    let place = expr(ExprKind::Name("item".to_string()));
    let member = expr(ExprKind::Member {
        object: Box::new(place.clone()),
        field: "value".to_string(),
    });
    let index = expr(ExprKind::Index {
        object: Box::new(member.clone()),
        index: Box::new(expr(ExprKind::Int(0))),
    });
    let grouped = expr(ExprKind::Group(Box::new(index.clone())));

    assert_eq!(checker.render_place_expr(&place), "item");
    assert_eq!(checker.render_place_expr(&member), "item.value");
    assert_eq!(checker.render_place_expr(&grouped), "item.value[..]");
    assert_eq!(checker.render_member_target(&place, "value"), "item.value");
    assert_eq!(checker.render_index_target(&member), "item.value[..]");
    assert_eq!(
        render_literal_pattern_key(&LiteralPatternKey::Int(IntegerValue::from_signed(-3))),
        "-3"
    );
    assert_eq!(
        render_literal_pattern_key(&LiteralPatternKey::Bool(true)),
        "true"
    );
    assert_eq!(
        render_literal_pattern_key(&LiteralPatternKey::String("aurora".to_string())),
        "\"aurora\""
    );

    assert_eq!(
        set_element_type(&Type::Named("Set".to_string(), vec![Type::named("String")])),
        Some(&Type::named("String"))
    );
    assert_eq!(set_element_type(&Type::named("String")), None);

    let option_name = expr(ExprKind::Name("Option".to_string()));
    let specialized_option = expr(ExprKind::Specialize {
        expr: Box::new(option_name.clone()),
        type_args: vec![type_ref("int32")],
    });
    let constructor_member = expr(ExprKind::Member {
        object: Box::new(specialized_option.clone()),
        field: "Some".to_string(),
    });
    let constructor_call = expr(ExprKind::Call {
        callee: Box::new(constructor_member.clone()),
        args: vec![arg(expr(ExprKind::Int(1)))],
    });

    assert!(checker.is_builtin_enum_constructor_expr(&option_name));
    assert!(checker.is_builtin_enum_constructor_expr(&specialized_option));
    assert!(!checker.is_builtin_enum_constructor_expr(&place));
    assert!(checker.expr_can_use_partial_expected_hint(&constructor_member));
    assert!(checker.expr_can_use_partial_expected_hint(&constructor_call));
    assert!(!checker.expr_can_use_partial_expected_hint(&place));
}

#[test]
fn checker_helper_paths_cover_imported_modules_type_args_and_binding_consumption() {
    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();
    let classes = BTreeMap::new();
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let trait_impls = Vec::new();
    let imported_modules = BTreeMap::from([("helpers".to_string(), namespace("pkg.helpers"))]);
    let mut current_namespace = namespace("pkg.current");
    current_namespace
        .imported_modules
        .insert("math".to_string(), namespace("pkg.current.math"));
    let module_registry = BTreeMap::from([("pkg.current".to_string(), current_namespace.clone())]);
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &trait_impls,
        &imported_modules,
        &module_registry,
    );
    let span = Span::new(3, 4);

    let mut top_level_locals = HashMap::new();
    checker.seed_imported_modules(&mut top_level_locals);
    assert_eq!(
        top_level_locals
            .get("helpers")
            .map(|binding| binding.ty.clone()),
        Some(Type::Module("pkg.helpers".to_string()))
    );

    let mut scoped_locals = HashMap::new();
    checker
        .with_module_name("pkg.current")
        .seed_imported_modules(&mut scoped_locals);
    assert_eq!(
        scoped_locals.get("math").map(|binding| binding.ty.clone()),
        Some(Type::Module("pkg.current.math".to_string()))
    );

    let plain_expr = expr(ExprKind::Name("Box".to_string()));
    let specialized_expr = expr(ExprKind::Specialize {
        expr: Box::new(plain_expr.clone()),
        type_args: vec![type_ref("int32")],
    });
    let (peeled, explicit_args) = checker.peel_specialization(&specialized_expr);
    assert!(matches!(peeled.kind, ExprKind::Name(ref name) if name == "Box"));
    assert_eq!(explicit_args.map(|args| args.len()), Some(1));
    let (plain_peeled, plain_args) = checker.peel_specialization(&plain_expr);
    assert!(matches!(plain_peeled.kind, ExprKind::Name(ref name) if name == "Box"));
    assert!(plain_args.is_none());

    let substitutions = checker
        .explicit_type_substitutions(
            &["T".to_string(), "U".to_string()],
            &[type_ref("int32"), type_ref("String")],
            span,
            "Pair",
        )
        .expect("matching explicit type arguments should lower");
    assert_eq!(substitutions.get("T"), Some(&Type::named("int32")));
    assert_eq!(substitutions.get("U"), Some(&Type::named("String")));
    let mismatch = checker
        .explicit_type_substitutions(
            &["T".to_string(), "U".to_string()],
            &[type_ref("int32")],
            span,
            "Pair",
        )
        .expect_err("arity mismatch should fail");
    assert!(mismatch
        .message
        .contains("Pair expects 2 type arguments, found 1"));

    checker
        .validate_negative_integer_literal(7, &Type::named("String"), span)
        .expect("non-integer targets should be ignored");
    let neg_overflow = checker
        .validate_negative_integer_literal(u128::MAX, &Type::named("int128"), span)
        .expect_err("unrepresentable negative literals should fail");
    assert!(neg_overflow.message.contains("does not fit in `int128`"));
    let int8_overflow = checker
        .validate_negative_integer_literal(129, &Type::named("int8"), span)
        .expect_err("out-of-bounds negative literals should fail");
    assert!(int8_overflow.message.contains("does not fit in `int8`"));

    let mut locals = HashMap::from([(
        "count".to_string(),
        LocalBinding {
            ty: Type::named("int32"),
            assignable: true,
            mutable_place: true,
            managed_resource: false,
            passing: ReceiverKind::Value,
            borrow_origin: None,
            borrow_label: None,
            match_borrow_mut_place: None,
            stale_match_borrow_mut_place: None,
            moved: false,
            moved_fields: BTreeSet::new(),
            frozen_places: BTreeSet::new(),
        },
    )]);
    checker
        .consume_binding("count", span, &mut locals)
        .expect("copy types should not be consumed");
    assert!(!locals["count"].moved);

    let unknown = checker
        .consume_binding("missing", span, &mut HashMap::new())
        .expect_err("unknown names should fail");
    assert!(unknown.message.contains("unknown name `missing`"));

    let mut borrowed_locals = HashMap::from([(
        "borrowed".to_string(),
        LocalBinding {
            ty: Type::named("String"),
            assignable: true,
            mutable_place: false,
            managed_resource: false,
            passing: ReceiverKind::Borrow,
            borrow_origin: Some("borrowed".to_string()),
            borrow_label: None,
            match_borrow_mut_place: None,
            stale_match_borrow_mut_place: None,
            moved: false,
            moved_fields: BTreeSet::new(),
            frozen_places: BTreeSet::new(),
        },
    )]);
    let borrowed_error = checker
        .consume_binding("borrowed", span, &mut borrowed_locals)
        .expect_err("borrowed values cannot be moved");
    assert!(borrowed_error
        .message
        .contains("cannot move borrowed value `borrowed`"));

    let mut moved_locals = HashMap::from([(
        "text".to_string(),
        LocalBinding {
            ty: Type::named("String"),
            assignable: true,
            mutable_place: true,
            managed_resource: false,
            passing: ReceiverKind::Value,
            borrow_origin: None,
            borrow_label: None,
            match_borrow_mut_place: None,
            stale_match_borrow_mut_place: None,
            moved: true,
            moved_fields: BTreeSet::new(),
            frozen_places: BTreeSet::new(),
        },
    )]);
    let moved_error = checker
        .consume_binding("text", span, &mut moved_locals)
        .expect_err("moved values should be rejected");
    assert!(moved_error.message.contains("use of moved value `text`"));
}

#[test]
fn vec_literal_consumes_non_copy_elements_only_once() {
    crate::check_source(
        "class Box:\n    value: int32\n\ndef main() -> int32:\n    b = Box(value=1)\n    values: Vec[Box] = [b]\n    return 0\n",
    )
    .expect("Vec literals should accept the first move of a non-copy element");
}

#[test]
fn namespace_and_type_parameter_helpers_cover_registration_lookup_and_collection() {
    let mut child = namespace("pkg.inner");
    child.classes.insert(
        "Thing".to_string(),
        class_info("Thing", false, vec![("value", Type::named("int32"), false)]),
    );
    let mut imported = namespace("pkg.external");
    imported
        .traits
        .insert("Named".to_string(), trait_info("Named", vec!["T"]));
    let mut root = namespace("pkg");
    root.enums.insert(
        "State".to_string(),
        enum_info("State", Some(Type::named("bool"))),
    );
    root.modules.insert("inner".to_string(), child.clone());
    root.imported_modules
        .insert("external".to_string(), imported.clone());

    let mut type_names = BTreeMap::new();
    let mut type_arities = BTreeMap::new();
    register_module_namespace_types(&root, &mut type_names, &mut type_arities);

    assert!(type_names.contains_key("pkg.State"));
    assert!(type_names.contains_key("pkg.inner.Thing"));
    assert!(type_names.contains_key("pkg.external.Named"));
    assert_eq!(type_arities.get("pkg.external.Named"), Some(&1));
    assert_eq!(
        find_namespace_in_modules(&BTreeMap::from([("pkg".to_string(), root)]), "pkg.inner")
            .map(|found| found.path.clone()),
        Some("pkg.inner".to_string())
    );

    validate_type_params(
        &["T".to_string(), "U".to_string()],
        Span::new(1, 1),
        "class Box",
    )
    .expect("unique type params should validate");
    let duplicate = validate_type_params(
        &["T".to_string(), "T".to_string()],
        Span::new(1, 1),
        "class Box",
    )
    .expect_err("duplicate type params should fail");
    assert!(duplicate.message.contains("duplicate type parameter `T`"));

    let parent = type_param_scope(&["T".to_string()]);
    let merged = merged_type_param_scope(&parent, &["U".to_string()]);
    assert!(merged.contains_key("T"));
    assert!(merged.contains_key("U"));

    let mut collected = BTreeSet::new();
    collect_type_ref_type_params(
        &nested_type_ref("Vec", vec![nested_type_ref("Boxed", vec![type_ref("T")])]),
        &BTreeMap::from([("Vec".to_string(), Span::new(1, 1))]),
        &mut collected,
        false,
    );
    assert!(collected.contains("T"));
}

#[test]
fn default_argument_and_trait_bound_helpers_cover_nested_expression_cases() {
    let param_names = vec!["source".to_string(), "fallback".to_string()];
    let default_expr = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::FString(vec![
                FormatPart::Literal("value=".to_string()),
                FormatPart::Expr(expr(ExprKind::Name("fallback".to_string()))),
            ]))),
            field: "replace".to_string(),
        })),
        args: vec![Argument {
            name: Some("value".to_string()),
            value: expr(ExprKind::Map(vec![MapEntryExpr {
                key: expr(ExprKind::String("x".to_string())),
                value: expr(ExprKind::Name("source".to_string())),
            }])),
            span: Span::new(1, 1),
        }],
    });
    assert_eq!(
        default_argument_references_param(&default_expr, &param_names),
        Some("fallback".to_string())
    );

    let traits = BTreeMap::from([
        ("Named".to_string(), trait_info("Named", vec![])),
        ("Mapper".to_string(), trait_info("Mapper", vec!["T"])),
    ]);
    let type_names = BTreeMap::from([
        ("String".to_string(), Span::new(1, 1)),
        ("int32".to_string(), Span::new(1, 1)),
    ]);
    let type_arities = BTreeMap::from([("String".to_string(), 0), ("int32".to_string(), 0)]);
    let lowered = lower_trait_bounds(
        &BTreeMap::from([(
            "T".to_string(),
            vec![
                type_ref("Named"),
                nested_type_ref("Mapper", vec![type_ref("String")]),
            ],
        )]),
        &traits,
        &type_names,
        &type_arities,
        &type_param_scope(&["T".to_string()]),
    )
    .expect("trait bounds should lower");
    assert_eq!(
        lowered.get("T"),
        Some(&vec![
            TraitBound {
                trait_name: "Named".to_string(),
                trait_args: Vec::new(),
            },
            TraitBound {
                trait_name: "Mapper".to_string(),
                trait_args: vec![Type::named("String")],
            },
        ])
    );
    let merged = merge_trait_bounds(
        &lowered,
        &BTreeMap::from([(
            "T".to_string(),
            vec![TraitBound {
                trait_name: "Extra".to_string(),
                trait_args: Vec::new(),
            }],
        )]),
    );
    assert_eq!(merged.get("T").map(Vec::len), Some(3));
}

#[test]
fn type_pattern_and_collection_helpers_cover_recursive_and_error_paths() {
    let classes = BTreeMap::from([
        (
            "Leaf".to_string(),
            class_info("Leaf", false, vec![("value", Type::named("int32"), false)]),
        ),
        (
            "Node".to_string(),
            class_info("Node", false, vec![("next", Type::named("Leaf"), false)]),
        ),
        (
            "Tree".to_string(),
            class_info("Tree", false, vec![("child", Type::named("Tree"), true)]),
        ),
    ]);

    assert!(type_contains_named(
        &Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("Leaf")]
        ),
        "Leaf"
    ));
    assert!(type_reaches_class_through_non_indirect_fields(
        &Type::named("Node"),
        "Leaf",
        &classes,
        &mut BTreeSet::new()
    ));
    assert!(!type_reaches_class_through_non_indirect_fields(
        &Type::named("Tree"),
        "Leaf",
        &classes,
        &mut BTreeSet::new()
    ));

    let substitutions = HashMap::from([("T".to_string(), Type::named("String"))]);
    assert_eq!(
        substitute_type(
            &Type::Named("Vec".to_string(), vec![Type::TypeParam("T".to_string())]),
            &substitutions,
        ),
        Type::Named("Vec".to_string(), vec![Type::named("String")])
    );
    let substituted_bounds = substitute_trait_bounds(
        &BTreeMap::from([(
            "T".to_string(),
            vec![TraitBound {
                trait_name: "Mapper".to_string(),
                trait_args: vec![Type::TypeParam("T".to_string())],
            }],
        )]),
        &substitutions,
    );
    assert_eq!(
        substituted_bounds.get("T"),
        Some(&vec![TraitBound {
            trait_name: "Mapper".to_string(),
            trait_args: vec![Type::named("String")],
        }])
    );

    let mut collected = BTreeSet::new();
    collect_type_params_from_type(
        &Type::Named(
            "Result".to_string(),
            vec![
                Type::TypeParam("T".to_string()),
                Type::TypeParam("E".to_string()),
            ],
        ),
        &mut collected,
    );
    assert_eq!(
        collected,
        BTreeSet::from(["E".to_string(), "T".to_string()])
    );

    let mut substitutions = HashMap::new();
    assert!(type_pattern_matches(
        &Type::Named("Vec".to_string(), vec![Type::TypeParam("T".to_string())]),
        &Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        &BTreeSet::from(["T".to_string()]),
        &mut substitutions,
    ));
    assert_eq!(substitutions.get("T"), Some(&Type::named("int32")));
    assert!(has_unresolved_type_params(&Type::TypeParam(
        "T".to_string()
    )));
    assert_eq!(
        substitutions_from_decl_type_args(
            &["K".to_string(), "V".to_string()],
            &[Type::named("String"), Type::named("int32")],
        ),
        HashMap::from([
            ("K".to_string(), Type::named("String")),
            ("V".to_string(), Type::named("int32")),
        ])
    );

    let mut unify = HashMap::new();
    unify_type_pattern(
        &Type::Named(
            "Map".to_string(),
            vec![
                Type::TypeParam("K".to_string()),
                Type::TypeParam("V".to_string()),
            ],
        ),
        &Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        &mut unify,
    )
    .expect("type pattern should unify");
    assert_eq!(unify.get("K"), Some(&Type::named("String")));
    assert_eq!(unify.get("V"), Some(&Type::named("int32")));
    let conflict = unify_type_pattern(
        &Type::TypeParam("T".to_string()),
        &Type::named("String"),
        &mut HashMap::from([("T".to_string(), Type::named("int32"))]),
    )
    .expect_err("conflicting substitutions should fail");
    assert!(conflict.message.contains("conflicting inferred types"));

    assert_eq!(
        render_literal_pattern_key(&LiteralPatternKey::Int(IntegerValue::from_signed(7))),
        "7"
    );
    assert_eq!(
        render_literal_pattern_key(&LiteralPatternKey::Bool(true)),
        "true"
    );
    assert_eq!(
        render_literal_pattern_key(&LiteralPatternKey::String("aurora".to_string())),
        "\"aurora\""
    );
    assert_eq!(
        vec_element_type(&Type::Named("Vec".to_string(), vec![Type::named("String")])),
        Some(&Type::named("String"))
    );
    assert_eq!(
        set_element_type(&Type::Named("Set".to_string(), vec![Type::named("bool")])),
        Some(&Type::named("bool"))
    );
    assert_eq!(
        map_key_value_types(&Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        )),
        Some((&Type::named("String"), &Type::named("int32")))
    );
    assert!(borrow_places_overlap("counter.value", "counter"));
    assert!(borrow_places_overlap("counter", "counter.value"));
    assert!(!borrow_places_overlap("left.value", "right.value"));
}

#[test]
fn operator_trait_helpers_map_supported_operators() {
    assert_eq!(unary_operator_trait(UnaryOp::Neg), Some(("Neg", "neg")));
    assert_eq!(unary_operator_trait(UnaryOp::Not), Some(("Not", "not")));
    assert_eq!(binary_operator_trait(BinaryOp::Add), Some(("Add", "add")));
    assert_eq!(binary_operator_trait(BinaryOp::Sub), Some(("Sub", "sub")));
    assert_eq!(binary_operator_trait(BinaryOp::Mul), Some(("Mul", "mul")));
    assert_eq!(binary_operator_trait(BinaryOp::Div), Some(("Div", "div")));
    assert_eq!(binary_operator_trait(BinaryOp::Mod), Some(("Mod", "mod")));
    assert_eq!(binary_operator_trait(BinaryOp::Less), Some(("Ord", "lt")));
    assert_eq!(binary_operator_trait(BinaryOp::LessEq), Some(("Ord", "le")));
    assert_eq!(
        binary_operator_trait(BinaryOp::Greater),
        Some(("Ord", "gt"))
    );
    assert_eq!(
        binary_operator_trait(BinaryOp::GreaterEq),
        Some(("Ord", "ge"))
    );
    assert_eq!(binary_operator_trait(BinaryOp::Eq), None);
    assert_eq!(
        TraitBound {
            trait_name: "Mapper".to_string(),
            trait_args: vec![Type::named("String")],
        }
        .to_string(),
        "Mapper[String]"
    );
}

#[test]
fn reserved_type_names_are_rejected() {
    let error = reject_reserved_type_name("Task", Span::new(1, 1))
        .expect_err("reserved built-in type names should fail");
    assert!(error.message.contains("reserved built-in type name"));

    for source in [
        "class Task:\n    value: int32\n\ndef main():\n    pass\n",
        "enum Result:\n    Ok\n\ndef main():\n    pass\n",
        "trait Queue:\n    def label(self) -> String\n\ndef main():\n    pass\n",
    ] {
        let error = crate::check_source(source).expect_err("reserved built-in names should fail");
        assert!(error.message.contains("reserved built-in type name"));
    }
}

#[test]
fn check_reports_duplicate_type_params_across_top_level_item_kinds() {
    let cases = [
            (
                "trait Box[T, T]:\n    def show(self) -> String\n\ndef main():\n    pass\n",
                "duplicate type parameter `T` on trait",
            ),
            (
                "trait Box:\n    def show[T, T](self) -> String\n\ndef main():\n    pass\n",
                "duplicate type parameter `T` on trait method",
            ),
            (
                "enum Maybe[T, T]:\n    Some(T)\n\ndef main():\n    pass\n",
                "duplicate type parameter `T` on enum",
            ),
            (
                "class Box[T, T]:\n    value: T\n\ndef main():\n    pass\n",
                "duplicate type parameter `T` on class",
            ),
            (
                "class Box:\n    def show[T, T](self) -> String:\n        return \"x\"\n\ndef main():\n    pass\n",
                "duplicate type parameter `T` on method",
            ),
            (
                "def identity[T, T](value: T) -> T:\n    return value\n\ndef main():\n    pass\n",
                "duplicate type parameter `T` on function",
            ),
            (
                "trait Show:\n    def show(self) -> String\n\nclass Box:\n    value: int32\n\nimpl[T, T] Show for Box:\n    def show(self) -> String:\n        return \"x\"\n\ndef main():\n    pass\n",
                "duplicate type parameter `T` on impl",
            ),
            (
                "trait Mapper[T]:\n    def map(self, value: T) -> T\n\nclass Box[T]:\n    value: T\n\nimpl[T] Mapper[T] for Box[T]:\n    def map[U, U](self, value: T) -> T:\n        return value\n\ndef main():\n    pass\n",
                "duplicate type parameter `U` on impl method",
            ),
        ];

    for (source, expected) in cases {
        let error = crate::check_source(source).expect_err("duplicate type params should fail");
        assert!(
            error.message.contains(expected),
            "expected `{expected}` in `{}`",
            error.message
        );
    }
}

#[test]
fn lower_type_covers_builtin_generic_and_error_paths() {
    let type_names = BTreeMap::from([
        ("Box".to_string(), Span::new(1, 1)),
        ("pkg.Counter".to_string(), Span::new(1, 1)),
    ]);
    let type_arities = BTreeMap::from([
        ("Box".to_string(), 1usize),
        ("pkg.Counter".to_string(), 0usize),
    ]);
    let type_params = type_param_scope(&["T".to_string()]);

    assert_eq!(
        lower_type(&type_ref("str"), &type_names, &type_arities, &type_params)
            .expect("str should canonicalize to String"),
        Type::named("String")
    );
    assert_eq!(
        lower_type(
            &nested_type_ref("Option", vec![type_ref("int32")]),
            &type_names,
            &type_arities,
            &type_params,
        )
        .expect("Option should lower"),
        Type::Named("Option".to_string(), vec![Type::named("int32")])
    );
    assert_eq!(
        lower_type(
            &nested_type_ref("Map", vec![type_ref("String"), type_ref("int32")]),
            &type_names,
            &type_arities,
            &type_params,
        )
        .expect("Map should lower"),
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        )
    );
    assert_eq!(
        lower_type(
            &nested_type_ref("Box", vec![type_ref("T")]),
            &type_names,
            &type_arities,
            &type_params,
        )
        .expect("user generic should lower"),
        Type::Named("Box".to_string(), vec![Type::TypeParam("T".to_string())])
    );
    assert_eq!(
        lower_type(
            &type_ref("pkg.Counter"),
            &type_names,
            &type_arities,
            &type_params
        )
        .expect("qualified user type should lower"),
        Type::named("Counter")
    );
    assert_eq!(
        lower_type(&type_ref("T"), &type_names, &type_arities, &type_params)
            .expect("type param should lower"),
        Type::TypeParam("T".to_string())
    );
    assert_eq!(
        lower_type(&type_ref("None"), &type_names, &type_arities, &type_params)
            .expect("None should lower to unit"),
        Type::Unit
    );

    let unknown = lower_type(
        &type_ref("Unknown"),
        &type_names,
        &type_arities,
        &type_params,
    )
    .expect_err("unknown types should fail");
    assert!(unknown.message.contains("unknown type `Unknown`"));
    let option_arity = lower_type(
        &nested_type_ref("Option", vec![]),
        &type_names,
        &type_arities,
        &type_params,
    )
    .expect_err("Option arity mismatch should fail");
    assert!(option_arity
        .message
        .contains("expects exactly one type argument"));
    let task_group_args = lower_type(
        &nested_type_ref("TaskGroup", vec![type_ref("int32")]),
        &type_names,
        &type_arities,
        &type_params,
    )
    .expect_err("TaskGroup should reject type args");
    assert!(task_group_args
        .message
        .contains("does not take type arguments"));
    let type_param_args = lower_type(
        &nested_type_ref("T", vec![type_ref("int32")]),
        &type_names,
        &type_arities,
        &type_params,
    )
    .expect_err("type params should reject generic args");
    assert!(type_param_args
        .message
        .contains("type parameter `T` does not take type arguments"));
}

#[test]
fn lower_trait_bounds_reports_unknown_traits_and_arity_mismatches() {
    let traits = BTreeMap::from([
        ("Named".to_string(), trait_info("Named", vec![])),
        ("Mapper".to_string(), trait_info("Mapper", vec!["T"])),
    ]);
    let type_names = BTreeMap::from([("String".to_string(), Span::new(1, 1))]);
    let type_arities = BTreeMap::from([("String".to_string(), 0usize)]);
    let scope = type_param_scope(&["T".to_string()]);

    let unknown = lower_trait_bounds(
        &BTreeMap::from([("T".to_string(), vec![type_ref("Missing")])]),
        &traits,
        &type_names,
        &type_arities,
        &scope,
    )
    .expect_err("unknown traits should fail");
    assert!(unknown.message.contains("unknown trait `Missing`"));

    let arity = lower_trait_bounds(
        &BTreeMap::from([("T".to_string(), vec![nested_type_ref("Mapper", vec![])])]),
        &traits,
        &type_names,
        &type_arities,
        &scope,
    )
    .expect_err("trait arity mismatch should fail");
    assert!(arity.message.contains("expects 1 type arguments, found 0"));
}

#[test]
fn function_signature_helper_constructor_is_used() {
    let signature = function_signature(vec![Type::named("int32")], Type::named("bool"));
    assert_eq!(signature.params, vec![Type::named("int32")]);
    assert_eq!(signature.return_type, Type::named("bool"));
    let decl = function_decl("ready");
    assert_eq!(decl.name, "ready");
}

#[test]
fn structured_wait_helpers_cover_valid_and_error_paths() {
    let valid = crate::check_source(
            "def worker(value: int32) -> int32:\n    return value\n\ndef notify(value: int32):\n    print(value)\n\ndef main() -> int32:\n    jobs = Queue[int32]()\n    with TaskGroup() as group:\n        mut tasks = Vec[Task[int32]]()\n        tasks.push(group.start(worker, 1))\n        group.start_soon(notify, 2)\n        print(wait_any(tasks, timeout=1ms))\n        print(wait_all(tasks))\n    match jobs.get(timeout=1ms):\n        case QueueReceive.TimedOut:\n            pass\n        case _:\n            pass\n    return 0\n",
        )
        .expect("structured wait helpers should type check");
    assert!(valid.functions.contains_key("main"));

    let wait_non_tasks =
        crate::check_source("def main() -> int32:\n    return wait_any(tasks=true)\n")
            .expect_err("wait_any should reject non-task vectors");
    assert!(wait_non_tasks.message.contains("expects `Vec[Task[T]]`"));

    let wait_timeout = crate::check_source(
            "def worker(value: int32) -> int32:\n    return value\n\ndef main() -> int32:\n    with TaskGroup() as group:\n        mut tasks = Vec[Task[int32]]()\n        tasks.push(group.start(worker, 1))\n        return wait_all(tasks, timeout=1)\n",
        )
        .expect_err("wait_all timeout should require Duration");
    assert!(wait_timeout
        .message
        .contains("`wait_all(timeout=...)` expects `Duration`, found `int32`"));

    let recv_timeout = crate::check_source(
        "def main() -> int32:\n    jobs = Queue[int32]()\n    return jobs.get(timeout=1)\n",
    )
    .expect_err("queue.get timeout should require Duration");
    assert!(recv_timeout
        .message
        .contains("`get(timeout=...)` expects `Duration`, found `int32`"));

    let send_wrong_type = crate::check_source(
        "def main() -> int32:\n    jobs = Queue[int32]()\n    return jobs.put(\"bad\")\n",
    )
    .expect_err("queue.put should enforce payload types");
    assert!(send_wrong_type.message.contains("`put` expects `int32`"));

    let start_soon_target = crate::check_source(
            "def main() -> int32:\n    with TaskGroup() as group:\n        group.start_soon(1)\n    return 0\n",
        )
        .expect_err("start_soon requires a callable");
    assert!(start_soon_target.message.contains(
        "task starting currently supports named functions and associated methods without `self`"
    ));
}

#[test]
fn checker_function_default_loop_and_resource_validation_cover_additional_branches() {
    for (source, expected) in [
            (
                "def helper(value: borrow int32 = 1) -> None:\n    pass\n\ndef main() -> None:\n    pass\n",
                "borrowed parameter `value` may not have a default value",
            ),
            (
                "def helper(value: int32 = true) -> int32:\n    return value\n\ndef main() -> None:\n    pass\n",
                "default argument for parameter `value` has type `bool`, expected `int32`",
            ),
            (
                "def helper(left: int32 = 1, right: int32) -> int32:\n    return right\n\ndef main() -> None:\n    pass\n",
                "parameters with default arguments must come after required parameters",
            ),
            (
                "def helper() -> int32:\n    pass\n\ndef main() -> None:\n    pass\n",
                "function `helper` is missing a return",
            ),
            (
                "class Counter:\n    def current(borrow self) -> int32:\n        pass\n",
                "method `current` is missing a return",
            ),
            (
                "trait Show:\n    def show(value: int32) -> int32\n\nclass Box:\n    pass\n\nimpl Show for Box:\n    def show(value: int32 = 1) -> int32:\n        return value\n",
                "default arguments are not allowed in impl method declarations",
            ),
            (
                "trait Show:\n    def show() -> int32\n\nclass Box:\n    pass\n\nimpl Show for Box:\n    def show() -> int32:\n        pass\n",
                "method `show` is missing a return",
            ),
            (
                "def main() -> None:\n    values = Set{1}\n    for value in borrow mut values:\n        pass\n",
                "`for value in borrow mut ...:` is not supported for `Set[T]`; use `insert`/`remove` on the set directly",
            ),
            (
                "def main() -> None:\n    values = [1]\n    for value in borrow mut values:\n        pass\n",
                "`for value in borrow mut ...:` requires a mutable `Vec[T]` place",
            ),
            (
                "def main() -> None:\n    flag = true\n    for value in flag:\n        pass\n",
                "`for` currently requires a `Range`, `Queue[T]`, `Vec[T]`, or `Set[T]` iterable, found `bool`",
            ),
            (
                "def main() -> None:\n    value = 1\n    for value in range(3):\n        pass\n",
                "loop binding `value` would shadow an existing name",
            ),
            (
                "class Resource:\n    def close(borrow mut self):\n        pass\n\ndef main() -> None:\n    resource = Resource()\n    with resource as resource:\n        pass\n",
                "with binding `resource` would shadow an existing name",
            ),
        ] {
            let error = crate::check_source(source).expect_err("source should fail checking");
            assert!(
                error.message.contains(expected),
                "expected `{expected}` in diagnostic, got `{}`",
                error.message
            );
        }
}

#[test]
fn checker_loop_move_helper_reports_full_and_partial_repeated_moves() {
    let program = crate::check_source("class Name:\n    value: String\n\ndef main():\n    pass\n")
        .expect("helper program should type check");
    let (type_names, type_arities) = type_maps_from_program(&program);
    let checker = FunctionChecker::new(
        &program.module_name,
        &type_names,
        &type_arities,
        &program.classes,
        &program.enums,
        &program.functions,
        &program.traits,
        &program.trait_impls,
        &program.imported_modules,
        &program.module_registry,
    );
    let span = Span::new(2, 3);

    let locals = HashMap::from([(
        "name".to_string(),
        local_binding(
            Type::named("Name"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    let moved_body = HashMap::from([(
        "name".to_string(),
        local_binding(
            Type::named("Name"),
            true,
            true,
            ReceiverKind::Value,
            true,
            &[],
        ),
    )]);
    let moved_error = checker
        .reject_loop_carried_moves(&locals, &moved_body, "while", span)
        .expect_err("repeated full moves from a loop body should be rejected");
    assert!(moved_error
        .message
        .contains("`while` loop body moves `name` and may execute more than once"));

    let partial_body = HashMap::from([(
        "name".to_string(),
        local_binding(
            Type::named("Name"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &["value"],
        ),
    )]);
    let partial_error = checker
        .reject_loop_carried_moves(&locals, &partial_body, "for", span)
        .expect_err("repeated partial moves from a loop body should be rejected");
    assert!(partial_error
        .message
        .contains("`for` loop body partially moves `name` and may execute more than once"));
}

#[test]
fn checker_direct_entrypoints_cover_top_level_function_method_and_impl_paths() {
    let classes = BTreeMap::from([(
        "Counter".to_string(),
        class_info(
            "Counter",
            false,
            vec![("value", Type::named("int32"), false)],
        ),
    )]);
    let type_names = BTreeMap::from([("Counter".to_string(), Span::new(1, 1))]);
    let type_arities = BTreeMap::from([("Counter".to_string(), 0usize)]);
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let span = Span::new(1, 1);

    checker
        .check_top_level(&[Stmt::Pass(PassStmt { span })])
        .expect("top-level pass should be allowed");

    let top_level_return = checker
        .check_top_level(&[Stmt::Return(ReturnStmt {
            value: Some(expr(ExprKind::Int(1))),
            span,
        })])
        .expect_err("top-level return should be rejected");
    assert!(top_level_return
        .message
        .contains("`return` is only allowed inside a function body"));

    let top_level_break = checker
        .check_top_level(&[Stmt::Break(BreakStmt { span })])
        .expect_err("top-level break should be rejected");
    assert!(top_level_break
        .message
        .contains("`break` is only allowed inside a loop"));

    let top_level_continue = checker
        .check_top_level(&[Stmt::Continue(ContinueStmt { span })])
        .expect_err("top-level continue should be rejected");
    assert!(top_level_continue
        .message
        .contains("`continue` is only allowed inside a loop"));

    let mut function_ok = function_decl("helper");
    function_ok.return_type = type_ref("int32");
    function_ok.body = vec![Stmt::Return(ReturnStmt {
        value: Some(expr(ExprKind::Int(7))),
        span,
    })];
    checker
        .check_function(&function_ok)
        .expect("ordinary functions with matching returns should pass");

    let mut function_missing_return = function_decl("missing");
    function_missing_return.return_type = type_ref("int32");
    function_missing_return.body = vec![Stmt::Pass(PassStmt { span })];
    let function_error = checker
        .check_function(&function_missing_return)
        .expect_err("non-unit functions without returns should fail");
    assert!(function_error
        .message
        .contains("function `missing` is missing a return"));

    let class_decl = classes
        .get("Counter")
        .expect("Counter class info should exist")
        .decl
        .clone();
    let mut method_ok = function_decl("read");
    method_ok.receiver = Some(ReceiverKind::Borrow);
    method_ok.return_type = type_ref("int32");
    method_ok.body = vec![Stmt::Return(ReturnStmt {
        value: Some(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Name("self".to_string()))),
            field: "value".to_string(),
        })),
        span,
    })];
    checker
        .check_method(&class_decl, &method_ok)
        .expect("class methods should be checked with an implicit self binding");

    let mut method_missing_return = function_decl("stuck");
    method_missing_return.receiver = Some(ReceiverKind::Borrow);
    method_missing_return.return_type = type_ref("int32");
    method_missing_return.body = vec![Stmt::Pass(PassStmt { span })];
    let method_error = checker
        .check_method(&class_decl, &method_missing_return)
        .expect_err("non-unit methods without returns should fail");
    assert!(method_error
        .message
        .contains("method `stuck` is missing a return"));

    let mut impl_method_ok = function_decl("touch");
    impl_method_ok.receiver = Some(ReceiverKind::Borrow);
    impl_method_ok.return_type = type_ref("int32");
    impl_method_ok.body = vec![Stmt::Return(ReturnStmt {
        value: Some(expr(ExprKind::Int(1))),
        span,
    })];
    checker
        .check_trait_impl_method(
            &Type::named("Counter"),
            &[],
            &BTreeMap::new(),
            &impl_method_ok,
        )
        .expect("impl methods without defaults should type check");

    let mut impl_method_with_default = function_decl("touch");
    impl_method_with_default.receiver = Some(ReceiverKind::Borrow);
    impl_method_with_default.return_type = type_ref("int32");
    impl_method_with_default.params = vec![Param {
        name: "value".to_string(),
        ty: type_ref("int32"),
        passing: ReceiverKind::Value,
        borrow_label: None,
        default: Some(expr(ExprKind::Int(1))),
        span,
    }];
    impl_method_with_default.body = vec![Stmt::Return(ReturnStmt {
        value: Some(expr(ExprKind::Int(1))),
        span,
    })];
    let impl_default_error = checker
        .check_trait_impl_method(
            &Type::named("Counter"),
            &[],
            &BTreeMap::new(),
            &impl_method_with_default,
        )
        .expect_err("impl methods should still reject default arguments");
    assert!(impl_default_error
        .message
        .contains("default arguments are not allowed in impl method declarations"));
}

#[test]
fn checker_select_and_assignment_direct_helpers_cover_remaining_error_and_success_paths() {
    let classes = BTreeMap::from([(
        "Counter".to_string(),
        class_info(
            "Counter",
            false,
            vec![("value", Type::named("int32"), false)],
        ),
    )]);
    let type_names = BTreeMap::from([("Counter".to_string(), Span::new(1, 1))]);
    let type_arities = BTreeMap::from([("Counter".to_string(), 0usize)]);
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let _span = Span::new(1, 1);

    let mut locals = HashMap::from([
        (
            "values".to_string(),
            local_binding(
                Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "mapping".to_string(),
            local_binding(
                Type::Named(
                    "Map".to_string(),
                    vec![Type::named("String"), Type::named("int32")],
                ),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "counter".to_string(),
            local_binding(
                Type::named("Counter"),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "jobs".to_string(),
            local_binding(
                Type::Named("Queue".to_string(), vec![Type::named("int32")]),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
    ]);

    let invalid_wait_any = checker
        .type_of_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("wait_any".to_string()))),
                args: vec![arg(expr(ExprKind::Bool(true)))],
            }),
            &mut locals,
        )
        .expect_err("wait_any should reject non-task vectors");
    assert!(invalid_wait_any.message.contains("expects `Vec[Task[T]]`"));

    let index_mut_error = checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("values".to_string()))),
                    index: Box::new(expr(ExprKind::Int(0))),
                },
                true,
                None,
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("index assignments should reject `mut`");
    assert!(index_mut_error
        .message
        .contains("`mut` can only be used when introducing a new binding"));

    let index_annotation_error = checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("values".to_string()))),
                    index: Box::new(expr(ExprKind::Int(0))),
                },
                false,
                Some(type_ref("int32")),
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("index assignments should reject annotations");
    assert!(index_annotation_error
        .message
        .contains("index assignment cannot include a type annotation"));

    checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("values".to_string()))),
                    index: Box::new(expr(ExprKind::Int(0))),
                },
                false,
                None,
                Some(BinaryOp::Add),
                expr(ExprKind::Int(4)),
            ),
            &mut locals,
        )
        .expect("compound vector assignment should type check");

    checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                    index: Box::new(expr(ExprKind::String("count".to_string()))),
                },
                false,
                None,
                Some(BinaryOp::Add),
                expr(ExprKind::Int(4)),
            ),
            &mut locals,
        )
        .expect("compound map assignment should type check");

    let member_mut_error = checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Member {
                    object: Box::new(expr(ExprKind::Name("counter".to_string()))),
                    field: "value".to_string(),
                },
                true,
                None,
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("member assignments should reject `mut`");
    assert!(member_mut_error
        .message
        .contains("`mut` can only be used when introducing a new binding"));

    let member_annotation_error = checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Member {
                    object: Box::new(expr(ExprKind::Name("counter".to_string()))),
                    field: "value".to_string(),
                },
                false,
                Some(type_ref("int32")),
                None,
                expr(ExprKind::Int(1)),
            ),
            &mut locals,
        )
        .expect_err("member assignments should reject annotations");
    assert!(member_annotation_error
        .message
        .contains("member assignment cannot include a type annotation"));

    checker
        .check_assign(
            &assign_stmt(
                AssignTarget::Member {
                    object: Box::new(expr(ExprKind::Name("counter".to_string()))),
                    field: "value".to_string(),
                },
                false,
                None,
                Some(BinaryOp::Add),
                expr(ExprKind::Int(4)),
            ),
            &mut locals,
        )
        .expect("compound member assignment should type check");
}

#[test]
fn builtin_call_and_member_resolution_surface_type_checks() {
    let program = crate::check_source(
            "def main() -> int32:\n    text = \"  Aurora  \"\n    pieces: Vec[String] = text.split(\"u\")\n    replaced: String = text.replace(\"Aurora\", \"language\")\n    lowered: String = text.to_lower()\n    raised: String = text.to_upper()\n    prefix: Option[String] = text.strip_prefix(\"  \")\n    suffix: Option[String] = text.strip_suffix(\"  \")\n    text_len: int32 = text.len()\n    text_has: bool = text.contains(\"Aur\")\n    text_start: bool = text.starts_with(\"  A\")\n    text_end: bool = text.ends_with(\"  \")\n    parsed_i32: Result[int32, String] = parse_int32(text=\"7\")\n    parsed_i64: Result[int64, String] = parse_int64(text=\"9\")\n    parsed_f64: Result[float64, String] = parse_float64(text=\"3.5\")\n    abs_i32: int32 = abs(value=-7)\n    min_i32: int32 = min(left=1, right=2)\n    max_i32: int32 = max(left=1, right=2)\n    root: float64 = sqrt(value=9.0)\n    mut values = [1, 2, 3]\n    popped: Option[int32] = values.pop()\n    gotten: Option[int32] = values.get(index=0)\n    inserted: bool = values.insert(index=0, value=9)\n    mut counts = {\"a\": 1}\n    keys: Vec[String] = counts.keys()\n    vals: Vec[int32] = counts.values()\n    entries: Vec[MapEntry[String, int32]] = counts.items()\n    mut names = Set{\"ada\"}\n    has_name: bool = names.contains(\"ada\")\n    inserted_name: bool = names.insert(\"bob\")\n    removed_name: bool = names.remove(\"ada\")\n    return text_len + abs_i32 + min_i32 + max_i32 + (root as int32)\n",
        )
        .expect("builtin call/member surface should type check");
    assert!(program.functions.contains_key("main"));
}

#[test]
fn checker_builtin_function_success_surface_infers_expected_types() {
    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();
    let classes = BTreeMap::new();
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let span = Span::new(1, 1);
    let string_ty = Type::named("String");
    let float_ty = Type::named("float64");
    let int_ty = Type::named("int32");
    let channel_ty = Type::Named("Queue".to_string(), vec![Type::named("String")]);
    let mut locals = HashMap::from([
        (
            "text".to_string(),
            local_binding(
                string_ty.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "ratio".to_string(),
            local_binding(
                float_ty.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "count".to_string(),
            local_binding(
                int_ty.clone(),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
    ]);

    for (label, callee, args, expected, expected_hint) in [
        (
            "print",
            expr(ExprKind::Name("print".to_string())),
            vec![arg(expr(ExprKind::Name("text".to_string())))],
            Type::Unit,
            None,
        ),
        (
            "range",
            expr(ExprKind::Name("range".to_string())),
            vec![
                named_arg("start", expr(ExprKind::Int(1))),
                named_arg("stop", expr(ExprKind::Int(3))),
            ],
            Type::named("Range"),
            None,
        ),
        (
            "Queue",
            expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Queue".to_string()))),
                type_args: vec![type_ref("String")],
            }),
            Vec::new(),
            channel_ty.clone(),
            None,
        ),
        (
            "TaskGroup",
            expr(ExprKind::Name("TaskGroup".to_string())),
            Vec::new(),
            Type::named("TaskGroup"),
            None,
        ),
        (
            "cancelled",
            expr(ExprKind::Name("cancelled".to_string())),
            Vec::new(),
            Type::named("bool"),
            None,
        ),
        (
            "sleep",
            expr(ExprKind::Name("sleep".to_string())),
            vec![arg(expr(ExprKind::DurationMillis(1)))],
            Type::Unit,
            None,
        ),
        (
            "abs",
            expr(ExprKind::Name("abs".to_string())),
            vec![named_arg(
                "value",
                expr(ExprKind::Name("count".to_string())),
            )],
            int_ty.clone(),
            None,
        ),
        (
            "min",
            expr(ExprKind::Name("min".to_string())),
            vec![
                named_arg("left", expr(ExprKind::Int(1))),
                named_arg("right", expr(ExprKind::Int(2))),
            ],
            int_ty.clone(),
            None,
        ),
        (
            "max",
            expr(ExprKind::Name("max".to_string())),
            vec![
                named_arg("left", expr(ExprKind::Name("ratio".to_string()))),
                named_arg("right", expr(ExprKind::Name("ratio".to_string()))),
            ],
            float_ty.clone(),
            None,
        ),
        (
            "sqrt",
            expr(ExprKind::Name("sqrt".to_string())),
            vec![named_arg(
                "value",
                expr(ExprKind::Name("ratio".to_string())),
            )],
            float_ty.clone(),
            None,
        ),
        (
            "parse_int32",
            expr(ExprKind::Name("parse_int32".to_string())),
            vec![named_arg("text", expr(ExprKind::Name("text".to_string())))],
            Type::Named(
                "Result".to_string(),
                vec![Type::named("int32"), string_ty.clone()],
            ),
            None,
        ),
        (
            "parse_int64",
            expr(ExprKind::Name("parse_int64".to_string())),
            vec![named_arg("text", expr(ExprKind::Name("text".to_string())))],
            Type::Named(
                "Result".to_string(),
                vec![Type::named("int64"), string_ty.clone()],
            ),
            None,
        ),
        (
            "parse_float64",
            expr(ExprKind::Name("parse_float64".to_string())),
            vec![named_arg("text", expr(ExprKind::Name("text".to_string())))],
            Type::Named(
                "Result".to_string(),
                vec![Type::named("float64"), string_ty.clone()],
            ),
            None,
        ),
        (
            "Vec",
            expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Vec".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            Vec::new(),
            Type::Named("Vec".to_string(), vec![Type::named("int32")]),
            None,
        ),
        (
            "Set",
            expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Set".to_string()))),
                type_args: vec![type_ref("String")],
            }),
            Vec::new(),
            Type::Named("Set".to_string(), vec![string_ty.clone()]),
            None,
        ),
        (
            "Map",
            expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Map".to_string()))),
                type_args: vec![type_ref("String"), type_ref("int32")],
            }),
            Vec::new(),
            Type::Named(
                "Map".to_string(),
                vec![string_ty.clone(), Type::named("int32")],
            ),
            None,
        ),
    ] {
        assert_eq!(
            checker
                .type_of_call(&callee, &args, span, &mut locals, expected_hint.as_ref())
                .unwrap_or_else(|error| panic!(
                    "{label} builtin constructor/call should type check: {error:?}"
                )),
            expected
        );
    }
}

#[test]
fn method_receiver_borrow_aliasing_checks_overlap_and_distinct_places() {
    let overlap = crate::check_source(
            "class Acc:\n    value: int32\n\n    def add_from(borrow mut self, source: borrow mut Acc):\n        self.value += source.value\n\ndef main() -> int32:\n    mut acc = Acc(value=1)\n    acc.add_from(source=acc)\n    return 0\n",
        )
        .expect_err("overlapping receiver and argument borrows should fail");
    assert!(overlap.message.contains(
            "argument for parameter `source` in method `add_from` overlaps mutable borrow for parameter `self`"
        ));

    let distinct = crate::check_source(
            "class Acc:\n    value: int32\n\n    def add_from(borrow mut self, source: borrow mut Acc):\n        self.value += source.value\n\ndef main() -> int32:\n    mut left = Acc(value=1)\n    mut right = Acc(value=2)\n    left.add_from(source=right)\n    return 0\n",
        )
        .expect("distinct receiver and argument borrows should type check");
    assert!(distinct.functions.contains_key("main"));
}

#[test]
fn checker_match_and_builtin_error_surfaces_cover_remaining_branches() {
    for (source, expected) in [
            (
                "def main() -> int32:\n    match 1:\n        case 1:\n            return 1\n        case 1:\n            return 2\n        case _:\n            return 3\n",
                "duplicate match arm for literal `1`",
            ),
            (
                "def main() -> int32:\n    match 1:\n        case _:\n            return 1\n        case 2:\n            return 2\n",
                "wildcard match arm must be the final `case`",
            ),
            (
                "def main() -> int32:\n    match 1:\n        case true:\n            return 1\n        case _:\n            return 0\n",
                "literal pattern `true` does not match scrutinee type `int32`",
            ),
            (
                "def main() -> int32:\n    match 1:\n        case \"aurora\":\n            return 1\n        case _:\n            return 0\n",
                "literal pattern \"aurora\" does not match scrutinee type `int32`",
            ),
            (
                "def main() -> int32:\n    match true:\n        case true:\n            return 1\n",
                "non-exhaustive match over `bool`: missing `false`",
            ),
            (
                "def main() -> int32:\n    match 1:\n        case 1:\n            return 1\n",
                "`match` over `int32` with literal patterns requires a final `case _:` arm",
            ),
            (
                "def main() -> int32:\n    value: int8 = 1\n    match value:\n        case 200:\n            return 1\n        case _:\n            return 0\n",
                "literal pattern `200` does not fit scrutinee type `int8`",
            ),
            (
                "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    match status:\n        case 1:\n            return 1\n        case _:\n            return 0\n",
                "match over `Status` expects enum variant patterns, not literal `1`",
            ),
            (
                "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    match status:\n        case Other.Ready:\n            return 1\n        case _:\n            return 0\n",
                "unknown enum `Other` in match pattern",
            ),
            (
                "enum Status:\n    Done(int32)\n\ndef main() -> int32:\n    status = Status.Done(1)\n    match status:\n        case Status.Done:\n            return 1\n        case _:\n            return 0\n",
                "variant `Status.Done` carries a payload and must bind it",
            ),
            (
                "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    match status:\n        case Status.Ready(value):\n            return 1\n        case _:\n            return 0\n",
                "variant `Status.Ready` does not carry a payload",
            ),
            (
                "enum Status:\n    Done(int32)\n\ndef main() -> int32:\n    value = 1\n    status = Status.Done(1)\n    match status:\n        case Status.Done(value):\n            return value\n        case _:\n            return 0\n",
                "pattern binding `value` would shadow an existing name",
            ),
            (
                "enum Leaf:\n    Value(int32)\n\nenum Outer:\n    Wrap(Leaf)\n\ndef main() -> int32:\n    value = Outer.Wrap(value=Leaf.Value(value=1))\n    match value:\n        case Outer.Wrap(Leaf.Value(left, right)):\n            return left\n        case _:\n            return 0\n",
                "variant `Leaf.Value` expects 1 pattern payload, found 2",
            ),
            (
                "enum Status:\n    Ready\n    Done\n\ndef main() -> int32:\n    status = Status.Ready\n    match status:\n        case Status.Ready:\n            return 1\n",
                "non-exhaustive match over `Status`: missing `Done`",
            ),
            (
                "def main() -> int32:\n    return range(start=true, stop=3)\n",
                "`range` arguments must have type `int32`, found `bool`",
            ),
            (
                "def main() -> int32:\n    return wait_any(tasks=true)\n",
                "`wait_any` expects `Vec[Task[T]]`, found `bool`",
            ),
            (
                "def main() -> int32:\n    jobs = Queue[int32]()\n    return jobs.get(timeout=1)\n",
                "`get(timeout=...)` expects `Duration`, found `int32`",
            ),
            (
                "def main() -> int32:\n    return sleep(duration=1)\n",
                "`sleep(...)` expects a `Duration`, found `int32`",
            ),
            (
                "def main() -> int32:\n    return abs(value=\"x\")\n",
                "`abs(...)` expects an integer or float value, found `String`",
            ),
            (
                "def main() -> int32:\n    return min(left=true, right=false)\n",
                "`min` expects numeric arguments, found `bool`",
            ),
            (
                "def main() -> int32:\n    return max(left=1, right=2.0)\n",
                "`max` arguments must match, found `int32` and `float64`",
            ),
            (
                "def main() -> int32:\n    return sqrt(value=9)\n",
                "`sqrt(...)` expects `float32` or `float64`, found `int32`",
            ),
            (
                "def main() -> int32:\n    return parse_int32(text=1)\n",
                "`parse_int32(...)` expects `String`, found `int32`",
            ),
            (
                "def main() -> int32:\n    return parse_int64(text=1)\n",
                "`parse_int64(...)` expects `String`, found `int32`",
            ),
            (
                "def main() -> int32:\n    return parse_float64(text=1)\n",
                "`parse_float64(...)` expects `String`, found `int32`",
            ),
        ] {
            let error = crate::check_source(source)
                .expect_err("checker surface case should report a diagnostic");
            assert!(
                error.message.contains(expected),
                "expected diagnostic containing `{expected}`, got `{}` for source:\n{}",
                error.message,
                source
            );
        }
}

#[test]
fn operator_trait_and_bound_helpers_cover_checker_resolution_paths() {
    let program = crate::check_source(
        "\
trait Named:
    def name(borrow self) -> String

trait Add[Rhs, Out]:
    def add(borrow self, rhs: Rhs) -> Out

trait Neg[Out]:
    def neg(borrow self) -> Out

class User:
    label: String

class Point:
    x: int32

impl Named for User:
    def name(borrow self) -> String:
        return self.label

impl Add[Point, Point] for Point:
    def add(borrow self, rhs: Point) -> Point:
        return Point(x=self.x + rhs.x)

impl Neg[Point] for Point:
    def neg(borrow self) -> Point:
        return Point(x=0 - self.x)

def main():
    pass
",
    )
    .expect("operator trait program should type-check");
    let (type_names, type_arities) = type_maps_from_program(&program);
    let span = Span::new(1, 1);
    let base_checker = checker(
        &program.module_name,
        &type_names,
        &type_arities,
        &program.classes,
        &program.enums,
        &program.functions,
        &program.traits,
        &program.trait_impls,
        &program.imported_modules,
        &program.module_registry,
    );

    assert_eq!(
        base_checker
            .type_of_unary_operator_via_trait(span, UnaryOp::Neg, &Type::named("Point"))
            .expect("neg trait lookup should succeed"),
        Some(Type::named("Point"))
    );
    assert_eq!(
        base_checker
            .type_of_binary_operator_via_trait(
                span,
                BinaryOp::Add,
                &Type::named("Point"),
                &Type::named("Point"),
            )
            .expect("add trait lookup should succeed"),
        Some(Type::named("Point"))
    );
    assert_eq!(
        base_checker
            .resolve_member_type(&Type::named("User"), "name", span)
            .expect("trait methods should resolve through member lookup"),
        Type::named("String")
    );
    base_checker
        .assert_type_satisfies_bounds(
            &Type::named("User"),
            &[TraitBound {
                trait_name: "Named".to_string(),
                trait_args: Vec::new(),
            }],
            span,
        )
        .expect("User should satisfy Named");
    let concrete_bound_error = base_checker
        .assert_type_satisfies_bounds(
            &Type::named("Point"),
            &[TraitBound {
                trait_name: "Named".to_string(),
                trait_args: Vec::new(),
            }],
            span,
        )
        .expect_err("Point should not satisfy Named");
    assert!(concrete_bound_error
        .message
        .contains("type `Point` does not implement trait `Named`"));

    let type_param_checker = base_checker.with_type_params(
        BTreeMap::from([("T".to_string(), ()), ("U".to_string(), ())]),
        BTreeMap::from([
            (
                "T".to_string(),
                vec![TraitBound {
                    trait_name: "Add".to_string(),
                    trait_args: vec![Type::named("Point"), Type::named("Point")],
                }],
            ),
            (
                "U".to_string(),
                vec![TraitBound {
                    trait_name: "Neg".to_string(),
                    trait_args: vec![Type::named("Point")],
                }],
            ),
        ]),
    );
    assert_eq!(
        type_param_checker
            .type_of_binary_operator_via_trait(
                span,
                BinaryOp::Add,
                &Type::TypeParam("T".to_string()),
                &Type::named("Point"),
            )
            .expect("type-param add bound should resolve"),
        Some(Type::named("Point"))
    );
    assert_eq!(
            type_param_checker
                .type_of_unary_operator_via_trait(
                    span,
                    UnaryOp::Neg,
                    &Type::TypeParam("U".to_string()),
                )
                .expect("type-param neg bound should resolve"),
            Some(Type::named("Point"))
        );
    let type_param_bound_error = type_param_checker
        .assert_type_satisfies_bounds(
            &Type::TypeParam("T".to_string()),
            &[TraitBound {
                trait_name: "Named".to_string(),
                trait_args: Vec::new(),
            }],
            span,
        )
        .expect_err("type parameter without Named bound should fail");
    assert!(type_param_bound_error
        .message
        .contains("type parameter `T` does not satisfy trait bound `Named`"));
}

#[test]
fn operator_method_from_type_param_reports_ambiguity_when_multiple_bounds_match() {
    let mut add_trait = trait_info("Add", vec!["Rhs", "Out"]);
    let mut add_decl = function_decl("add");
    add_decl.receiver = Some(ReceiverKind::Borrow);
    add_decl.params = vec![Param {
        name: "rhs".to_string(),
        passing: ReceiverKind::Value,
        borrow_label: None,
        ty: type_ref("Rhs"),
        default: None,
        span: Span::new(1, 1),
    }];
    add_decl.return_type = type_ref("Out");
    add_trait.methods.insert(
        "add".to_string(),
        TraitMethodInfo {
            decl: add_decl,
            signature: function_signature(
                vec![Type::TypeParam("Rhs".to_string())],
                Type::TypeParam("Out".to_string()),
            ),
            type_param_bounds: BTreeMap::new(),
        },
    );

    let type_names = BTreeMap::from([("Add".to_string(), Span::new(1, 1))]);
    let type_arities = BTreeMap::from([("Add".to_string(), 2usize)]);
    let classes = BTreeMap::new();
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::from([("Add".to_string(), add_trait)]);
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    )
    .with_type_params(
        BTreeMap::from([("T".to_string(), ())]),
        BTreeMap::from([(
            "T".to_string(),
            vec![
                TraitBound {
                    trait_name: "Add".to_string(),
                    trait_args: vec![Type::named("int32"), Type::named("String")],
                },
                TraitBound {
                    trait_name: "Add".to_string(),
                    trait_args: vec![Type::named("int32"), Type::named("bool")],
                },
            ],
        )]),
    );

    let error = match checker.operator_method_from_type_param(
        "T",
        "Add",
        "add",
        Some(&Type::named("int32")),
    ) {
        Ok(_) => panic!("multiple matching Add bounds should be ambiguous"),
        Err(error) => error,
    };
    assert!(error
        .message
        .contains("operator trait `Add` is ambiguous for type parameter `T`"));
}

#[test]
fn module_namespace_and_builtin_enum_helpers_cover_resolution_paths() {
    let span = Span::new(1, 1);
    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();
    let classes = BTreeMap::new();
    let enums = BTreeMap::new();
    let functions = BTreeMap::from([(
        "work".to_string(),
        FunctionInfo {
            module_name: "pkg.tools".to_string(),
            decl: function_decl("work"),
            signature: function_signature(Vec::new(), Type::Unit),
            type_param_bounds: BTreeMap::new(),
        },
    )]);
    let traits = BTreeMap::new();

    let mut tools = namespace("pkg.tools");
    tools.functions.insert(
        "work".to_string(),
        FunctionInfo {
            module_name: "pkg.tools".to_string(),
            decl: function_decl("work"),
            signature: function_signature(Vec::new(), Type::Unit),
            type_param_bounds: BTreeMap::new(),
        },
    );
    tools.all_functions = tools.functions.clone();
    tools.classes.insert(
        "Widget".to_string(),
        class_info(
            "Widget",
            false,
            vec![("value", Type::named("int32"), false)],
        ),
    );
    tools.classes.get_mut("Widget").unwrap().methods.insert(
        "build".to_string(),
        MethodInfo {
            decl: {
                let mut decl = function_decl("build");
                decl.return_type = type_ref("int32");
                decl
            },
            signature: function_signature(Vec::new(), Type::named("int32")),
            type_param_bounds: BTreeMap::new(),
        },
    );
    tools.all_classes = tools.classes.clone();
    tools.enums.insert(
        "Status".to_string(),
        enum_info("Status", Some(Type::named("int32"))),
    );
    tools.all_enums = tools.enums.clone();
    tools
        .modules
        .insert("inner".to_string(), namespace("pkg.tools.inner"));

    let mut pkg = namespace("pkg");
    pkg.modules.insert("tools".to_string(), tools.clone());

    let imported_modules = BTreeMap::from([("pkg".to_string(), pkg.clone())]);
    let module_registry = BTreeMap::from([
        ("pkg".to_string(), pkg),
        ("pkg.tools".to_string(), tools.clone()),
    ]);

    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let mut locals = HashMap::from([(
        "tools_module".to_string(),
        local_binding(
            Type::Module("pkg.tools".to_string()),
            false,
            false,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    let tools_expr = expr(ExprKind::Name("tools_module".to_string()));
    let module_function = expr(ExprKind::Member {
        object: Box::new(tools_expr.clone()),
        field: "work".to_string(),
    });
    let module_widget = expr(ExprKind::Member {
        object: Box::new(tools_expr.clone()),
        field: "Widget".to_string(),
    });

    assert_eq!(
        checker
            .type_of_call(&module_function, &[], span, &mut locals, None)
            .expect("module function calls should resolve"),
        Type::Unit
    );
    assert_eq!(
        checker
            .type_of_call(
                &module_widget,
                &[named_arg("value", expr(ExprKind::Int(1)))],
                span,
                &mut locals,
                None,
            )
            .expect("module class constructors should resolve"),
        Type::named("Widget")
    );
    assert_eq!(
        checker
            .type_of_call(
                &module_widget,
                &[arg(expr(ExprKind::Int(1)))],
                span,
                &mut locals,
                None,
            )
            .expect("module constructors should now accept positional arguments"),
        Type::named("Widget")
    );
    assert!(checker
        .type_of_call(
            &module_widget,
            &[named_arg("missing", expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("module constructors should reject unknown fields")
        .message
        .contains("has no field named `missing`"));
    assert!(checker
        .type_of_call(
            &module_widget,
            &[
                named_arg("value", expr(ExprKind::Int(1))),
                named_arg("value", expr(ExprKind::Int(2))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("module constructors should reject duplicate fields")
        .message
        .contains("provided more than once"));
    assert!(checker
        .type_of_call(
            &module_widget,
            &[named_arg("value", expr(ExprKind::Bool(true)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("module constructors should enforce field types")
        .message
        .contains("field `value` expects `int32`, found `bool`"));
    assert!(checker
        .type_of_call(&module_widget, &[], span, &mut locals, None)
        .expect_err("module constructors should require all required fields")
        .message
        .contains("missing required field `value`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(tools_expr.clone()),
                field: "missing".to_string(),
            }),
            &[],
            span,
            &mut locals,
            None,
        )
        .expect_err("unknown module callable members should fail")
        .message
        .contains("module `pkg.tools` has no callable member `missing`"));

    assert_eq!(
        checker
            .module_namespace("pkg.tools")
            .map(|ns| ns.path.as_str()),
        Some("pkg.tools")
    );
    assert_eq!(
        checker
            .resolve_member_type(&Type::Module("pkg".to_string()), "tools", span)
            .expect("child module should resolve"),
        Type::Module("pkg.tools".to_string())
    );

    let fn_error = checker
        .resolve_member_type(&Type::Module("pkg.tools".to_string()), "work", span)
        .expect_err("module functions should require call syntax");
    assert!(fn_error.message.contains("must be called with `(...)`"));

    let class_error = checker
        .resolve_member_type(&Type::Module("pkg.tools".to_string()), "Widget", span)
        .expect_err("module classes should require constructor syntax");
    assert!(class_error
        .message
        .contains("must be constructed with `(...)`"));

    assert_eq!(
        checker
            .resolve_member_type(&Type::Module("pkg.tools".to_string()), "Status", span)
            .expect("module enums should resolve to enum types for qualified variant access"),
        Type::Named("Status".to_string(), Vec::new())
    );

    let missing_error = checker
        .resolve_member_type(&Type::Module("pkg.tools".to_string()), "missing", span)
        .expect_err("missing module members should fail");
    assert!(missing_error.message.contains("has no member `missing`"));

    assert_eq!(
        checker.builtin_enum_variant_payload(
            &Type::Named("Option".to_string(), vec![Type::named("int32")]),
            "Option",
            "Some",
        ),
        Some(vec![Type::named("int32")])
    );
    assert_eq!(
        checker.builtin_enum_variant_payload(
            &Type::Named("Option".to_string(), vec![Type::named("int32")]),
            "Option",
            "None",
        ),
        Some(Vec::new())
    );
    assert_eq!(
        checker
            .explicit_builtin_type(
                "Result",
                &[Type::named("int32"), Type::named("String")],
                span,
            )
            .expect("built-in enum specialization should succeed"),
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")]
        )
    );
    let builtin_arity = checker
        .explicit_builtin_type("Option", &[], span)
        .expect_err("wrong explicit type arg arity should fail");
    assert!(builtin_arity.message.contains("expects 1 type argument"));
    let builtin_missing = checker
        .explicit_builtin_type("Missing", &[], span)
        .expect_err("unknown builtin enum should fail");
    assert!(builtin_missing.message.contains("unknown name `Missing`"));

    let builtin_ctor = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Specialize {
            expr: Box::new(expr(ExprKind::Name("Option".to_string()))),
            type_args: vec![type_ref("int32")],
        })),
        field: "Some".to_string(),
    });
    assert!(checker.expr_can_use_partial_expected_hint(&builtin_ctor));
    assert!(checker.is_builtin_enum_constructor_expr(&expr(ExprKind::Name("Option".to_string()))));

    let mut locals = HashMap::new();
    assert_eq!(
        checker
            .type_check_builtin_enum_variant_constructor(
                "Option",
                "Some",
                &Type::Named("Option".to_string(), vec![Type::named("int32")]),
                &[arg(expr(ExprKind::Int(7)))],
                span,
                &mut locals,
            )
            .expect("builtin Option.Some constructor should type check"),
        Type::Named("Option".to_string(), vec![Type::named("int32")])
    );
    let none_payload = checker
        .type_check_builtin_enum_variant_constructor(
            "Option",
            "None",
            &Type::Named("Option".to_string(), vec![Type::named("int32")]),
            &[arg(expr(ExprKind::Int(7)))],
            span,
            &mut locals,
        )
        .expect_err("Option.None should reject payloads");
    assert!(none_payload.message.contains("does not take a payload"));

    let mut locals = HashMap::new();
    let qualified_widget_build = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
                field: "tools".to_string(),
            })),
            field: "Widget".to_string(),
        })),
        field: "build".to_string(),
    });
    assert_eq!(
        checker
            .type_of_call(&qualified_widget_build, &[], span, &mut locals, None)
            .expect("qualified module class associated methods should type check"),
        Type::named("int32")
    );

    let qualified_status_value = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
                field: "tools".to_string(),
            })),
            field: "Status".to_string(),
        })),
        field: "Value".to_string(),
    });
    let qualified_status_missing = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
                field: "tools".to_string(),
            })),
            field: "Status".to_string(),
        })),
        field: "Missing".to_string(),
    });

    let missing_variant = checker
        .type_of_call(&qualified_status_missing, &[], span, &mut locals, None)
        .expect_err("missing qualified variants should fail");
    assert!(missing_variant
        .message
        .contains("enum `Status` has no variant `Missing`"));

    assert_eq!(
        checker
            .type_of_call(
                &qualified_status_value,
                &[named_arg("value", expr(ExprKind::Int(1)))],
                span,
                &mut locals,
                None,
            )
            .expect("qualified enum constructors should accept `value=`"),
        Type::named("Status")
    );

    let missing_payload = checker
        .type_of_call(&qualified_status_value, &[], span, &mut locals, None)
        .expect_err("qualified payload variants should require one argument");
    assert!(missing_payload.message.contains("payload"));

    let wrong_payload = checker
        .type_of_call(
            &qualified_status_value,
            &[arg(expr(ExprKind::Bool(true)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("qualified payload variants should enforce payload types");
    assert!(wrong_payload
        .message
        .contains("variant `Value` of enum `Status` expects `int32`, found `bool`"));
}

#[test]
fn module_qualified_builtin_io_error_variants_type_check() {
    crate::check_source(
        "import io\n\ndef main() -> int32:\n    err: io.Error = io.Error.NotFound\n    return 0\n",
    )
    .expect("qualified builtin io.Error variants should type-check");
}

#[test]
fn checker_module_resolution_helpers_cover_current_module_and_index_wrappers() {
    let span = Span::new(1, 1);
    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();
    let classes = BTreeMap::new();
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();

    let mut math = namespace("helpers.math");
    math.functions.insert(
        "work".to_string(),
        FunctionInfo {
            module_name: "helpers.math".to_string(),
            decl: function_decl("work"),
            signature: function_signature(Vec::new(), Type::Unit),
            type_param_bounds: BTreeMap::new(),
        },
    );
    math.all_functions = math.functions.clone();
    let mut math_widget = class_info(
        "Widget",
        false,
        vec![("value", Type::named("int32"), false)],
    );
    math_widget.module_name = "helpers.math".to_string();
    math.classes.insert("Widget".to_string(), math_widget);
    math.all_classes = math.classes.clone();
    let mut math_status = enum_info("Status", Some(Type::named("int32")));
    math_status.module_name = "helpers.math".to_string();
    math.enums.insert("Status".to_string(), math_status);
    math.all_enums = math.enums.clone();

    let mut other = namespace("helpers.other");
    let mut other_widget = class_info(
        "Widget",
        false,
        vec![("label", Type::named("String"), false)],
    );
    other_widget.module_name = "helpers.other".to_string();
    other.classes.insert("Widget".to_string(), other_widget);
    other.all_classes = other.classes.clone();
    let mut other_status = enum_info("Status", Some(Type::named("bool")));
    other_status.module_name = "helpers.other".to_string();
    other.enums.insert("Status".to_string(), other_status);
    other.all_enums = other.enums.clone();

    math.imported_modules
        .insert("other".to_string(), other.clone());

    let mut helpers = namespace("helpers");
    helpers.modules.insert("math".to_string(), math.clone());
    helpers.modules.insert("other".to_string(), other.clone());

    let imported_modules = BTreeMap::from([("helpers".to_string(), helpers.clone())]);
    let module_registry = BTreeMap::from([
        ("helpers".to_string(), helpers),
        ("helpers.math".to_string(), math.clone()),
        ("helpers.other".to_string(), other.clone()),
    ]);

    let root_checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    let helpers_expr = expr(ExprKind::Name("helpers".to_string()));
    let math_expr = expr(ExprKind::Member {
        object: Box::new(helpers_expr.clone()),
        field: "math".to_string(),
    });
    let specialized_math = expr(ExprKind::Specialize {
        expr: Box::new(math_expr.clone()),
        type_args: vec![type_ref("int32")],
    });
    let widget_expr = expr(ExprKind::Member {
        object: Box::new(math_expr.clone()),
        field: "Widget".to_string(),
    });
    let indexed_widget_expr = expr(ExprKind::Index {
        object: Box::new(widget_expr.clone()),
        index: Box::new(expr(ExprKind::Int(0))),
    });

    assert!(root_checker.current_module_namespace().is_none());
    assert_eq!(
        root_checker.infer_module_path(&helpers_expr),
        Some("helpers".to_string())
    );
    assert_eq!(
        root_checker.infer_module_path(&math_expr),
        Some("helpers.math".to_string())
    );
    assert_eq!(
        root_checker.infer_module_path(&specialized_math),
        Some("helpers.math".to_string())
    );
    assert_eq!(
        root_checker.infer_module_path(&expr(ExprKind::Group(Box::new(math_expr.clone())))),
        Some("helpers.math".to_string())
    );
    assert_eq!(
        root_checker.qualified_module_item(&indexed_widget_expr),
        Some(("helpers.math".to_string(), "Widget".to_string()))
    );
    assert!(root_checker.imported_class_info("Widget").is_none());
    assert!(root_checker.imported_enum_info("Status").is_none());
    assert!(root_checker.resolve_class_info("Widget").is_none());
    assert!(root_checker.resolve_enum_info("Status").is_none());
    assert_eq!(
        root_checker
            .resolve_class_info("helpers.math.Widget")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );
    assert_eq!(
        root_checker
            .resolve_enum_info("helpers.math.Status")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );

    let module_checker = checker(
        "helpers.math",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );
    assert_eq!(
        module_checker
            .current_module_namespace()
            .map(|namespace| namespace.path.as_str()),
        Some("helpers.math")
    );
    assert_eq!(
        module_checker.infer_module_path(&expr(ExprKind::Name("other".to_string()))),
        Some("helpers.other".to_string())
    );
    assert_eq!(
        module_checker
            .resolve_function_info("work")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );
    assert_eq!(
        module_checker
            .resolve_class_info("Widget")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );
    assert_eq!(
        module_checker
            .resolve_enum_info("Status")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );

    let member_error = module_checker
        .resolve_member_type(&Type::Module("helpers".to_string()), "missing", span)
        .expect_err("missing module members should still fail");
    assert!(member_error.message.contains("has no member `missing`"));
}

#[test]
fn place_path_and_resource_helpers_cover_remaining_checker_paths() {
    let span = Span::new(1, 1);
    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();

    let mut resource = class_info(
        "Resource",
        false,
        vec![("value", Type::named("int32"), false)],
    );
    let mut close_decl = function_decl("close");
    close_decl.receiver = Some(ReceiverKind::BorrowMut);
    let good_close = MethodInfo {
        decl: close_decl.clone(),
        signature: function_signature(Vec::new(), Type::Unit),
        type_param_bounds: BTreeMap::new(),
    };
    resource.methods.insert("close".to_string(), good_close);

    let mut bad_resource = class_info("BadResource", false, vec![]);
    let mut bad_close_decl = function_decl("close");
    bad_close_decl.receiver = Some(ReceiverKind::Borrow);
    bad_resource.methods.insert(
        "close".to_string(),
        MethodInfo {
            decl: bad_close_decl.clone(),
            signature: function_signature(Vec::new(), Type::named("int32")),
            type_param_bounds: BTreeMap::new(),
        },
    );

    let classes = BTreeMap::from([
        (
            "Counter".to_string(),
            class_info(
                "Counter",
                false,
                vec![("value", Type::named("int32"), false)],
            ),
        ),
        ("Resource".to_string(), resource),
        ("BadResource".to_string(), bad_resource),
    ]);
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let checker = checker(
        "<main>",
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &functions,
        &traits,
        &[],
        &imported_modules,
        &module_registry,
    );

    let mut locals = HashMap::from([
        (
            "counter".to_string(),
            LocalBinding {
                ty: Type::named("Counter"),
                assignable: true,
                mutable_place: true,
                managed_resource: false,
                passing: ReceiverKind::BorrowMut,
                borrow_origin: Some("counter".to_string()),
                borrow_label: None,
                match_borrow_mut_place: None,
                stale_match_borrow_mut_place: None,
                moved: false,
                moved_fields: BTreeSet::from(["value.inner".to_string()]),
                frozen_places: BTreeSet::new(),
            },
        ),
        (
            "borrowed".to_string(),
            LocalBinding {
                ty: Type::named("Counter"),
                assignable: false,
                mutable_place: false,
                managed_resource: false,
                passing: ReceiverKind::Borrow,
                borrow_origin: Some("borrowed".to_string()),
                borrow_label: None,
                match_borrow_mut_place: None,
                stale_match_borrow_mut_place: None,
                moved: false,
                moved_fields: BTreeSet::new(),
                frozen_places: BTreeSet::new(),
            },
        ),
        (
            "self".to_string(),
            LocalBinding {
                ty: Type::named("Counter"),
                assignable: false,
                mutable_place: false,
                managed_resource: false,
                passing: ReceiverKind::Borrow,
                borrow_origin: Some("self".to_string()),
                borrow_label: None,
                match_borrow_mut_place: None,
                stale_match_borrow_mut_place: None,
                moved: false,
                moved_fields: BTreeSet::new(),
                frozen_places: BTreeSet::new(),
            },
        ),
    ]);

    let member_expr = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("counter".to_string()))),
        field: "value".to_string(),
    });
    assert!(checker
        .is_mutable_place(&member_expr, &mut locals)
        .expect("member place should resolve"));
    assert_eq!(
        checker.borrow_call_place(&member_expr),
        Some("counter.value".to_string())
    );
    assert_eq!(
        checker.render_member_target(&expr(ExprKind::Name("counter".to_string())), "value"),
        "counter.value"
    );
    assert_eq!(
        checker.render_index_target(&expr(ExprKind::Name("counter".to_string()))),
        "counter[..]"
    );
    assert_eq!(
        checker.render_place_expr(&expr(ExprKind::Index {
            object: Box::new(expr(ExprKind::Name("counter".to_string()))),
            index: Box::new(expr(ExprKind::Int(0))),
        })),
        "counter[..]"
    );
    assert_eq!(
        checker.borrowed_root_binding_name(&member_expr, &locals),
        Some("counter".to_string())
    );
    assert_eq!(
        checker.borrowed_root_binding_name(&expr(ExprKind::Name("self".to_string())), &locals),
        None
    );
    assert_eq!(
        checker.member_access_path(&member_expr),
        Some(("counter".to_string(), "value".to_string()))
    );
    assert_eq!(
        checker.member_target_path(&expr(ExprKind::Name("counter".to_string())), "value"),
        Some(("counter".to_string(), "value".to_string()))
    );
    assert!(FunctionChecker::field_path_is_moved(
        locals.get("counter").unwrap(),
        "value"
    ));
    let binding = locals.get_mut("counter").unwrap();
    FunctionChecker::clear_moved_field_path(binding, "value");
    assert!(!FunctionChecker::field_path_is_moved(binding, "value"));

    let borrowed_receiver = checker
        .prepare_method_receiver_borrows(
            "touch",
            Some(ReceiverKind::Borrow),
            &member_expr,
            span,
            &mut locals,
        )
        .expect("borrowed receiver should resolve");
    assert_eq!(borrowed_receiver.len(), 1);

    let mut immutable_locals = HashMap::from([(
        "counter".to_string(),
        LocalBinding {
            ty: Type::named("Counter"),
            assignable: true,
            mutable_place: false,
            managed_resource: false,
            passing: ReceiverKind::Value,
            borrow_origin: None,
            borrow_label: None,
            match_borrow_mut_place: None,
            stale_match_borrow_mut_place: None,
            moved: false,
            moved_fields: BTreeSet::new(),
            frozen_places: BTreeSet::new(),
        },
    )]);
    let receiver_error = match checker.prepare_method_receiver_borrows(
        "touch",
        Some(ReceiverKind::BorrowMut),
        &expr(ExprKind::Name("counter".to_string())),
        span,
        &mut immutable_locals,
    ) {
        Ok(_) => panic!("mutable receiver should require mutable places"),
        Err(error) => error,
    };
    assert!(receiver_error
        .message
        .contains("requires a mutable receiver"));

    assert_eq!(
        checker
            .resolve_member_type(
                &Type::Named(
                    "MapEntry".to_string(),
                    vec![Type::named("String"), Type::named("int32")]
                ),
                "key",
                span,
            )
            .expect("MapEntry.key should resolve"),
        Type::named("String")
    );
    let map_entry_error = checker
        .resolve_member_type(
            &Type::Named(
                "MapEntry".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            ),
            "missing",
            span,
        )
        .expect_err("unknown MapEntry fields should fail");
    assert!(map_entry_error.message.contains("has no field `missing`"));

    checker
        .require_with_resource(&Type::named("TaskGroup"), span)
        .expect("TaskGroup should be allowed in with");
    checker
        .require_with_resource(&Type::named("Resource"), span)
        .expect("resource with correct close should pass");
    let generic_with = checker
        .require_with_resource(
            &Type::Named("Box".to_string(), vec![Type::named("int32")]),
            span,
        )
        .expect_err("generic resources should be rejected");
    assert!(generic_with
        .message
        .contains("does not yet support generic resource types"));
    let bad_with = checker
        .require_with_resource(&Type::named("BadResource"), span)
        .expect_err("bad close signature should fail");
    assert!(bad_with.message.contains("close(borrow mut self)"));

    checker
        .require_task_startable_function("work", &[], span)
        .expect("by-value params should be task-startable");
    let task_start_error = checker
        .require_task_startable_function(
            "work",
            &[Param {
                name: "value".to_string(),
                ty: type_ref("int32"),
                passing: ReceiverKind::Borrow,
                borrow_label: None,
                default: None,
                span,
            }],
            span,
        )
        .expect_err("borrowed params should not be task-startable");
    assert!(task_start_error
        .message
        .contains("does not yet support borrowed parameter `value`"));
}

#[test]
fn top_level_type_and_trait_helpers_cover_display_and_copy_paths() {
    let bound = TraitBound {
        trait_name: "Mapper".to_string(),
        trait_args: vec![Type::named("String"), Type::named("int32")],
    };
    assert_eq!(bound.to_string(), "Mapper[String, int32]");
    assert_eq!(
        TraitBound {
            trait_name: "Show".to_string(),
            trait_args: Vec::new(),
        }
        .to_string(),
        "Show"
    );

    assert_eq!(unary_operator_trait(UnaryOp::Neg), Some(("Neg", "neg")));
    assert_eq!(unary_operator_trait(UnaryOp::Not), Some(("Not", "not")));
    assert_eq!(binary_operator_trait(BinaryOp::Add), Some(("Add", "add")));
    assert_eq!(binary_operator_trait(BinaryOp::Div), Some(("Div", "div")));
    assert_eq!(binary_operator_trait(BinaryOp::Eq), None);
    assert_eq!(
        binary_operator_trait(BinaryOp::GreaterEq),
        Some(("Ord", "ge"))
    );

    assert_eq!(
        Type::named("int32"),
        Type::Named("int32".to_string(), Vec::new())
    );
    assert!(Type::Unit.is_copy());
    assert!(!Type::Module("pkg.tools".to_string()).is_copy());
    assert!(!Type::TypeParam("T".to_string()).is_copy());
    assert!(Type::named("float64").is_copy());
    assert!(!Type::named("String").is_copy());
    assert_eq!(Type::Unit.to_string(), "None");
    assert_eq!(
        Type::Module("pkg.tools".to_string()).to_string(),
        "module pkg.tools"
    );
    assert_eq!(Type::TypeParam("T".to_string()).to_string(), "T");
    assert_eq!(
        Type::Named("Vec".to_string(), vec![Type::named("int32")]).to_string(),
        "Vec[int32]"
    );

    let classes = BTreeMap::from([
        (
            "CopyBox".to_string(),
            class_info(
                "CopyBox",
                true,
                vec![("value", Type::named("int32"), false)],
            ),
        ),
        (
            "Thing".to_string(),
            class_info("Thing", false, vec![("name", Type::named("String"), false)]),
        ),
    ]);
    let enums = BTreeMap::from([
        (
            "MaybeInt".to_string(),
            enum_info("MaybeInt", Some(Type::named("int32"))),
        ),
        (
            "MaybeText".to_string(),
            enum_info("MaybeText", Some(Type::named("String"))),
        ),
    ]);
    assert!(type_is_copy_in_context(
        &Type::named("int32"),
        &classes,
        &enums
    ));
    assert!(type_is_copy_in_context(
        &Type::Named("Option".to_string(), vec![Type::named("int32")]),
        &classes,
        &enums
    ));
    assert!(type_is_copy_in_context(
        &Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("bool")]
        ),
        &classes,
        &enums
    ));
    assert!(type_is_copy_in_context(
        &Type::Named("SendError".to_string(), vec![Type::named("int32")]),
        &classes,
        &enums
    ));
    assert!(type_is_copy_in_context(
        &Type::named("CopyBox"),
        &classes,
        &enums
    ));
    assert!(type_is_copy_in_context(
        &Type::named("MaybeInt"),
        &classes,
        &enums
    ));
    assert!(!type_is_copy_in_context(
        &Type::named("Thing"),
        &classes,
        &enums
    ));
    assert!(!type_is_copy_in_context(
        &Type::named("MaybeText"),
        &classes,
        &enums
    ));
    assert!(!type_is_copy_in_context(
        &Type::Named("Unknown".to_string(), vec![Type::named("int32")]),
        &classes,
        &enums
    ));
}

#[test]
fn check_with_context_covers_imported_binding_registration_and_duplicate_item_paths() {
    let span = Span::new(1, 1);
    let remote_function = FunctionInfo {
        module_name: "pkg.tools".to_string(),
        decl: function_decl("remote_fn"),
        signature: function_signature(vec![Type::named("int32")], Type::named("int32")),
        type_param_bounds: BTreeMap::new(),
    };
    let remote_class = class_info(
        "RemoteBox",
        false,
        vec![("value", Type::named("int32"), false)],
    );
    let remote_enum = enum_info("RemoteStatus", Some(Type::named("int32")));
    let remote_trait = trait_info("RemoteShow", Vec::new());
    let namespace = ModuleNamespace {
        name: "tools".to_string(),
        path: "pkg.tools".to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: BTreeMap::from([("remote_fn".to_string(), remote_function.clone())]),
        classes: BTreeMap::from([("RemoteBox".to_string(), remote_class.clone())]),
        enums: BTreeMap::from([("RemoteStatus".to_string(), remote_enum.clone())]),
        traits: BTreeMap::from([("RemoteShow".to_string(), remote_trait.clone())]),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::from([("remote_fn".to_string(), remote_function.clone())]),
        all_classes: BTreeMap::from([("RemoteBox".to_string(), remote_class.clone())]),
        all_enums: BTreeMap::from([("RemoteStatus".to_string(), remote_enum.clone())]),
        all_traits: BTreeMap::from([("RemoteShow".to_string(), remote_trait.clone())]),
        imported_modules: BTreeMap::new(),
    };
    let context = ModuleContext {
        module_name: "<main>".to_string(),
        imported_bindings: BTreeMap::from([
            (
                "remote_fn".to_string(),
                ImportedBinding::Function(remote_function.clone()),
            ),
            (
                "RemoteBox".to_string(),
                ImportedBinding::Class(remote_class.clone()),
            ),
            (
                "RemoteStatus".to_string(),
                ImportedBinding::Enum(remote_enum.clone()),
            ),
            (
                "RemoteShow".to_string(),
                ImportedBinding::Trait(remote_trait.clone()),
            ),
            (
                "tools".to_string(),
                ImportedBinding::Module(namespace.clone()),
            ),
        ]),
        module_registry: BTreeMap::from([("pkg.tools".to_string(), namespace.clone())]),
    };
    let program = check_with_context(
        Module {
            imports: Vec::new(),
            items: vec![Item::Function(function_decl("main"))],
            top_level_stmts: Vec::new(),
        },
        context,
    )
    .expect("context-backed program should check");
    assert!(program.imported_modules.contains_key("tools"));
    assert!(program.module_registry.contains_key("pkg.tools"));
    assert!(program.functions.contains_key("main"));

    let duplicate_class = check_with_context(
        Module {
            imports: Vec::new(),
            items: vec![Item::Class(class_decl("RemoteBox", false, Vec::new()))],
            top_level_stmts: Vec::new(),
        },
        ModuleContext {
            module_name: "<main>".to_string(),
            imported_bindings: BTreeMap::from([(
                "RemoteBox".to_string(),
                ImportedBinding::Class(remote_class.clone()),
            )]),
            module_registry: BTreeMap::from([("pkg.tools".to_string(), namespace.clone())]),
        },
    )
    .expect_err("duplicate imported class names should fail");
    assert!(duplicate_class
        .message
        .contains("duplicate item `RemoteBox`"));

    let duplicate_enum = check_with_context(
        Module {
            imports: Vec::new(),
            items: vec![Item::Enum(EnumDecl {
                public: true,
                name: "RemoteStatus".to_string(),
                type_params: Vec::new(),
                type_param_bounds: BTreeMap::new(),
                variants: Vec::new(),
                span,
            })],
            top_level_stmts: Vec::new(),
        },
        ModuleContext {
            module_name: "<main>".to_string(),
            imported_bindings: BTreeMap::from([(
                "RemoteStatus".to_string(),
                ImportedBinding::Enum(remote_enum),
            )]),
            module_registry: BTreeMap::from([("pkg.tools".to_string(), namespace.clone())]),
        },
    )
    .expect_err("duplicate imported enum names should fail");
    assert!(duplicate_enum
        .message
        .contains("duplicate item `RemoteStatus`"));

    let duplicate_function = check_with_context(
        Module {
            imports: Vec::new(),
            items: vec![Item::Function(function_decl("remote_fn"))],
            top_level_stmts: Vec::new(),
        },
        ModuleContext {
            module_name: "<main>".to_string(),
            imported_bindings: BTreeMap::from([(
                "remote_fn".to_string(),
                ImportedBinding::Function(remote_function),
            )]),
            module_registry: BTreeMap::from([("pkg.tools".to_string(), namespace)]),
        },
    )
    .expect_err("duplicate imported function names should fail");
    assert!(duplicate_function
        .message
        .contains("duplicate item `remote_fn`"));
}

#[test]
fn check_reports_field_default_and_trait_impl_validation_errors() {
    let default_mismatch = check(
        crate::parser::parse("class Box:\n    value: int32 = \"oops\"\n")
            .expect("default mismatch snippet should parse"),
    )
    .expect_err("mismatched defaults should fail");
    assert!(default_mismatch
        .message
        .contains("default value for field `value` has type `String`, expected `int32`"));

    let unknown_trait = check(
            crate::parser::parse(
                "class Box:\n    value: int32\n\nimpl Missing for Box:\n    def show(self) -> String:\n        return \"x\"\n",
            )
            .expect("unknown-trait snippet should parse"),
        )
        .expect_err("unknown traits should fail");
    assert!(unknown_trait.message.contains("unknown trait `Missing`"));

    let trait_arity = check(
            crate::parser::parse(
                "trait Mapper[T]:\n    def map(self, value: T) -> T\n\nclass Box:\n    value: int32\n\nimpl Mapper[int32, String] for Box:\n    def map(self, value: int32) -> int32:\n        return value\n",
            )
            .expect("trait-arity snippet should parse"),
        )
        .expect_err("trait arg arity mismatches should fail");
    assert!(trait_arity
        .message
        .contains("trait `Mapper` expects exactly 1 type argument"));

    let type_param_target = check(
            crate::parser::parse(
                "trait Show:\n    def show(self) -> String\n\nimpl[T] Show for T:\n    def show(self) -> String:\n        return \"x\"\n",
            )
            .expect("type-param target snippet should parse"),
        )
        .expect_err("plain type-parameter impl targets should fail");
    assert!(type_param_target
        .message
        .contains("trait impl target must name a concrete or generic outer type"));

    let duplicate_impl = check(
            crate::parser::parse(
                "trait Show:\n    def show(self) -> String\n\nclass Box:\n    value: int32\n\nimpl Show for Box:\n    def show(self) -> String:\n        return \"a\"\n\nimpl Show for Box:\n    def show(self) -> String:\n        return \"b\"\n",
            )
            .expect("duplicate-impl snippet should parse"),
        )
        .expect_err("duplicate impls should fail");
    assert!(duplicate_impl
        .message
        .contains("duplicate impl of trait `Show` for `Box`"));

    let unknown_method = check(
            crate::parser::parse(
                "trait Show:\n    def show(self) -> String\n\nclass Box:\n    value: int32\n\nimpl Show for Box:\n    def other(self) -> String:\n        return \"x\"\n",
            )
            .expect("unknown-method snippet should parse"),
        )
        .expect_err("impl methods outside the trait should fail");
    assert!(unknown_method
        .message
        .contains("method `other` is not part of trait `Show`"));

    let receiver_mismatch = check(
            crate::parser::parse(
                "trait Show:\n    def show(borrow self) -> String\n\nclass Box:\n    value: int32\n\nimpl Show for Box:\n    def show(borrow mut self) -> String:\n        return \"x\"\n",
            )
            .expect("receiver-mismatch snippet should parse"),
        )
        .expect_err("receiver mismatches should fail");
    assert!(receiver_mismatch
        .message
        .contains("method `show` receiver does not match trait `Show`"));

    let signature_mismatch = check(
            crate::parser::parse(
                "trait Mapper[T]:\n    def map(self, value: T) -> T\n\nclass Box:\n    value: int32\n\nimpl Mapper[int32] for Box:\n    def map(self, value: String) -> String:\n        return value\n",
            )
            .expect("signature-mismatch snippet should parse"),
        )
        .expect_err("trait signature mismatches should fail");
    assert!(signature_mismatch
        .message
        .contains("method `map` in impl of `Mapper` does not match the trait signature"));

    let missing_method = check(
            crate::parser::parse(
                "trait Pairing:\n    def left(self) -> int32\n    def right(self) -> int32\n\nclass Box:\n    value: int32\n\nimpl Pairing for Box:\n    def left(self) -> int32:\n        return 1\n",
            )
            .expect("missing-method snippet should parse"),
        )
        .expect_err("missing trait methods should fail");
    assert!(missing_method
        .message
        .contains("impl of `Pairing` for `Box` is missing method `right`"));
}

#[test]
fn check_reports_duplicate_recursive_and_copy_class_errors() {
    for (source, expected) in [
            (
                "def dup() -> int32:\n    return 1\n\ndef dup() -> int32:\n    return 2\n",
                "duplicate item `dup`",
            ),
            (
                "class Box:\n    value: int32\n\nclass Box:\n    other: int32\n",
                "duplicate item `Box`",
            ),
            (
                "enum Status:\n    Ready\n\nenum Status:\n    Waiting\n",
                "duplicate item `Status`",
            ),
            (
                "trait Show:\n    def show() -> int32\n\ntrait Show:\n    def other() -> int32\n",
                "duplicate item `Show`",
            ),
            (
                "trait Show:\n    def show() -> int32\n    def show() -> int32\n",
                "duplicate method `show` in trait `Show`",
            ),
            (
                "enum Status:\n    Ready\n    Ready\n",
                "duplicate variant `Ready` in enum `Status`",
            ),
            (
                "class Counter:\n    value: int32\n    value: int32\n",
                "duplicate field `value` in class `Counter`",
            ),
            (
                "class Counter:\n    def value() -> int32:\n        return 1\n    def value() -> int32:\n        return 2\n",
                "duplicate method `value` in class `Counter`",
            ),
            (
                "class Node:\n    next: Node\n",
                "recursive field `next` on class `Node` requires `indirect`",
            ),
            (
                "copy class Holder:\n    name: String\n",
                "field `name` on `copy class Holder` must be a copy type",
            ),
        ] {
            let error = check(crate::parser::parse(source).expect("fixture should parse"))
                .expect_err("invalid program should fail checking");
            assert!(
                error.message.contains(expected),
                "expected `{expected}` in `{}`",
                error.message
            );
        }
}

#[test]
fn check_lowers_generic_top_level_items_and_impls() {
    let program = check(
            crate::parser::parse(
                "trait Mapper[T]:\n    def map(self, value: T) -> T\n\nenum Maybe[T]:\n    Some(T)\n    None\n\nclass Box[T]:\n    value: T\n    def take(self, value: T) -> T:\n        return value\n\ndef wrap[T](value: T, maybe: Maybe[T]) -> T:\n    return value\n\nimpl[T] Mapper[T] for Box[T]:\n    def map(self, value: T) -> T:\n        return value\n",
            )
            .expect("generic lowering snippet should parse"),
        )
        .expect("generic lowering snippet should type check");

    let mapper = program.traits.get("Mapper").expect("trait should exist");
    assert_eq!(mapper.decl.type_params, vec!["T".to_string()]);
    assert_eq!(
        mapper
            .methods
            .get("map")
            .expect("trait method should exist")
            .signature
            .params,
        vec![Type::TypeParam("T".to_string())]
    );

    let maybe = program.enums.get("Maybe").expect("enum should exist");
    assert_eq!(maybe.decl.type_params, vec!["T".to_string()]);
    let some_payloads = &maybe
        .variants
        .get("Some")
        .expect("Some should exist")
        .payloads;
    assert_eq!(some_payloads.len(), 1);
    assert_eq!(some_payloads[0].name, None);
    assert_eq!(some_payloads[0].ty, Type::TypeParam("T".to_string()));
    let none_payloads = &maybe
        .variants
        .get("None")
        .expect("None should exist")
        .payloads;
    assert!(none_payloads.is_empty());

    let class = program.classes.get("Box").expect("class should exist");
    assert_eq!(class.decl.type_params, vec!["T".to_string()]);
    assert_eq!(
        class.fields.get("value").expect("field should exist").ty,
        Type::TypeParam("T".to_string())
    );
    assert_eq!(
        class
            .methods
            .get("take")
            .expect("method should exist")
            .signature
            .return_type,
        Type::TypeParam("T".to_string())
    );

    let function = program
        .functions
        .get("wrap")
        .expect("function should exist");
    assert_eq!(function.decl.type_params, vec!["T".to_string()]);
    assert_eq!(
        function.signature.params,
        vec![
            Type::TypeParam("T".to_string()),
            Type::Named("Maybe".to_string(), vec![Type::TypeParam("T".to_string())]),
        ]
    );
    assert_eq!(
        function.signature.return_type,
        Type::TypeParam("T".to_string())
    );

    let impl_info = program
        .trait_impls
        .iter()
        .find(|info| info.trait_name == "Mapper")
        .expect("trait impl should exist");
    assert_eq!(impl_info.trait_args, vec![Type::TypeParam("T".to_string())]);
    assert_eq!(
        impl_info.for_type,
        Type::Named("Box".to_string(), vec![Type::TypeParam("T".to_string())])
    );
    assert_eq!(
        impl_info
            .methods
            .get("map")
            .expect("impl method should exist")
            .signature
            .return_type,
        Type::TypeParam("T".to_string())
    );
}

#[test]
fn lower_type_and_imported_context_helpers_cover_builtin_and_context_paths() {
    let mut type_names = BTreeMap::from([
        ("Pair".to_string(), Span::new(1, 1)),
        ("pkg.tools.Widget".to_string(), Span::new(1, 1)),
    ]);
    let mut type_arities = BTreeMap::from([
        ("Pair".to_string(), 2usize),
        ("pkg.tools.Widget".to_string(), 0usize),
    ]);
    let type_params = BTreeMap::from([("T".to_string(), ())]);

    assert_eq!(
        lower_type(&type_ref("str"), &type_names, &type_arities, &type_params)
            .expect("str should canonicalize"),
        Type::named("String")
    );
    assert_eq!(
        lower_type(
            &TypeRef {
                name: "pkg.tools.Widget".to_string(),
                args: Vec::new(),
                indirect: false,
                span: Span::new(1, 1),
            },
            &type_names,
            &type_arities,
            &BTreeMap::new(),
        )
        .expect("qualified imported type should lower"),
        Type::named("Widget")
    );

    for (invalid_type, expected) in [
        (
            TypeRef {
                name: "T".to_string(),
                args: vec![type_ref("int32")],
                indirect: false,
                span: Span::new(2, 1),
            },
            "type parameter `T` does not take type arguments",
        ),
        (
            nested_type_ref("None", vec![type_ref("int32")]),
            "`None` does not take generic arguments",
        ),
        (
            nested_type_ref("Option", Vec::new()),
            "`Option` expects exactly one type argument",
        ),
        (
            nested_type_ref("Result", vec![type_ref("int32")]),
            "`Result` expects exactly two type arguments",
        ),
        (
            nested_type_ref("Queue", Vec::new()),
            "`Queue` expects exactly one type argument",
        ),
        (
            nested_type_ref("Map", vec![type_ref("String")]),
            "`Map` expects exactly two type arguments",
        ),
        (
            nested_type_ref("MapEntry", vec![type_ref("String")]),
            "`MapEntry` expects exactly two type arguments",
        ),
        (
            nested_type_ref("TaskGroup", vec![type_ref("int32")]),
            "`TaskGroup` does not take type arguments",
        ),
        (
            nested_type_ref("int32", vec![type_ref("String")]),
            "`int32` does not take type arguments",
        ),
        (
            nested_type_ref("Pair", vec![type_ref("int32")]),
            "`Pair` expects exactly 2 type arguments, found 1",
        ),
        (type_ref("Missing"), "unknown type `Missing`"),
    ] {
        let error = lower_type(&invalid_type, &type_names, &type_arities, &type_params)
            .expect_err("invalid type should fail lowering");
        assert!(
            error.message.contains(expected),
            "expected `{expected}` in `{}`",
            error.message
        );
    }

    let reserved = reject_reserved_type_name("Task", Span::new(3, 1))
        .expect_err("built-in type names are reserved");
    assert!(reserved
        .message
        .contains("`Task` is a reserved built-in type name"));

    let mut child = namespace("pkg.tools.child");
    child
        .classes
        .insert("Inner".to_string(), class_info("Inner", true, Vec::new()));
    let mut imported = namespace("pkg.helpers");
    imported
        .traits
        .insert("Named".to_string(), trait_info("Named", Vec::new()));
    let mut registry_root = namespace("pkg.tools");
    registry_root.classes.insert(
        "Widget".to_string(),
        class_info("Widget", false, Vec::new()),
    );
    registry_root
        .enums
        .insert("Status".to_string(), enum_info("Status", None));
    registry_root
        .traits
        .insert("Show".to_string(), trait_info("Show", Vec::new()));
    registry_root
        .modules
        .insert("child".to_string(), child.clone());
    registry_root
        .imported_modules
        .insert("helpers".to_string(), imported.clone());
    register_module_namespace_types(&registry_root, &mut type_names, &mut type_arities);
    assert!(type_names.contains_key("pkg.tools.Widget"));
    assert!(type_names.contains_key("pkg.tools.Status"));
    assert!(type_names.contains_key("pkg.tools.Show"));
    assert!(type_names.contains_key("pkg.tools.child.Inner"));
    assert!(type_names.contains_key("pkg.helpers.Named"));

    let mut remote_function = FunctionInfo {
        module_name: "pkg.tools".to_string(),
        decl: function_decl("remote_fn"),
        signature: function_signature(Vec::new(), Type::named("int32")),
        type_param_bounds: BTreeMap::new(),
    };
    remote_function.decl.return_type = type_ref("int32");
    remote_function.decl.body = vec![Stmt::Return(crate::ast::ReturnStmt {
        value: Some(expr(ExprKind::Int(7))),
        span: Span::new(1, 1),
    })];
    let mut remote_class = class_info("Widget", false, Vec::new());
    remote_class.module_name = "pkg.tools".to_string();
    let mut remote_enum = enum_info("Status", None);
    remote_enum.module_name = "pkg.tools".to_string();
    let mut remote_trait = trait_info("Show", Vec::new());
    remote_trait.module_name = "pkg.tools".to_string();

    let program = check_with_context(
        crate::parser::parse("def main() -> int32:\n    return remote_fn()\n")
            .expect("main snippet should parse"),
        ModuleContext {
            module_name: "main".to_string(),
            imported_bindings: BTreeMap::from([
                (
                    "remote_fn".to_string(),
                    ImportedBinding::Function(remote_function),
                ),
                ("Widget".to_string(), ImportedBinding::Class(remote_class)),
                ("Status".to_string(), ImportedBinding::Enum(remote_enum)),
                ("Show".to_string(), ImportedBinding::Trait(remote_trait)),
                (
                    "tools".to_string(),
                    ImportedBinding::Module(registry_root.clone()),
                ),
            ]),
            module_registry: BTreeMap::from([("pkg.tools".to_string(), registry_root)]),
        },
    )
    .expect("imported binding kinds should seed program context");
    assert!(program.functions.contains_key("remote_fn"));
    assert!(program.classes.contains_key("Widget"));
    assert!(program.enums.contains_key("Status"));
    assert!(program.traits.contains_key("Show"));
    assert!(program.imported_modules.contains_key("tools"));
}

#[test]
fn type_copy_and_display_helpers_cover_builtin_module_and_generic_paths() {
    let program = crate::check_source(
        "\
copy class Count:
    value: int32

class HeapBox[T]:
    value: T

enum CopyState:
    Ready
    Count(int32)

enum HeapState:
    Text(String)

def main():
    pass
",
    )
    .expect("program should type-check");

    assert!(type_is_copy_in_context(
        &Type::Unit,
        &program.classes,
        &program.enums,
    ));
    assert!(!type_is_copy_in_context(
        &Type::Module("pkg.tools".to_string()),
        &program.classes,
        &program.enums,
    ));
    assert!(!type_is_copy_in_context(
        &Type::TypeParam("T".to_string()),
        &program.classes,
        &program.enums,
    ));
    assert!(type_is_copy_in_context(
        &Type::named("int32"),
        &program.classes,
        &program.enums,
    ));
    assert!(type_is_copy_in_context(
        &Type::Named("Option".to_string(), vec![Type::named("int32")]),
        &program.classes,
        &program.enums,
    ));
    assert!(!type_is_copy_in_context(
        &Type::Named("Option".to_string(), vec![Type::named("String")]),
        &program.classes,
        &program.enums,
    ));
    assert!(type_is_copy_in_context(
        &Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("bool")],
        ),
        &program.classes,
        &program.enums,
    ));
    assert!(type_is_copy_in_context(
        &Type::Named("SendError".to_string(), vec![Type::named("int32")]),
        &program.classes,
        &program.enums,
    ));
    assert!(type_is_copy_in_context(
        &Type::Named("Count".to_string(), Vec::new()),
        &program.classes,
        &program.enums,
    ));
    assert!(!type_is_copy_in_context(
        &Type::Named("HeapBox".to_string(), vec![Type::named("String")]),
        &program.classes,
        &program.enums,
    ));
    assert!(!type_is_copy_in_context(
        &Type::Named("HeapBox".to_string(), vec![Type::named("int32")]),
        &program.classes,
        &program.enums,
    ));
    assert!(type_is_copy_in_context(
        &Type::Named("CopyState".to_string(), Vec::new()),
        &program.classes,
        &program.enums,
    ));
    assert!(!type_is_copy_in_context(
        &Type::Named("HeapState".to_string(), Vec::new()),
        &program.classes,
        &program.enums,
    ));
    assert!(!type_is_copy_in_context(
        &Type::Named("Missing".to_string(), Vec::new()),
        &program.classes,
        &program.enums,
    ));

    assert_eq!(Type::Unit.to_string(), "None");
    assert_eq!(
        Type::Module("pkg.tools".to_string()).to_string(),
        "module pkg.tools"
    );
    assert_eq!(Type::TypeParam("T".to_string()).to_string(), "T");
    assert_eq!(Type::named("String").to_string(), "String");
    assert_eq!(
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        )
        .to_string(),
        "Map[String, int32]"
    );
}

#[test]
fn sema_type_helper_suite_covers_default_args_patterns_and_classifiers() {
    let param_names = vec!["value".to_string(), "fallback".to_string()];
    let referenced = default_argument_references_param(
        &expr(ExprKind::Call {
            callee: Box::new(expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Group(Box::new(expr(ExprKind::Name(
                    "value".to_string(),
                )))))),
                field: "trim".to_string(),
            })),
            args: vec![
                arg(expr(ExprKind::String("ignored".to_string()))),
                arg(expr(ExprKind::Map(vec![MapEntryExpr {
                    key: expr(ExprKind::String("k".to_string())),
                    value: expr(ExprKind::FString(vec![
                        FormatPart::Literal("".to_string()),
                        FormatPart::Expr(expr(ExprKind::Name("fallback".to_string()))),
                    ])),
                }]))),
            ],
        }),
        &param_names,
    );
    assert_eq!(referenced, Some("value".to_string()));
    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::Binary {
                left: Box::new(expr(ExprKind::Int(1))),
                op: BinaryOp::Add,
                right: Box::new(expr(ExprKind::Int(2))),
            }),
            &param_names,
        ),
        None
    );

    let merged = merge_trait_bounds(
        &BTreeMap::from([(
            "T".to_string(),
            vec![TraitBound {
                trait_name: "Show".to_string(),
                trait_args: Vec::new(),
            }],
        )]),
        &BTreeMap::from([
            (
                "T".to_string(),
                vec![TraitBound {
                    trait_name: "Cloneable".to_string(),
                    trait_args: Vec::new(),
                }],
            ),
            (
                "U".to_string(),
                vec![TraitBound {
                    trait_name: "Named".to_string(),
                    trait_args: Vec::new(),
                }],
            ),
        ]),
    );
    assert_eq!(merged.get("T").map(Vec::len), Some(2));
    assert_eq!(merged.get("U").map(Vec::len), Some(1));

    assert!(type_contains_named(
        &Type::Named(
            "Result".to_string(),
            vec![
                Type::Named("Option".to_string(), vec![Type::named("String")]),
                Type::named("int32"),
            ],
        ),
        "String",
    ));
    assert!(!type_contains_named(&Type::Unit, "String"));

    let class_program = crate::check_source(
        "\
class Target:
    value: int32

class Wrapper:
    target: Target

class IndirectWrapper:
    target: indirect Target?

def main():
    pass
",
    )
    .expect("class program should type-check");
    assert!(type_reaches_class_through_non_indirect_fields(
        &Type::named("Wrapper"),
        "Target",
        &class_program.classes,
        &mut BTreeSet::new(),
    ));
    assert!(!type_reaches_class_through_non_indirect_fields(
        &Type::named("IndirectWrapper"),
        "Target",
        &class_program.classes,
        &mut BTreeSet::new(),
    ));

    let substitutions = HashMap::from([
        ("T".to_string(), Type::named("String")),
        ("U".to_string(), Type::named("int32")),
    ]);
    assert_eq!(
        substitute_type(
            &Type::Named(
                "Map".to_string(),
                vec![
                    Type::TypeParam("T".to_string()),
                    Type::TypeParam("U".to_string())
                ],
            ),
            &substitutions,
        ),
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        )
    );
    assert_eq!(
        substitute_trait_bound(
            &TraitBound {
                trait_name: "Mapper".to_string(),
                trait_args: vec![Type::TypeParam("T".to_string())],
            },
            &substitutions,
        ),
        TraitBound {
            trait_name: "Mapper".to_string(),
            trait_args: vec![Type::named("String")],
        }
    );
    assert_eq!(
        substitute_trait_bounds(
            &BTreeMap::from([(
                "T".to_string(),
                vec![TraitBound {
                    trait_name: "Mapper".to_string(),
                    trait_args: vec![Type::TypeParam("U".to_string())],
                }],
            )]),
            &substitutions,
        )
        .get("T")
        .expect("bounds should be substituted")[0]
            .trait_args,
        vec![Type::named("int32")]
    );

    let mut collected = BTreeSet::new();
    collect_type_params_from_type(
        &Type::Named(
            "Result".to_string(),
            vec![
                Type::TypeParam("T".to_string()),
                Type::Named("Option".to_string(), vec![Type::TypeParam("U".to_string())]),
            ],
        ),
        &mut collected,
    );
    assert_eq!(
        collected,
        BTreeSet::from(["T".to_string(), "U".to_string()])
    );

    let type_params = BTreeSet::from(["T".to_string()]);
    let mut pattern_substitutions = HashMap::new();
    assert!(type_pattern_matches(
        &Type::Named("Box".to_string(), vec![Type::TypeParam("T".to_string())]),
        &Type::Named("Box".to_string(), vec![Type::named("String")]),
        &type_params,
        &mut pattern_substitutions,
    ));
    assert_eq!(pattern_substitutions.get("T"), Some(&Type::named("String")));
    assert!(!type_pattern_matches(
        &Type::Named("Box".to_string(), vec![Type::TypeParam("T".to_string())]),
        &Type::Named("Vec".to_string(), vec![Type::named("String")]),
        &type_params,
        &mut HashMap::new(),
    ));
    assert!(has_unresolved_type_params(&Type::TypeParam(
        "T".to_string()
    )));
    assert!(!has_unresolved_type_params(&Type::named("String")));
    assert_eq!(
        substitutions_from_decl_type_args(
            &["K".to_string(), "V".to_string()],
            &[Type::named("String"), Type::named("int32")],
        ),
        HashMap::from([
            ("K".to_string(), Type::named("String")),
            ("V".to_string(), Type::named("int32")),
        ])
    );

    let mut unified = HashMap::new();
    unify_type_pattern(
        &Type::Named("Box".to_string(), vec![Type::TypeParam("T".to_string())]),
        &Type::Named("Box".to_string(), vec![Type::named("int32")]),
        &mut unified,
    )
    .expect("type patterns should unify");
    assert_eq!(unified.get("T"), Some(&Type::named("int32")));
    let conflict = unify_type_pattern(
        &Type::TypeParam("T".to_string()),
        &Type::named("String"),
        &mut HashMap::from([("T".to_string(), Type::named("int32"))]),
    )
    .expect_err("conflicting substitutions should fail");
    assert!(conflict.message.contains("conflicting inferred types"));
    let mismatch = unify_type_pattern(
        &Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        &Type::named("String"),
        &mut HashMap::new(),
    )
    .expect_err("named type mismatches should fail");
    assert!(mismatch
        .message
        .contains("expected `Vec[int32]`, found `String`"));

    assert!(is_builtin_type("Vec"));
    assert!(!is_builtin_type("Widget"));
    assert!(is_integer_type(&Type::named("int32")));
    assert!(is_float_type(&Type::named("float64")));
    assert!(is_string_type(&Type::named("String")));
    assert!(is_numeric_type(&Type::named("uint64")));
    assert!(!is_numeric_type(&Type::named("String")));
    assert!(integer_type_bounds(&Type::named("int8")).is_some());
    assert!(integer_type_bounds(&Type::named("float64")).is_none());

    let duplicate_enum = crate::check_source(
        "\
enum Status:
    Ready

enum Status:
    Done

def main():
    pass
",
    )
    .expect_err("duplicate enums should be rejected");
    assert!(duplicate_enum.message.contains("duplicate item `Status`"));

    let duplicate_function = crate::check_source(
        "\
def helper():
    pass

def helper():
    pass

def main():
    pass
",
    )
    .expect_err("duplicate functions should be rejected");
    assert!(duplicate_function
        .message
        .contains("duplicate item `helper`"));

    let duplicate_trait_method = crate::check_source(
        "\
trait Show:
    def render() -> String
    def render() -> String

def main():
    pass
",
    )
    .expect_err("duplicate trait methods should be rejected");
    assert!(duplicate_trait_method
        .message
        .contains("duplicate method `render` in trait `Show`"));

    let duplicate_variant = crate::check_source(
        "\
enum Status:
    Ready
    Ready

def main():
    pass
",
    )
    .expect_err("duplicate variants should be rejected");
    assert!(duplicate_variant
        .message
        .contains("duplicate variant `Ready` in enum `Status`"));

    let duplicate_field = crate::check_source(
        "\
class Box:
    value: int32
    value: int32

def main():
    pass
",
    )
    .expect_err("duplicate fields should be rejected");
    assert!(duplicate_field
        .message
        .contains("duplicate field `value` in class `Box`"));

    let duplicate_method = crate::check_source(
        "\
class Box:
    def render() -> String:
        return \"a\"

    def render() -> String:
        return \"b\"

def main():
    pass
",
    )
    .expect_err("duplicate methods should be rejected");
    assert!(duplicate_method
        .message
        .contains("duplicate method `render` in class `Box`"));

    let bad_field_default = crate::check_source(
        "\
class Counter:
    value: int32 = \"zero\"

def main():
    pass
",
    )
    .expect_err("mismatched field defaults should be rejected");
    assert!(bad_field_default
        .message
        .contains("default value for field `value` has type `String`, expected `int32`"));

    let mixed_top_level = crate::check_source(
        "\
print(1)

def main():
    pass
",
    )
    .expect_err("top-level statements and main should not mix");
    assert!(mixed_top_level.message.contains(
        "files cannot mix top-level statements, including declarations, with an explicit `main` function"
    ));

    let main_params = crate::check_source(
        "\
def main(value: int32):
    pass
",
    )
    .expect_err("main parameters should be rejected");
    assert!(main_params
        .message
        .contains("`main` must not take parameters in the bootstrap runtime"));

    let unknown_trait_impl = crate::check_source(
        "\
class Box:
    value: int32

impl Missing for Box:
    def render() -> String:
        return \"x\"

def main():
    pass
",
    )
    .expect_err("unknown impl traits should be rejected");
    assert!(unknown_trait_impl
        .message
        .contains("unknown trait `Missing`"));

    let trait_arity_mismatch = crate::check_source(
        "\
trait Mapper[T]:
    def map(value: T) -> T

class Box:
    value: int32

impl Mapper for Box:
    def map(value: int32) -> int32:
        return value

def main():
    pass
",
    )
    .expect_err("trait impl arity mismatches should be rejected");
    assert!(trait_arity_mismatch
        .message
        .contains("expects exactly 1 type argument"));

    let trait_impl_target_type_param = crate::check_source(
        "\
trait Show:
    def render() -> String

impl[T] Show for T:
    def render() -> String:
        return \"x\"

def main():
    pass
",
    )
    .expect_err("impl targets cannot be bare type params");
    assert!(trait_impl_target_type_param
        .message
        .contains("trait impl target must name a concrete or generic outer type"));

    let duplicate_trait_impl = crate::check_source(
        "\
trait Show:
    def render() -> String

class Box:
    value: int32

impl Show for Box:
    def render() -> String:
        return \"x\"

impl Show for Box:
    def render() -> String:
        return \"y\"

def main():
    pass
",
    )
    .expect_err("duplicate trait impls should be rejected");
    assert!(duplicate_trait_impl
        .message
        .contains("duplicate impl of trait `Show` for `Box`"));

    let trait_impl_unknown_method = crate::check_source(
        "\
trait Show:
    def render() -> String

class Box:
    value: int32

impl Show for Box:
    def missing() -> String:
        return \"x\"

def main():
    pass
",
    )
    .expect_err("unknown impl methods should be rejected");
    assert!(trait_impl_unknown_method
        .message
        .contains("method `missing` is not part of trait `Show`"));

    let trait_impl_receiver_mismatch = crate::check_source(
        "\
trait Show:
    def render() -> String

class Box:
    value: int32

impl Show for Box:
    def render(borrow self) -> String:
        return \"x\"

def main():
    pass
",
    )
    .expect_err("receiver mismatches should be rejected");
    assert!(trait_impl_receiver_mismatch
        .message
        .contains("receiver does not match trait `Show`"));

    let trait_impl_signature_mismatch = crate::check_source(
        "\
trait Show:
    def render() -> String

class Box:
    value: int32

impl Show for Box:
    def render() -> int32:
        return 1

def main():
    pass
",
    )
    .expect_err("trait impl signatures should match");
    assert!(trait_impl_signature_mismatch
        .message
        .contains("does not match the trait signature"));

    let trait_impl_missing_method = crate::check_source(
        "\
trait Show:
    def render() -> String
    def label() -> String

class Box:
    value: int32

impl Show for Box:
    def render() -> String:
        return \"x\"

def main():
    pass
",
    )
    .expect_err("missing trait impl methods should be rejected");
    assert!(trait_impl_missing_method
        .message
        .contains("is missing method `label`"));
}
