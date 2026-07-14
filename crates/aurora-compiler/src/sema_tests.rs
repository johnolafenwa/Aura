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

#[test]
fn d3_int_alias_canonicalizes_across_signatures_generics_and_casts() {
    let source = r#"
def identity[T](value: T) -> T:
    return value

def round_trip(value: int, values: Vec[int]) -> int:
    casted: int = value as int
    return identity[int](value=casted)

def main() -> int32:
    values: Vec[int] = [2147483648]
    print(round_trip(1, values))
    return 0
"#;

    let program = crate::check_source(source).expect("the int alias surface should type check");
    let round_trip = program
        .functions
        .get("round_trip")
        .expect("round_trip should be registered");
    assert_eq!(
        round_trip.signature.params,
        vec![
            Type::named("int64"),
            Type::Named("Vec".to_string(), vec![Type::named("int64")]),
        ]
    );
    assert_eq!(round_trip.signature.return_type, Type::named("int64"));

    assert_eq!(
        lower_type(
            &type_ref("int"),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .expect("int should lower as a built-in alias"),
        Type::named("int64")
    );
}

#[test]
fn d3_unhinted_integer_literals_default_to_checked_int64() {
    let source = r#"
def accept(value: int64):
    print(value)

def accept_values(values: Vec[int64]):
    print(values.len())

def accept_option(value: Option[int64]):
    print(value != Option.None)

def main() -> int32:
    positive = 2147483648
    negative = -2147483649
    values = [1, 2147483648]
    maybe = Option.Some(1)
    accept(positive)
    accept(negative)
    accept_values(values)
    accept_option(maybe)
    return 0
"#;
    crate::check_source(source).expect("unhinted integer expressions should infer int64");

    for (literal, expected_message) in [
        (
            "9223372036854775808",
            "integer literal `9223372036854775808` does not fit in `int64`",
        ),
        (
            "-9223372036854775809",
            "integer literal `-9223372036854775809` does not fit in `int64`",
        ),
    ] {
        let source = format!("def main():\n    value = {literal}\n");
        let error = crate::check_source(&source)
            .expect_err("an unhinted literal outside int64 must be rejected");
        assert_eq!(error.message, expected_message);
    }

    crate::check_source(
        "def main() -> int32:\n    positive: int128 = 9223372036854775808\n    negative: int128 = -9223372036854775809\n    narrow: int32 = 2147483647\n    return 0\n",
    )
    .expect("explicit wider and fixed-width integer contexts should remain authoritative");
}

#[test]
fn d3_fixed_int32_builtin_and_index_positions_contextually_type_literals() {
    crate::check_source(
        r#"
def main() -> int32:
    mut values: Vec[int32] = [1]
    values[0] = 2
    print(values[0])
    print(values.get(index=0))
    jobs = Queue[int32](capacity=4)
    for value in range(stop=4):
        print(value)
    for value in range(1, 4):
        print(value)
    jobs.close()
    return 0
"#,
    )
    .expect("fixed int32 APIs should contextually type ordinary literals");

    for statement in ["print(values[2147483648])", "values[2147483648] = 1"] {
        let source = format!(
            "def main() -> int32:\n    mut values: Vec[int32] = [1]\n    {statement}\n    return 0\n"
        );
        let error = crate::check_source(&source)
            .expect_err("vector index literals must remain constrained to int32");
        assert_eq!(
            error.message,
            "integer literal `2147483648` does not fit in `int32`"
        );
    }
}

#[test]
fn d3_vec_index_contract_rejects_default_int64_variables() {
    for statement in [
        "print(values[index])",
        "values[index] = 2",
        "print(values.get(index))",
        "values.set(index, 2)",
        "values.remove(index)",
        "values.swap(index, 0)",
        "values.insert(index, 2)",
    ] {
        let source = format!(
            "def main() -> int32:\n    mut values: Vec[int32] = [1]\n    index = 0\n    {statement}\n    return 0\n"
        );
        let error = crate::check_source(&source)
            .expect_err("default int64 variables must not enter fixed int32 index positions");
        assert_eq!(
            error.message, "vector indices must have type `int32`, found `int64`",
            "unexpected diagnostic for `{statement}`"
        );
    }
}

#[test]
fn d3_generic_calls_use_expected_results_to_contextually_type_literal_arguments() {
    crate::check_source(
        r#"
def identity[T](value: T) -> T:
    return value

def main() -> int32:
    positional: int32 = identity(100)
    named: int32 = identity(value=5)
    defaulted: int64 = identity(2147483648)
    print(positional)
    print(named)
    print(defaulted)
    return 0
"#,
    )
    .expect("the expected generic result should contextually type literal arguments");
}

#[test]
fn d3_int_alias_is_a_reserved_builtin_type_name() {
    let direct = reject_reserved_type_name("int", Span::new(1, 1))
        .expect_err("the int alias should be reserved");
    assert_eq!(direct.message, "`int` is a reserved built-in type name");

    let error = crate::check_source("class int:\n    value: int32\n")
        .expect_err("user types cannot shadow the int alias");
    assert_eq!(error.message, "`int` is a reserved built-in type name");
}

fn projection_path(path: &str) -> ProjectionPath {
    if path.is_empty() {
        return ProjectionPath::default();
    }
    ProjectionPath(
        path.split('.')
            .map(|field| PlaceProjection::Field(field.to_string()))
            .collect(),
    )
}

fn place_path(path: &str) -> PlacePath {
    let mut segments = path.split('.');
    let root = segments.next().expect("test place paths require a root");
    PlacePath {
        root: root.to_string(),
        projections: ProjectionPath(
            segments
                .map(|field| PlaceProjection::Field(field.to_string()))
                .collect(),
        ),
    }
}

#[test]
fn contextual_none_equality_type_checks_symmetrically() {
    let source = include_str!("../tests/fixtures/check-pass/contextual_none_positions.au");
    crate::check_source(source).expect("contextual None positions should type-check");
}

#[test]
fn contextual_none_rejects_non_optional_comparisons_symmetrically() {
    for expression in ["value == None", "None != value"] {
        let source = format!("def invalid(value: int32) -> bool:\n    return {expression}\n");
        let error = crate::check_source(&source)
            .expect_err("None comparisons with non-optional values should fail");
        assert_eq!(
            error.message,
            "type `int32` is not optional; only `Option[T]` values can be compared with `None`"
        );
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
            .map(|field| projection_path(field))
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
    assert_eq!(
        render_literal_pattern_key(&LiteralPatternKey::Float(1.5f64.to_bits())),
        "1.5"
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

    let recursive_classes = BTreeMap::from([
        (
            "A".to_string(),
            class_info("A", false, vec![("b", Type::named("B"), false)]),
        ),
        (
            "B".to_string(),
            class_info("B", false, vec![("a", Type::named("A"), false)]),
        ),
    ]);
    assert!(!type_reaches_class_through_non_indirect_fields(
        &Type::named("A"),
        "Missing",
        &recursive_classes,
        &mut BTreeSet::new(),
    ));

    let mut broken = class_info(
        "Broken",
        false,
        vec![("lost", Type::named("Missing"), false)],
    );
    broken.fields.clear();
    assert!(!type_reaches_class_through_non_indirect_fields(
        &Type::named("Broken"),
        "Missing",
        &BTreeMap::from([("Broken".to_string(), broken)]),
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
    let empty_type_params = BTreeMap::new();
    assert_eq!(
        lower_type(
            &type_ref("TaskGroup"),
            &type_names,
            &type_arities,
            &empty_type_params
        )
        .expect("TaskGroup should lower without type arguments"),
        Type::named("TaskGroup")
    );
    assert_eq!(
        lower_type(
            &type_ref("Duration"),
            &type_names,
            &type_arities,
            &empty_type_params
        )
        .expect("Duration should lower without type arguments"),
        Type::named("Duration")
    );

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
    assert!(type_pattern_matches(
        &Type::TypeParam("U".to_string()),
        &Type::TypeParam("U".to_string()),
        &type_params,
        &mut HashMap::new(),
    ));
    assert_eq!(type_pattern_specificity(&Type::Unit), 1);
    assert_eq!(
        type_pattern_specificity(&Type::Module("helpers.math".to_string())),
        1
    );
    assert!(has_unresolved_type_params(&Type::Named(
        "Option".to_string(),
        vec![Type::TypeParam("T".to_string())],
    )));
    assert!(!has_unresolved_type_params(&Type::Unit));
    assert!(!has_unresolved_type_params(&Type::Module(
        "helpers.math".to_string()
    )));
    assert_eq!(
        substitute_type(&Type::Module("helpers.math".to_string()), &HashMap::new()),
        Type::Module("helpers.math".to_string())
    );
    let mut collected = BTreeSet::new();
    collect_type_params_from_type(&Type::Unit, &mut collected);
    collect_type_params_from_type(&Type::Module("helpers.math".to_string()), &mut collected);
    assert!(collected.is_empty());

    unify_type_pattern(&Type::Unit, &Type::Unit, &mut HashMap::new())
        .expect("unit patterns should unify with unit");
    unify_type_pattern(
        &Type::Module("helpers.math".to_string()),
        &Type::Module("helpers.math".to_string()),
        &mut HashMap::new(),
    )
    .expect("module patterns should unify with matching modules");

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
    let named_against_unit =
        unify_type_pattern(&Type::named("String"), &Type::Unit, &mut HashMap::new())
            .expect_err("named patterns should reject non-named actual types");
    assert!(named_against_unit
        .message
        .contains("expected `String`, found `None`"));
}

#[test]
fn checker_expression_helper_paths_cover_collection_specialization_and_control_edges() {
    let program = crate::check_source(
            "class Counter:\n    value: int32\n\nclass Holder[T]:\n    value: T\n\nclass PairBox[A, B]:\n    left: A\n    right: B\n\nclass Flag:\n    value: bool\n\nenum Maybe[T]:\n    Value(T)\n    Empty\n\nenum Pair[A, B]:\n    Empty\n\ndef work(value: int32) -> int32:\n    return value\n\ndef main():\n    pass\n",
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
            "unit_value".to_string(),
            local_binding(Type::Unit, false, false, ReceiverKind::Value, false, &[]),
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
        (
            "unit_tasks".to_string(),
            local_binding(
                Type::Named("Vec".to_string(), vec![Type::Unit]),
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
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Set".to_string()))),
                    type_args: vec![type_ref("int32")],
                }),
                &mut locals,
            )
            .expect("Set specialization should preserve its explicit element type"),
        Type::Named("Set".to_string(), vec![Type::named("int32")])
    );
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
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Map".to_string()))),
                    type_args: vec![type_ref("String"), type_ref("int32")],
                }),
                &mut locals,
            )
            .expect("Map specialization should preserve explicit key/value types"),
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")]
        )
    );
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
                expr: Box::new(expr(ExprKind::Name("PairBox".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            &mut locals,
        )
        .expect_err("plural class arity diagnostics should be covered")
        .message
        .contains("class `PairBox` expects 2 type arguments"));
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Holder".to_string()))),
                    type_args: vec![type_ref("int32")],
                }),
                &mut locals,
            )
            .expect("generic class specialization should lower explicit type args"),
        Type::Named("Holder".to_string(), vec![Type::named("int32")])
    );
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
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Pair".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            &mut locals,
        )
        .expect_err("plural enum arity diagnostics should be covered")
        .message
        .contains("enum `Pair` expects 2 type arguments"));
    assert_eq!(
        checker
            .type_of_expr(
                &expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Maybe".to_string()))),
                    type_args: vec![type_ref("String")],
                }),
                &mut locals,
            )
            .expect("generic enum specialization should lower explicit type args"),
        Type::Named("Maybe".to_string(), vec![Type::named("String")])
    );
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
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("wait_any".to_string()))),
                args: vec![arg(expr(ExprKind::Name("unit_value".to_string())))],
            }),
            &mut locals,
        )
        .expect_err("wait_any should reject non-container task arguments")
        .message
        .contains("`wait_any` expects `Vec[Task[T]]`, found `None`"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("wait_all".to_string()))),
                args: vec![arg(expr(ExprKind::Name("unit_tasks".to_string())))],
            }),
            &mut locals,
        )
        .expect_err("wait_all should reject Vec[None] task containers")
        .message
        .contains("`wait_all` expects `Vec[Task[T]]`, found `Vec[None]`"));
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("wait_any".to_string()))),
                args: vec![arg(expr(ExprKind::Name("words".to_string())))],
            }),
            &mut locals,
        )
        .expect_err("wait_any should reject non-task vector elements")
        .message
        .contains("`wait_any` expects `Vec[Task[T]]`, found `Vec[String]`"));

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
    checker.current_return_type = Some(Type::Unit);
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                "result_value".to_string(),
            ))))),
            &mut locals,
        )
        .expect_err("non-named returns should fail")
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
    assert!(checker
        .type_of_expr(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                "unit_value".to_string(),
            ))))),
            &mut locals,
        )
        .expect_err("try requires named Result expressions")
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
                    expr: Box::new(expr(ExprKind::Name("Pair".to_string()))),
                    type_args: vec![type_ref("int32")],
                })),
                field: "Empty".to_string(),
            }),
            &mut locals,
        )
        .expect_err("plural enum arity diagnostics should be covered on members")
        .message
        .contains("enum `Pair` expects 2 type arguments"));
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
        .contains("vector indices must have type `int32`"));
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
        .contains("vector indices must have type `int32`"));

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

    let mut locals = HashMap::new();
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
        .contains("binding `fresh` has annotated type `String`, but value has type `int64`"));
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
    assert_eq!(fresh.ty, Type::named("int64"));
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
        (
            "numbers".to_string(),
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
            "generic_tasks".to_string(),
            local_binding(
                Type::Named("Vec".to_string(), vec![Type::TypeParam("T".to_string())]),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "queued_tasks".to_string(),
            local_binding(
                Type::Named(
                    "Queue".to_string(),
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
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("TaskGroup".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            &[],
            span,
            &mut locals,
            None,
        )
        .expect_err("TaskGroup should reject explicit type args")
        .message
        .contains("does not take type arguments"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("TaskGroup".to_string()))),
                type_args: Vec::new(),
            }),
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("TaskGroup should reject constructor args")
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
            &expr(ExprKind::Name("wait_any".to_string())),
            &[arg(expr(ExprKind::Name("queued_tasks".to_string())))],
            span,
            &mut locals,
            None,
        )
        .expect_err("wait_any() requires Vec rather than Queue")
        .message
        .contains("expects `Vec[Task[T]]`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("wait_any".to_string())),
            &[arg(expr(ExprKind::Name("generic_tasks".to_string())))],
            span,
            &mut locals,
            None,
        )
        .expect_err("wait_any() requires Vec[Task[T]], not Vec[T]")
        .message
        .contains("expects `Vec[Task[T]]`"));
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("wait_any".to_string())),
            &[arg(expr(ExprKind::Name("numbers".to_string())))],
            span,
            &mut locals,
            None,
        )
        .expect_err("wait_any() requires task elements")
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
        Type::Named("Box".to_string(), vec![Type::named("int64")])
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
        Type::Named("Box".to_string(), vec![Type::named("int64")])
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
        .contains("field `value` expects `String`, found `int64`"));
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
fn checker_user_call_argument_mismatch_reports_direct_callable_mismatch() {
    let error = crate::check_source(
        r#"
def takes_count(value: int32) -> None:
    pass

def main() -> None:
    takes_count("bad")
"#,
    )
    .expect_err("ordinary callable arguments should enforce declared parameter types");

    assert!(error.message.contains(
        "argument type mismatch for function `takes_count`: expected `int32`, found `String`"
    ));
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

    let mut shape = enum_info("Shape", None);
    shape.decl.variants = vec![crate::ast::EnumVariantDecl {
        name: "Point".to_string(),
        payloads: vec![
            crate::ast::EnumPayloadFieldDecl {
                name: Some("x".to_string()),
                ty: type_ref("int32"),
                span,
            },
            crate::ast::EnumPayloadFieldDecl {
                name: Some("y".to_string()),
                ty: type_ref("int32"),
                span,
            },
        ],
        named_payloads: true,
        span,
    }];
    shape.variants = BTreeMap::from([(
        "Point".to_string(),
        EnumVariantInfo {
            payloads: vec![
                EnumPayloadFieldInfo {
                    name: Some("x".to_string()),
                    ty: Type::named("int32"),
                    span,
                },
                EnumPayloadFieldInfo {
                    name: Some("y".to_string()),
                    ty: Type::named("int32"),
                    span,
                },
            ],
            named_payloads: true,
            span,
        },
    )]);

    let classes = BTreeMap::from([
        ("Widget".to_string(), widget),
        ("SecretBox".to_string(), secret_box),
    ]);
    let enums = BTreeMap::from([("Shape".to_string(), shape), ("Status".to_string(), status)]);
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
    let type_names = BTreeMap::from([
        ("Widget".to_string(), span),
        ("SecretBox".to_string(), span),
        ("Shape".to_string(), span),
        ("Status".to_string(), span),
    ]);
    let type_arities = BTreeMap::from([
        ("Widget".to_string(), 0usize),
        ("SecretBox".to_string(), 0usize),
        ("Shape".to_string(), 0usize),
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
    let mut locals = HashMap::from([
        (
            "flag".to_string(),
            local_binding(
                Type::named("bool"),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "widget".to_string(),
            local_binding(
                Type::named("Widget"),
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
                object: Box::new(expr(ExprKind::Name("widget".to_string()))),
                field: "build".to_string(),
            }),
            &[],
            span,
            &mut locals,
            None,
        )
        .expect_err("associated methods should require class-name calls")
        .message
        .contains(
            "associated method `build` on class `Widget` must be called through the class name"
        ));
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
    assert!(checker
        .type_of_call(
            &expr(ExprKind::Name("Widget".to_string())),
            &[arg(expr(ExprKind::Int(1))), arg(expr(ExprKind::Int(2)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("class constructors should reject extra positional arguments")
        .message
        .contains("received too many positional arguments"));

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

    assert!(checker
        .type_of_expr(&status_value, &mut locals)
        .expect_err("payload variants used as values should require construction")
        .message
        .contains("requires a payload"));
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
            &[arg(expr(ExprKind::Int(1))), arg(expr(ExprKind::Int(2)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("single-payload variants should reject extra payloads")
        .message
        .contains("expects 1 payload argument"));
    assert!(checker
        .type_of_call(
            &specialized_status_value,
            &[named_arg("item", expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("single-payload variants should only accept value=")
        .message
        .contains("only accepts the keyword `value=`"));
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

    let shape_point = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("Shape".to_string()))),
        field: "Point".to_string(),
    });
    assert_eq!(
        checker
            .type_of_call(
                &shape_point,
                &[
                    named_arg("x", expr(ExprKind::Int(1))),
                    named_arg("y", expr(ExprKind::Int(2))),
                ],
                span,
                &mut locals,
                None,
            )
            .expect("named enum payload constructors should type check"),
        Type::named("Shape")
    );
    assert_eq!(
        checker
            .type_of_call(
                &shape_point,
                &[arg(expr(ExprKind::Int(1))), arg(expr(ExprKind::Int(2)))],
                span,
                &mut locals,
                None,
            )
            .expect("named enum payloads should still accept positional construction"),
        Type::named("Shape")
    );
    assert!(checker
        .type_of_call(
            &shape_point,
            &[named_arg("x", expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("named enum payloads should require all fields")
        .message
        .contains("expects 2 payload arguments, found 1"));
    assert!(checker
        .type_of_call(
            &shape_point,
            &[
                named_arg("x", expr(ExprKind::Int(1))),
                named_arg("z", expr(ExprKind::Int(2))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("named enum payloads should require declared names")
        .message
        .contains("is missing payload argument `y`"));
    assert!(checker
        .type_of_call(
            &shape_point,
            &[
                named_arg("x", expr(ExprKind::Int(1))),
                named_arg("y", expr(ExprKind::Int(2))),
                named_arg("z", expr(ExprKind::Int(3))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("named enum payloads should reject extra names after required fields")
        .message
        .contains("has no payload named `z`"));
    assert!(checker
        .type_of_call(
            &shape_point,
            &[
                named_arg("x", expr(ExprKind::Int(1))),
                named_arg("y", expr(ExprKind::Bool(true))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("named enum payloads should enforce field types")
        .message
        .contains("expects `int32`, found `bool`"));
    assert!(checker
        .type_of_call(
            &status_ready,
            &[arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            Some(&Type::Named(
                "Status".to_string(),
                vec![Type::named("int32")]
            )),
        )
        .expect_err("payload-free variants should reject arguments")
        .message
        .contains("does not take a payload"));

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
    let inferred_option_some = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("Option".to_string()))),
        field: "Some".to_string(),
    });
    let inferred_option_none = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("Option".to_string()))),
        field: "None".to_string(),
    });
    assert!(checker
        .type_of_call(&inferred_option_some, &[], span, &mut locals, None)
        .expect_err("unqualified Option.Some should still require a payload")
        .message
        .contains("expects 1 payload argument, found 0"));
    assert!(checker
        .type_of_expr(&inferred_option_none, &mut locals)
        .expect_err("bare Option.None needs an expected type")
        .message
        .contains("cannot infer type parameter `T`"));
    assert!(checker
        .type_of_expr_hint(
            &inferred_option_some,
            &mut locals,
            Some(&Type::Named(
                "Option".to_string(),
                vec![Type::named("int32")]
            )),
        )
        .expect_err("payload-bearing builtin variants should not be used as values")
        .message
        .contains("requires a payload"));
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
    let vec_bool = Type::Named("Vec".to_string(), vec![bool_ty.clone()]);
    let bytes_ty = Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
    let headers_ty = Type::Named(
        "Map".to_string(),
        vec![string_ty.clone(), string_ty.clone()],
    );
    let map_ty = Type::Named("Map".to_string(), vec![string_ty.clone(), int_ty.clone()]);
    let set_ty = Type::Named("Set".to_string(), vec![string_ty.clone()]);
    let channel_ty = Type::Named("Queue".to_string(), vec![string_ty.clone()]);
    let task_ty = Type::Named("Task".to_string(), vec![int_ty.clone()]);
    let result_ty = |ok: Type| {
        Type::Named(
            "Result".to_string(),
            vec![ok, crate::builtin_modules::io_error_type()],
        )
    };
    let process_result_ty = |ok: Type| {
        Type::Named(
            "Result".to_string(),
            vec![ok, crate::builtin_modules::process_error_type()],
        )
    };
    let option_ty = |inner: Type| Type::Named("Option".to_string(), vec![inner]);
    let bytes_expr = || expr(ExprKind::List(vec![expr(ExprKind::Int(1))]));
    let headers_expr = || {
        expr(ExprKind::Map(vec![MapEntryExpr {
            key: expr(ExprKind::String("content-type".to_string())),
            value: expr(ExprKind::String("text/plain".to_string())),
        }]))
    };
    let timeout_arg = || arg(expr(ExprKind::DurationMillis(1)));
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
            "bad_bytes".to_string(),
            local_binding(vec_bool, false, false, ReceiverKind::Value, false, &[]),
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
    for (name, ty) in [
        ("tcp_listener", Type::named("net.TcpListener")),
        ("tcp_stream", Type::named("net.TcpStream")),
        ("udp_socket", Type::named("net.UdpSocket")),
        ("udp_datagram", Type::named("net.UdpDatagram")),
        ("http_listener", Type::named("net.HttpListener")),
        ("http_exchange", Type::named("net.HttpExchange")),
        ("http_response", Type::named("net.HttpResponse")),
        ("websocket_listener", Type::named("net.WebSocketListener")),
        ("websocket", Type::named("net.WebSocket")),
        ("unix_listener", Type::named("net.UnixListener")),
        ("unix_stream", Type::named("net.UnixStream")),
        ("tls_listener", Type::named("net.TlsListener")),
        ("tls_stream", Type::named("net.TlsStream")),
        ("child", Type::named("process.Child")),
        ("pipe", Type::named("process.Pipe")),
        ("completed", Type::named("process.Completed")),
        ("supervisor", Type::named("process.Supervisor")),
    ] {
        locals.insert(
            name.to_string(),
            local_binding(ty, false, false, ReceiverKind::Value, false, &[]),
        );
    }
    locals.insert(
        "file".to_string(),
        local_binding(
            Type::named("fs.File"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &[],
        ),
    );

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
                field: "contains".to_string(),
            }),
            vec![arg(expr(ExprKind::String("ur".to_string())))],
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "starts_with".to_string(),
            }),
            vec![arg(expr(ExprKind::String("au".to_string())))],
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "ends_with".to_string(),
            }),
            vec![arg(expr(ExprKind::String("ra".to_string())))],
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "join".to_string(),
            }),
            vec![arg(expr(ExprKind::List(vec![
                expr(ExprKind::String("left".to_string())),
                expr(ExprKind::String("right".to_string())),
            ])))],
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "strip_prefix".to_string(),
            }),
            vec![arg(expr(ExprKind::String("au".to_string())))],
            Type::Named("Option".to_string(), vec![string_ty.clone()]),
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
                field: "clone".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
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
                field: "clone".to_string(),
            }),
            Vec::new(),
            vec_int.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "push".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "pop".to_string(),
            }),
            Vec::new(),
            Type::Named("Option".to_string(), vec![int_ty.clone()]),
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
                field: "remove".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(0)))],
            Type::Named("Option".to_string(), vec![int_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "swap".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(0))), arg(expr(ExprKind::Int(1)))],
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "extend".to_string(),
            }),
            vec![arg(expr(ExprKind::List(vec![expr(ExprKind::Int(2))])))],
            Type::Unit,
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
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "reverse".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "len".to_string(),
            }),
            Vec::new(),
            int_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "is_empty".to_string(),
            }),
            Vec::new(),
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "clone".to_string(),
            }),
            Vec::new(),
            map_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "get".to_string(),
            }),
            vec![arg(expr(ExprKind::String("count".to_string())))],
            Type::Named("Option".to_string(), vec![int_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "set".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("count".to_string()))),
                arg(expr(ExprKind::Int(1))),
            ],
            Type::Named("Option".to_string(), vec![int_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "remove".to_string(),
            }),
            vec![arg(expr(ExprKind::String("count".to_string())))],
            Type::Named("Option".to_string(), vec![int_ty.clone()]),
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
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "extend".to_string(),
            }),
            vec![arg(expr(ExprKind::Map(vec![MapEntryExpr {
                key: expr(ExprKind::String("next".to_string())),
                value: expr(ExprKind::Int(2)),
            }])))],
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
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "clone".to_string(),
            }),
            Vec::new(),
            set_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "insert".to_string(),
            }),
            vec![arg(expr(ExprKind::String("name".to_string())))],
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "remove".to_string(),
            }),
            vec![arg(expr(ExprKind::String("name".to_string())))],
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "try_put".to_string(),
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
                field: "get".to_string(),
            }),
            Vec::new(),
            Type::Named("QueueReceive".to_string(), vec![string_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "get_or_none".to_string(),
            }),
            Vec::new(),
            Type::Named("Option".to_string(), vec![string_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "get_or".to_string(),
            }),
            vec![arg(expr(ExprKind::String("fallback".to_string())))],
            string_ty.clone(),
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
                object: Box::new(expr(ExprKind::Name("task".to_string()))),
                field: "result_or_none".to_string(),
            }),
            Vec::new(),
            Type::Named("Option".to_string(), vec![int_ty.clone()]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("task".to_string()))),
                field: "result_or".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(0)))],
            int_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("group".to_string()))),
                field: "cancel".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("file".to_string()))),
                field: "read_all".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("file".to_string()))),
                field: "read_bytes".to_string(),
            }),
            Vec::new(),
            result_ty(bytes_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("file".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![arg(expr(ExprKind::String("ok".to_string())))],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("file".to_string()))),
                field: "write_bytes".to_string(),
            }),
            vec![arg(bytes_expr())],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("file".to_string()))),
                field: "flush".to_string(),
            }),
            Vec::new(),
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("file".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "stdin".to_string(),
            }),
            Vec::new(),
            option_ty(Type::named("process.Pipe")),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "stdout".to_string(),
            }),
            Vec::new(),
            option_ty(Type::named("process.Pipe")),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "stderr".to_string(),
            }),
            Vec::new(),
            option_ty(Type::named("process.Pipe")),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "wait".to_string(),
            }),
            vec![timeout_arg()],
            Type::named("process.Wait"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "wait_or_none".to_string(),
            }),
            vec![timeout_arg()],
            process_result_ty(option_ty(Type::named("process.ExitStatus"))),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "wait_ok".to_string(),
            }),
            vec![timeout_arg()],
            process_result_ty(Type::named("process.ExitStatus")),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "kill".to_string(),
            }),
            Vec::new(),
            process_result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "terminate".to_string(),
            }),
            Vec::new(),
            process_result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "read_all".to_string(),
            }),
            Vec::new(),
            process_result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "read_line".to_string(),
            }),
            vec![timeout_arg()],
            process_result_ty(option_ty(string_ty.clone())),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "read_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), timeout_arg()],
            process_result_ty(option_ty(bytes_ty.clone())),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![arg(expr(ExprKind::String("ok".to_string()))), timeout_arg()],
            process_result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "write_bytes".to_string(),
            }),
            vec![arg(bytes_expr()), timeout_arg()],
            process_result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "flush".to_string(),
            }),
            Vec::new(),
            process_result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("completed".to_string()))),
                field: "status".to_string(),
            }),
            Vec::new(),
            Type::named("process.ExitStatus"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("completed".to_string()))),
                field: "success".to_string(),
            }),
            Vec::new(),
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("completed".to_string()))),
                field: "stdout".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("completed".to_string()))),
                field: "stderr".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("completed".to_string()))),
                field: "stdout_bytes".to_string(),
            }),
            Vec::new(),
            bytes_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("completed".to_string()))),
                field: "stderr_bytes".to_string(),
            }),
            Vec::new(),
            bytes_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("completed".to_string()))),
                field: "check".to_string(),
            }),
            Vec::new(),
            process_result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
            ],
            process_result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "wait".to_string(),
            }),
            vec![timeout_arg()],
            Type::named("process.SupervisorWait"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "wait_or_none".to_string(),
            }),
            vec![timeout_arg()],
            process_result_ty(option_ty(Type::named("process.SupervisorEvent"))),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "stop".to_string(),
            }),
            Vec::new(),
            process_result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "is_empty".to_string(),
            }),
            Vec::new(),
            bool_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_listener".to_string()))),
                field: "accept".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(Type::named("net.TcpStream")),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_listener".to_string()))),
                field: "local_addr".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_listener".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "read_all".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "read_line".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(option_ty(string_ty.clone())),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "read_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), timeout_arg()],
            result_ty(option_ty(bytes_ty.clone())),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "read_exact".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), timeout_arg()],
            result_ty(bytes_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![arg(expr(ExprKind::String("ok".to_string()))), timeout_arg()],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "write_bytes".to_string(),
            }),
            vec![arg(bytes_expr()), timeout_arg()],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "shutdown_read".to_string(),
            }),
            Vec::new(),
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "shutdown_write".to_string(),
            }),
            Vec::new(),
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "shutdown_both".to_string(),
            }),
            Vec::new(),
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "flush".to_string(),
            }),
            Vec::new(),
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "local_addr".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "peer_addr".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "send_text".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("127.0.0.1:9".to_string()))),
                arg(expr(ExprKind::String("ok".to_string()))),
                timeout_arg(),
            ],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "send_bytes".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("127.0.0.1:9".to_string()))),
                arg(bytes_expr()),
                timeout_arg(),
            ],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "recv".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), timeout_arg()],
            result_ty(option_ty(bytes_ty.clone())),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "recv_from".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), timeout_arg()],
            result_ty(option_ty(Type::named("net.UdpDatagram"))),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "local_addr".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "peer_addr".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_datagram".to_string()))),
                field: "address".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_datagram".to_string()))),
                field: "bytes".to_string(),
            }),
            Vec::new(),
            bytes_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_datagram".to_string()))),
                field: "text".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_listener".to_string()))),
                field: "accept".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(Type::named("net.HttpExchange")),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_listener".to_string()))),
                field: "local_addr".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_listener".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "method".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "path".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "headers".to_string(),
            }),
            Vec::new(),
            headers_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "body_text".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "body_bytes".to_string(),
            }),
            Vec::new(),
            bytes_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "respond_text".to_string(),
            }),
            vec![
                arg(expr(ExprKind::Int(200))),
                arg(expr(ExprKind::String("ok".to_string()))),
                arg(headers_expr()),
            ],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "respond_bytes".to_string(),
            }),
            vec![
                arg(expr(ExprKind::Int(200))),
                arg(bytes_expr()),
                arg(headers_expr()),
            ],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_response".to_string()))),
                field: "status".to_string(),
            }),
            Vec::new(),
            int_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_response".to_string()))),
                field: "reason".to_string(),
            }),
            Vec::new(),
            string_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_response".to_string()))),
                field: "headers".to_string(),
            }),
            Vec::new(),
            headers_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_response".to_string()))),
                field: "text".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_response".to_string()))),
                field: "bytes".to_string(),
            }),
            Vec::new(),
            bytes_ty.clone(),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket_listener".to_string()))),
                field: "accept".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(Type::named("net.WebSocket")),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket_listener".to_string()))),
                field: "local_addr".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "send_text".to_string(),
            }),
            vec![arg(expr(ExprKind::String("ok".to_string()))), timeout_arg()],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "send_bytes".to_string(),
            }),
            vec![arg(bytes_expr()), timeout_arg()],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "recv_text".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(option_ty(string_ty.clone())),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "recv_bytes".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(option_ty(bytes_ty.clone())),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_listener".to_string()))),
                field: "accept".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(Type::named("net.UnixStream")),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_listener".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_stream".to_string()))),
                field: "read_line".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(option_ty(string_ty.clone())),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_stream".to_string()))),
                field: "read_exact".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), timeout_arg()],
            result_ty(bytes_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_stream".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![arg(expr(ExprKind::String("ok".to_string()))), timeout_arg()],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_stream".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_listener".to_string()))),
                field: "accept".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(Type::named("net.TlsStream")),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_listener".to_string()))),
                field: "local_addr".to_string(),
            }),
            Vec::new(),
            result_ty(string_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_listener".to_string()))),
                field: "close".to_string(),
            }),
            Vec::new(),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_stream".to_string()))),
                field: "read_line".to_string(),
            }),
            vec![timeout_arg()],
            result_ty(option_ty(string_ty.clone())),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_stream".to_string()))),
                field: "read_exact".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), timeout_arg()],
            result_ty(bytes_ty.clone()),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_stream".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![arg(expr(ExprKind::String("ok".to_string()))), timeout_arg()],
            result_ty(Type::Unit),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_stream".to_string()))),
                field: "close".to_string(),
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

    for (callee, args, expected) in [
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "put".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("ok".to_string()))),
                arg(expr(ExprKind::Int(1))),
            ],
            "`put(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "try_put".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`try_put` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "get".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`get(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "get_or_none".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`get(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "get_or".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("fallback".to_string()))),
                arg(expr(ExprKind::Int(1))),
            ],
            "`get_or(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("queue".to_string()))),
                field: "get_or".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`get_or` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("task".to_string()))),
                field: "result".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`result(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("task".to_string()))),
                field: "result_or_none".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`result(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("task".to_string()))),
                field: "result_or".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(0))), arg(expr(ExprKind::Int(1)))],
            "`result_or(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("task".to_string()))),
                field: "result_or".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`result_or` expects `int32`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("group".to_string()))),
                field: "start".to_string(),
            }),
            Vec::new(),
            "`start` expects a target function followed by its arguments",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("group".to_string()))),
                field: "start_soon".to_string(),
            }),
            vec![named_arg(
                "target",
                expr(ExprKind::Name("worker".to_string())),
            )],
            "`start_soon` does not take keyword arguments",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "wait".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`wait(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "wait_or_none".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`wait_or_none(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("child".to_string()))),
                field: "wait_ok".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`wait_ok(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "read_line".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`read_line(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "read_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::String("bad".to_string())))],
            "`read_bytes` expects `int32`, found `String`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "read_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), arg(expr(ExprKind::Int(1)))],
            "`read_bytes(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`write_all` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("ok".to_string()))),
                arg(expr(ExprKind::Int(1))),
            ],
            "`write_all(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "write_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::Name("bad_bytes".to_string())))],
            "`write_bytes` expects `Vec[uint8]`, found `Vec[bool]`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pipe".to_string()))),
                field: "write_bytes".to_string(),
            }),
            vec![arg(bytes_expr()), arg(expr(ExprKind::Int(1)))],
            "`write_bytes(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::Bool(true))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
            ],
            "`start` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::Bool(true))),
            ],
            "`start` expects `Vec[String]`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
                named_arg("cwd", expr(ExprKind::Bool(true))),
            ],
            "`start` expects `Option[String]`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
                named_arg("env", expr(ExprKind::Bool(true))),
            ],
            "`start` expects `Map[String, String]`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
                named_arg("stdin", expr(ExprKind::Bool(true))),
            ],
            "`start` expects `process.Stdio`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
                named_arg("stdout", expr(ExprKind::Bool(true))),
            ],
            "`start` expects `process.Stdio`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
                named_arg("stderr", expr(ExprKind::Bool(true))),
            ],
            "`start` expects `process.Stdio`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
                named_arg("backoff", expr(ExprKind::Bool(true))),
            ],
            "`start` expects `Duration`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
                named_arg("max_restarts", expr(ExprKind::Bool(true))),
            ],
            "`start` expects `int32`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
                named_arg("group", expr(ExprKind::Int(1))),
            ],
            "`start` expects `bool`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "wait".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`wait(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "wait_or_none".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`wait_or_none(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_listener".to_string()))),
                field: "accept".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`accept(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`write_all` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("file".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`write_all` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("file".to_string()))),
                field: "write_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::Name("bad_bytes".to_string())))],
            "`write_bytes` expects `Vec[uint8]`, found `Vec[bool]`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "split".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`split` expects `String`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "strip_prefix".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`strip_prefix` expects `String`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "get".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`get` expects `String`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "set".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1))), arg(expr(ExprKind::Int(2)))],
            "`set` expects key type `String`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "set".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("count".to_string()))),
                arg(expr(ExprKind::Bool(true))),
            ],
            "`set` expects value type `int32`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "remove".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`remove` expects `String`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "contains_key".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`contains_key` expects `String`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "extend".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`extend` expects `Map[String, int32]`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "contains".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`contains` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "insert".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`insert` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("items".to_string()))),
                field: "remove".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`remove` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "read_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::String("bad".to_string())))],
            "`read_bytes` expects `int32`, found `String`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "read_all".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`read_all(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "read_line".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`read_line(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "read_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), arg(expr(ExprKind::Int(1)))],
            "`read_bytes(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "read_exact".to_string(),
            }),
            vec![arg(expr(ExprKind::String("bad".to_string())))],
            "`read_exact` expects `int32`, found `String`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "read_exact".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), arg(expr(ExprKind::Int(1)))],
            "`read_exact(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("ok".to_string()))),
                arg(expr(ExprKind::Int(1))),
            ],
            "`write_all(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "write_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::Name("bad_bytes".to_string())))],
            "`write_bytes` expects `Vec[uint8]`, found `Vec[bool]`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tcp_stream".to_string()))),
                field: "write_bytes".to_string(),
            }),
            vec![arg(bytes_expr()), arg(expr(ExprKind::Int(1)))],
            "`write_bytes(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "send_text".to_string(),
            }),
            vec![
                arg(expr(ExprKind::Int(1))),
                arg(expr(ExprKind::String("ok".to_string()))),
            ],
            "`send_text` expects `String`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "send_text".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("127.0.0.1:9".to_string()))),
                arg(expr(ExprKind::Bool(true))),
            ],
            "`send_text` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "send_text".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("127.0.0.1:9".to_string()))),
                arg(expr(ExprKind::String("ok".to_string()))),
                arg(expr(ExprKind::Int(1))),
            ],
            "`send_text(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "send_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1))), arg(bytes_expr())],
            "`send_bytes` expects `String`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "send_bytes".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("127.0.0.1:9".to_string()))),
                arg(expr(ExprKind::Name("bad_bytes".to_string()))),
            ],
            "`send_bytes` expects `Vec[uint8]`, found `Vec[bool]`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "send_bytes".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("127.0.0.1:9".to_string()))),
                arg(bytes_expr()),
                arg(expr(ExprKind::Int(1))),
            ],
            "`send_bytes(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "recv".to_string(),
            }),
            vec![arg(expr(ExprKind::String("bad".to_string())))],
            "`recv` expects `int32`, found `String`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "recv".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), arg(expr(ExprKind::Int(1)))],
            "`recv(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "recv_from".to_string(),
            }),
            vec![arg(expr(ExprKind::String("bad".to_string())))],
            "`recv_from` expects `int32`, found `String`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("udp_socket".to_string()))),
                field: "recv_from".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), arg(expr(ExprKind::Int(1)))],
            "`recv_from(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_listener".to_string()))),
                field: "accept".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`accept(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "respond_text".to_string(),
            }),
            vec![
                arg(expr(ExprKind::Bool(true))),
                arg(expr(ExprKind::String("ok".to_string()))),
                arg(headers_expr()),
            ],
            "`respond_text` expects `int32`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "respond_text".to_string(),
            }),
            vec![
                arg(expr(ExprKind::Int(200))),
                arg(expr(ExprKind::Bool(true))),
                arg(headers_expr()),
            ],
            "`respond_text` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "respond_text".to_string(),
            }),
            vec![
                arg(expr(ExprKind::Int(200))),
                arg(expr(ExprKind::String("ok".to_string()))),
                arg(expr(ExprKind::Bool(true))),
            ],
            "`respond_text` expects `Map[String, String]`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "respond_bytes".to_string(),
            }),
            vec![
                arg(expr(ExprKind::Bool(true))),
                arg(bytes_expr()),
                arg(headers_expr()),
            ],
            "`respond_bytes` expects `int32`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "respond_bytes".to_string(),
            }),
            vec![
                arg(expr(ExprKind::Int(200))),
                arg(expr(ExprKind::Name("bad_bytes".to_string()))),
                arg(headers_expr()),
            ],
            "`respond_bytes` expects `Vec[uint8]`, found `Vec[bool]`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("http_exchange".to_string()))),
                field: "respond_bytes".to_string(),
            }),
            vec![
                arg(expr(ExprKind::Int(200))),
                arg(bytes_expr()),
                arg(expr(ExprKind::Bool(true))),
            ],
            "`respond_bytes` expects `Map[String, String]`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket_listener".to_string()))),
                field: "accept".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`accept(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "send_text".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`send_text` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "send_text".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("ok".to_string()))),
                arg(expr(ExprKind::Int(1))),
            ],
            "`send_text(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "send_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::Name("bad_bytes".to_string())))],
            "`send_bytes` expects `Vec[uint8]`, found `Vec[bool]`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "send_bytes".to_string(),
            }),
            vec![arg(bytes_expr()), arg(expr(ExprKind::Int(1)))],
            "`send_bytes(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "recv_text".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`recv_text(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("websocket".to_string()))),
                field: "recv_bytes".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`recv_bytes(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_listener".to_string()))),
                field: "accept".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`accept(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_stream".to_string()))),
                field: "read_line".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`read_line(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_stream".to_string()))),
                field: "read_exact".to_string(),
            }),
            vec![arg(expr(ExprKind::String("bad".to_string())))],
            "`read_exact` expects `int32`, found `String`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_stream".to_string()))),
                field: "read_exact".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), arg(expr(ExprKind::Int(1)))],
            "`read_exact(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_stream".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`write_all` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unix_stream".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("ok".to_string()))),
                arg(expr(ExprKind::Int(1))),
            ],
            "`write_all(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_listener".to_string()))),
                field: "accept".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`accept(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_stream".to_string()))),
                field: "read_line".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(1)))],
            "`read_line(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_stream".to_string()))),
                field: "read_exact".to_string(),
            }),
            vec![arg(expr(ExprKind::String("bad".to_string())))],
            "`read_exact` expects `int32`, found `String`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_stream".to_string()))),
                field: "read_exact".to_string(),
            }),
            vec![arg(expr(ExprKind::Int(8))), arg(expr(ExprKind::Int(1)))],
            "`read_exact(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_stream".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![arg(expr(ExprKind::Bool(true)))],
            "`write_all` expects `String`, found `bool`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("tls_stream".to_string()))),
                field: "write_all".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("ok".to_string()))),
                arg(expr(ExprKind::Int(1))),
            ],
            "`write_all(timeout=...)` expects `Duration`, found `int64`",
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("supervisor".to_string()))),
                field: "start".to_string(),
            }),
            vec![
                arg(expr(ExprKind::String("svc".to_string()))),
                arg(expr(ExprKind::List(vec![expr(ExprKind::String(
                    "/bin/echo".to_string(),
                ))]))),
                named_arg("restart", expr(ExprKind::Bool(true))),
            ],
            "`start` expects `process.RestartPolicy`, found `bool`",
        ),
    ] {
        let error = match checker.type_of_call(&callee, &args, span, &mut locals, None) {
            Ok(actual) => {
                panic!("member call should report `{expected}`, but type checked as `{actual}`")
            }
            Err(error) => error,
        };
        assert!(
            error.message.contains(expected),
            "expected diagnostic containing `{expected}`, got `{}`",
            error.message
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
                field: "get".to_string(),
            }),
            &[arg(expr(ExprKind::Bool(true)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("vec.get() should enforce integer indices")
        .message
        .contains("vector indices must have type `int32`"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "set".to_string(),
            }),
            &[
                named_arg("index", expr(ExprKind::Bool(true))),
                named_arg("value", expr(ExprKind::Int(1))),
            ],
            span,
            &mut locals,
            None,
        )
        .expect_err("vec.set() should enforce integer indices")
        .message
        .contains("vector indices must have type `int32`"));

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
        .contains("vector indices must have type `int32`"));

    assert!(checker
        .type_of_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "swap".to_string(),
            }),
            &[arg(expr(ExprKind::Bool(true))), arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("vec.swap() should enforce the first integer index")
        .message
        .contains("vector indices must have type `int32`"));

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
        .contains("vector indices must have type `int32`"));

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
            &[arg(expr(ExprKind::Bool(true))), arg(expr(ExprKind::Int(1)))],
            span,
            &mut locals,
            None,
        )
        .expect_err("vec.insert() should enforce integer indices")
        .message
        .contains("vector indices must have type `int32`"));

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

    let pkg_widget = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
        field: "Widget".to_string(),
    });
    let mut call_locals = HashMap::new();
    checker.seed_imported_modules(&mut call_locals);
    assert_eq!(
        checker
            .type_of_call(
                &pkg_widget,
                &[named_arg("value", expr(ExprKind::Int(1)))],
                Span::new(1, 1),
                &mut call_locals,
                None,
            )
            .expect("module class constructors should type check"),
        Type::named("Widget")
    );
    for (args, expected) in [
        (
            vec![
                named_arg("value", expr(ExprKind::Int(1))),
                arg(expr(ExprKind::Int(2))),
            ],
            "positional class constructor arguments must come before named arguments",
        ),
        (
            vec![arg(expr(ExprKind::Int(1))), arg(expr(ExprKind::Int(2)))],
            "class constructor `Widget` received too many positional arguments",
        ),
        (
            vec![named_arg("missing", expr(ExprKind::Int(1)))],
            "class `Widget` has no field named `missing`",
        ),
        (
            vec![
                named_arg("value", expr(ExprKind::Int(1))),
                named_arg("value", expr(ExprKind::Int(2))),
            ],
            "field `value` was provided more than once",
        ),
    ] {
        assert!(
            checker
                .type_of_call(&pkg_widget, &args, Span::new(1, 1), &mut call_locals, None,)
                .expect_err("module class constructor diagnostics should be reported")
                .message
                .contains(expected),
            "expected module constructor diagnostic containing `{expected}`"
        );
    }
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
fn checker_move_consumption_helpers_cover_managed_specialized_member_and_match_paths() {
    let span = Span::new(1, 1);
    let classes = BTreeMap::from([(
        "Holder".to_string(),
        class_info(
            "Holder",
            false,
            vec![("text", Type::named("String"), false)],
        ),
    )]);
    let type_names = BTreeMap::from([("Holder".to_string(), span)]);
    let type_arities = BTreeMap::from([("Holder".to_string(), 0usize)]);
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

    let mut managed_locals = HashMap::from([(
        "resource".to_string(),
        LocalBinding {
            managed_resource: true,
            ..local_binding(
                Type::named("String"),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            )
        },
    )]);
    let managed_error = checker
        .consume_binding("resource", span, &mut managed_locals)
        .expect_err("managed resources should not move out by value");
    assert!(managed_error
        .message
        .contains("cannot move managed `with` resource `resource`"));

    let mut specialized_locals = HashMap::from([(
        "owned".to_string(),
        local_binding(
            Type::named("String"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    checker
        .consume_value_expr(
            &expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("owned".to_string()))),
                type_args: vec![type_ref("String")],
            }),
            &mut specialized_locals,
        )
        .expect("specialized value expressions should consume their base value");
    assert!(specialized_locals["owned"].moved);

    let mut member_locals = HashMap::from([(
        "holder".to_string(),
        local_binding(
            Type::named("Holder"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    checker
        .consume_value_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("holder".to_string()))),
                field: "text".to_string(),
            }),
            &mut member_locals,
        )
        .expect("moving a non-copy field from an owned binding should be tracked");
    assert!(member_locals["holder"]
        .moved_fields
        .contains(&projection_path("text")));

    let mut match_locals = HashMap::from([
        (
            "flag".to_string(),
            local_binding(
                Type::named("bool"),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "owned".to_string(),
            local_binding(
                Type::named("String"),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
    ]);
    checker
        .consume_value_expr(
            &expr(ExprKind::Match {
                scrutinee: Box::new(expr(ExprKind::Name("flag".to_string()))),
                borrow_mode: None,
                arms: vec![MatchExprArm {
                    pattern: Pattern::Wildcard(span),
                    value: expr(ExprKind::Name("owned".to_string())),
                    span,
                }],
            }),
            &mut match_locals,
        )
        .expect("match expression arms should merge consumed value state");
    assert!(match_locals["owned"].moved);

    let mut group_locals = HashMap::from([(
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
    checker
        .consume_match_scrutinee_expr(
            &expr(ExprKind::Group(Box::new(expr(ExprKind::Name(
                "flag".to_string(),
            ))))),
            &mut group_locals,
        )
        .expect("grouped match scrutinees should be consumed through their inner expression");

    let mut borrowed_holder_locals = HashMap::from([(
        "holder".to_string(),
        LocalBinding {
            passing: ReceiverKind::Borrow,
            borrow_origin: Some("holder".to_string()),
            ..local_binding(
                Type::named("Holder"),
                false,
                false,
                ReceiverKind::Borrow,
                false,
                &[],
            )
        },
    )]);
    let borrowed_field_error = checker
        .consume_match_scrutinee_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("holder".to_string()))),
                field: "text".to_string(),
            }),
            &mut borrowed_holder_locals,
        )
        .expect_err("match scrutinees should reject moving non-copy fields out of borrows");
    assert!(borrowed_field_error
        .message
        .contains("cannot move non-copy field `text` out of borrowed value `holder`"));

    let grouped_borrowed_field_error = checker
        .consume_match_scrutinee_expr(
            &expr(ExprKind::Group(Box::new(expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("holder".to_string()))),
                field: "text".to_string(),
            })))),
            &mut borrowed_holder_locals,
        )
        .expect_err("grouped match scrutinees should still reject borrowed field moves");
    assert!(grouped_borrowed_field_error
        .message
        .contains("cannot move non-copy field `text` out of borrowed value `holder`"));

    let mut merged_locals = HashMap::from([(
        "holder".to_string(),
        local_binding(
            Type::named("Holder"),
            true,
            true,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    let mut branch_with_stale_match_borrow = merged_locals.clone();
    let branch_binding = branch_with_stale_match_borrow
        .get_mut("holder")
        .expect("branch binding should exist");
    branch_binding.moved = true;
    branch_binding.moved_fields.insert(projection_path("text"));
    branch_binding.stale_match_borrow_mut_place = Some(place_path("holder.text"));
    let branch_without_binding = HashMap::new();
    checker.merge_control_flow_moves(
        &mut merged_locals,
        &[&branch_with_stale_match_borrow, &branch_without_binding],
    );
    assert!(merged_locals["holder"].moved);
    assert!(merged_locals["holder"]
        .moved_fields
        .contains(&projection_path("text")));
    assert_eq!(
        merged_locals["holder"].stale_match_borrow_mut_place,
        Some(place_path("holder.text"))
    );

    assert_eq!(
        checker.const_bool_value(&expr(ExprKind::Unary {
            op: UnaryOp::Not,
            expr: Box::new(expr(ExprKind::Group(Box::new(expr(ExprKind::Bool(false)))))),
        })),
        Some(true)
    );

    checker
        .reject_loop_carried_moves(
            &HashMap::from([(
                "outer".to_string(),
                local_binding(
                    Type::named("String"),
                    true,
                    true,
                    ReceiverKind::Value,
                    false,
                    &[],
                ),
            )]),
            &HashMap::new(),
            "while",
            span,
        )
        .expect("bindings absent from the loop body state should be ignored");
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
    let mut root = namespace("pkg");
    root.imported_modules
        .insert("external".to_string(), imported.clone());
    assert_eq!(
        find_namespace_in_modules(&BTreeMap::from([("pkg".to_string(), root)]), "pkg.external")
            .map(|found| found.path.clone()),
        Some("pkg.external".to_string())
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
    let reserved_self = validate_type_params(&["Self".to_string()], Span::new(1, 1), "class Box")
        .expect_err("Self cannot be used as a type parameter");
    assert!(reserved_self.message.contains("`Self` is reserved"));

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
    assert!(place_path("counter.value").overlaps(&place_path("counter")));
    assert!(place_path("counter").overlaps(&place_path("counter.value")));
    assert!(!place_path("counter.left").overlaps(&place_path("counter.right")));
    assert!(!place_path("left.value").overlaps(&place_path("right.value")));
}

#[test]
fn operator_trait_helpers_map_supported_operators() {
    assert_eq!(unary_operator_trait(UnaryOp::Neg), Some(("Neg", "neg")));
    assert_eq!(unary_operator_trait(UnaryOp::Not), Some(("Not", "not")));
    assert_eq!(binary_operator_trait(BinaryOp::Add), Some(("Add", "add")));
    assert_eq!(binary_operator_trait(BinaryOp::Sub), Some(("Sub", "sub")));
    assert_eq!(binary_operator_trait(BinaryOp::Mul), Some(("Mul", "mul")));
    assert_eq!(binary_operator_trait(BinaryOp::Div), Some(("Div", "div")));
    assert_eq!(binary_operator_trait(BinaryOp::FloorDiv), None);
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
fn return_borrow_source_resolution_covers_explicit_and_inferred_edges() {
    fn borrowed_param(name: &str, passing: ReceiverKind, label: Option<&str>) -> Param {
        Param {
            name: name.to_string(),
            passing,
            borrow_label: label.map(str::to_string),
            ty: type_ref("String"),
            default: None,
            span: Span::new(1, 1),
        }
    }

    let span = Span::new(1, 1);
    assert_eq!(
        resolve_return_borrow_source(None, &[], ReceiverKind::Value, None, span)
            .expect("owned returns do not need a borrow source"),
        None
    );
    assert_eq!(
        resolve_return_borrow_source(
            Some(ReceiverKind::BorrowMut),
            &[],
            ReceiverKind::BorrowMut,
            None,
            span,
        )
        .expect("a borrowed receiver can infer self as source"),
        Some("self".to_string())
    );
    assert_eq!(
        resolve_return_borrow_source(
            None,
            &[borrowed_param("text", ReceiverKind::Borrow, None)],
            ReceiverKind::Borrow,
            None,
            span,
        )
        .expect("a single borrowed parameter can be inferred"),
        Some("text".to_string())
    );

    let missing = resolve_return_borrow_source(
        None,
        &[borrowed_param("text", ReceiverKind::Borrow, None)],
        ReceiverKind::Borrow,
        Some("other"),
        span,
    )
    .expect_err("explicit sources must name a borrowed parameter");
    assert!(missing.message.contains("must name a borrowed parameter"));

    let immutable_for_mut = resolve_return_borrow_source(
        None,
        &[borrowed_param("text", ReceiverKind::Borrow, Some("src"))],
        ReceiverKind::BorrowMut,
        Some("src"),
        span,
    )
    .expect_err("borrow mut returns only consider mutable sources");
    assert!(immutable_for_mut
        .message
        .contains("must name a borrowed parameter"));

    let none_available = resolve_return_borrow_source(
        None,
        &[borrowed_param("text", ReceiverKind::Value, None)],
        ReceiverKind::Borrow,
        None,
        span,
    )
    .expect_err("borrowed returns require at least one borrowed source");
    assert!(none_available
        .message
        .contains("require a borrowed parameter or receiver"));

    let ambiguous_mut = resolve_return_borrow_source(
        None,
        &[
            borrowed_param("left", ReceiverKind::BorrowMut, None),
            borrowed_param("right", ReceiverKind::BorrowMut, None),
        ],
        ReceiverKind::BorrowMut,
        None,
        span,
    )
    .expect_err("multiple mutable candidates need an explicit source");
    assert!(ambiguous_mut.message.contains("-> borrow mut[left]"));
}

#[test]
fn call_expr_borrow_info_covers_method_return_sources() {
    let span = Span::new(1, 1);
    let mut holder = class_info(
        "Holder",
        false,
        vec![("value", Type::named("String"), false)],
    );
    holder.methods.insert(
        "value_ref".to_string(),
        MethodInfo {
            decl: {
                let mut decl = function_decl("value_ref");
                decl.receiver = Some(ReceiverKind::Borrow);
                decl.return_passing = ReceiverKind::Borrow;
                decl.return_borrow_source = Some("self".to_string());
                decl.return_type = type_ref("String");
                decl
            },
            signature: FunctionSignature {
                params: Vec::new(),
                return_type: Type::named("String"),
                return_passing: ReceiverKind::Borrow,
                return_borrow_source: Some("self".to_string()),
            },
            type_param_bounds: BTreeMap::new(),
        },
    );
    holder.methods.insert(
        "pick".to_string(),
        MethodInfo {
            decl: {
                let mut decl = function_decl("pick");
                decl.receiver = Some(ReceiverKind::Borrow);
                decl.params = vec![Param {
                    name: "source".to_string(),
                    ty: type_ref("String"),
                    passing: ReceiverKind::Borrow,
                    borrow_label: None,
                    default: None,
                    span,
                }];
                decl.return_passing = ReceiverKind::Borrow;
                decl.return_borrow_source = Some("source".to_string());
                decl.return_type = type_ref("String");
                decl
            },
            signature: FunctionSignature {
                params: vec![Type::named("String")],
                return_type: Type::named("String"),
                return_passing: ReceiverKind::Borrow,
                return_borrow_source: Some("source".to_string()),
            },
            type_param_bounds: BTreeMap::new(),
        },
    );

    let type_names = BTreeMap::from([("Holder".to_string(), span)]);
    let type_arities = BTreeMap::from([("Holder".to_string(), 0usize)]);
    let classes = BTreeMap::from([("Holder".to_string(), holder)]);
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
            "holder".to_string(),
            local_binding(
                Type::named("Holder"),
                true,
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
                true,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
    ]);

    let self_return = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Name("holder".to_string()))),
            field: "value_ref".to_string(),
        })),
        args: Vec::new(),
    });
    let self_info = checker
        .expr_borrow_info(&self_return, &mut locals)
        .expect("method self borrowed return should resolve")
        .expect("borrow source should be present");
    assert_eq!(self_info.origin, "holder");
    assert_eq!(self_info.passing, ReceiverKind::Borrow);

    let arg_return = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Name("holder".to_string()))),
            field: "pick".to_string(),
        })),
        args: vec![named_arg(
            "source",
            expr(ExprKind::Name("text".to_string())),
        )],
    });
    let arg_info = checker
        .expr_borrow_info(&arg_return, &mut locals)
        .expect("method argument borrowed return should resolve")
        .expect("borrow source should be present");
    assert_eq!(arg_info.origin, "text");
    assert_eq!(arg_info.passing, ReceiverKind::Borrow);
}

#[test]
fn borrowed_copy_return_assignments_bind_as_plain_values() {
    crate::check_source(
        "def id_ref(value: borrow[src] int32) -> borrow[src] int32:\n    return value\n\n\
def main() -> int32:\n    value: int32 = 7\n    mirrored = id_ref(value)\n    return mirrored\n",
    )
    .expect("copy-typed borrowed returns should be bindable as plain values");
}

#[test]
fn default_argument_reference_detection_walks_nested_expression_shapes() {
    let params = vec!["left".to_string(), "right".to_string()];
    let name_left = expr(ExprKind::Name("left".to_string()));
    let name_right = expr(ExprKind::Name("right".to_string()));
    let unrelated = expr(ExprKind::Name("other".to_string()));

    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(name_left.clone()),
            }),
            &params,
        ),
        Some("left".to_string())
    );
    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::Cast {
                expr: Box::new(name_right.clone()),
                ty: type_ref("int32"),
            }),
            &params,
        ),
        Some("right".to_string())
    );
    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::Specialize {
                expr: Box::new(name_left.clone()),
                type_args: vec![type_ref("String")],
            }),
            &params,
        ),
        Some("left".to_string())
    );
    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::Member {
                object: Box::new(name_left.clone()),
                field: "len".to_string(),
            }),
            &params,
        ),
        Some("left".to_string())
    );
    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::Index {
                object: Box::new(unrelated.clone()),
                index: Box::new(name_right.clone()),
            }),
            &params,
        ),
        Some("right".to_string())
    );
    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::Call {
                callee: Box::new(unrelated.clone()),
                args: vec![named_arg("value", name_left.clone())],
            }),
            &params,
        ),
        Some("left".to_string())
    );
    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::Map(vec![MapEntryExpr {
                key: unrelated.clone(),
                value: name_right.clone(),
            }])),
            &params,
        ),
        Some("right".to_string())
    );
    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::FString(vec![
                FormatPart::Literal("prefix".to_string()),
                FormatPart::Expr(name_left.clone()),
            ])),
            &params,
        ),
        Some("left".to_string())
    );
    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::Match {
                scrutinee: Box::new(unrelated.clone()),
                borrow_mode: None,
                arms: vec![MatchExprArm {
                    pattern: Pattern::Wildcard(Span::new(1, 1)),
                    value: name_right.clone(),
                    span: Span::new(1, 1),
                }],
            }),
            &params,
        ),
        Some("right".to_string())
    );
    assert_eq!(
        default_argument_references_param(
            &expr(ExprKind::Binary {
                op: BinaryOp::Add,
                left: Box::new(unrelated),
                right: Box::new(name_left),
            }),
            &params,
        ),
        Some("left".to_string())
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
fn check_rejects_duplicate_ordinary_parameter_names() {
    let cases = [
        (
            "def choose(value: int32, value: int32) -> int32:\n    return value\n",
            "duplicate parameter `value` on function `choose`",
        ),
        (
            "class Counter:\n    value: int32\n\n    def add(borrow self, amount: int32, amount: int32) -> int32:\n        return self.value + amount\n",
            "duplicate parameter `amount` on method `add`",
        ),
        (
            "trait Combine:\n    def combine(borrow self, other: int32, other: int32) -> int32\n",
            "duplicate parameter `other` on trait method `combine`",
        ),
        (
            "trait Combine:\n    def combine(borrow self, left: int32, right: int32) -> int32\n\nclass Counter:\n    value: int32\n\nimpl Combine for Counter:\n    def combine(borrow self, value: int32, value: int32) -> int32:\n        return self.value + value\n",
            "duplicate parameter `value` on impl method `combine`",
        ),
        (
            "class Counter:\n    value: int32\n\n    def add(borrow self, self: int32) -> int32:\n        return self\n",
            "parameter `self` conflicts with the receiver on method `add`",
        ),
    ];

    for (source, expected) in cases {
        let error = crate::check_source(source).expect_err("duplicate parameters should fail");
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
    assert_eq!(
        lower_type_with_self(
            &type_ref("Self"),
            &type_names,
            &type_arities,
            &type_params,
            Some(&Type::named("Counter"))
        )
        .expect("Self should lower with an explicit self type"),
        Type::named("Counter")
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
    let self_type_args = lower_type_with_self(
        &nested_type_ref("Self", vec![type_ref("int32")]),
        &type_names,
        &type_arities,
        &type_params,
        Some(&Type::named("Counter")),
    )
    .expect_err("Self should reject explicit type arguments");
    assert!(self_type_args
        .message
        .contains("`Self` does not take generic arguments"));
    let self_without_context = lower_type_with_self(
        &type_ref("Self"),
        &type_names,
        &type_arities,
        &type_params,
        None,
    )
    .expect_err("Self requires an enclosing self type");
    assert!(self_without_context
        .message
        .contains("`Self` is only available"));
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
fn lower_supertraits_reports_unknown_arity_and_lowers_self_args() {
    let traits = BTreeMap::from([
        ("Base".to_string(), trait_info("Base", vec![])),
        ("Mapper".to_string(), trait_info("Mapper", vec!["T"])),
    ]);
    let type_names = BTreeMap::from([
        ("String".to_string(), Span::new(1, 1)),
        ("Widget".to_string(), Span::new(1, 1)),
    ]);
    let type_arities = BTreeMap::from([("String".to_string(), 0usize), ("Widget".to_string(), 0)]);
    let scope = type_param_scope(&["T".to_string()]);

    let unknown = lower_supertraits(
        &[type_ref("Missing")],
        &traits,
        &type_names,
        &type_arities,
        &scope,
        Some(&Type::named("Widget")),
    )
    .expect_err("unknown supertraits should fail");
    assert!(unknown.message.contains("unknown trait `Missing`"));

    let arity = lower_supertraits(
        &[nested_type_ref("Mapper", vec![])],
        &traits,
        &type_names,
        &type_arities,
        &scope,
        Some(&Type::named("Widget")),
    )
    .expect_err("supertrait arity mismatches should fail");
    assert!(arity
        .message
        .contains("trait `Mapper` expects 1 type arguments, found 0"));

    let lowered = lower_supertraits(
        &[
            type_ref("Base"),
            nested_type_ref("Mapper", vec![type_ref("Self")]),
        ],
        &traits,
        &type_names,
        &type_arities,
        &scope,
        Some(&Type::named("Widget")),
    )
    .expect("valid supertraits should lower");
    assert_eq!(
        lowered,
        vec![
            TraitBound {
                trait_name: "Base".to_string(),
                trait_args: Vec::new(),
            },
            TraitBound {
                trait_name: "Mapper".to_string(),
                trait_args: vec![Type::named("Widget")],
            },
        ]
    );

    let bad_arg = lower_supertraits(
        &[nested_type_ref("Mapper", vec![type_ref("MissingType")])],
        &traits,
        &type_names,
        &type_arities,
        &scope,
        Some(&Type::named("Widget")),
    )
    .expect_err("supertrait arguments should be type-checked");
    assert!(bad_arg.message.contains("unknown type `MissingType`"));
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
        .contains("`wait_all(timeout=...)` expects `Duration`, found `int64`"));

    let recv_timeout = crate::check_source(
        "def main() -> int32:\n    jobs = Queue[int32]()\n    return jobs.get(timeout=1)\n",
    )
    .expect_err("queue.get timeout should require Duration");
    assert!(recv_timeout
        .message
        .contains("`get(timeout=...)` expects `Duration`, found `int64`"));

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
    for source in [
        "class Job:\n    label: String\n\ndef main() -> None:\n    jobs = Queue[Job]()\n    for job in borrow jobs:\n        pass\n",
        "def main() -> None:\n    jobs = Queue[int32]()\n    for job in borrow jobs:\n        pass\n",
        "class Job:\n    label: String\n\ndef main() -> None:\n    jobs: Set[Job] = Set[Job]()\n    for job in borrow jobs:\n        pass\n",
    ] {
        crate::check_source(source).expect("borrowed loop source should type check");
    }

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
fn checker_loop_const_bool_conditions_cover_grouped_and_negated_forms() {
    crate::check_source(
        "class Name:\n    value: String\n\ndef main():\n    name = Name(value=\"aurora\")\n    while (false):\n        moved = name.value\n    later = name.value\n",
    )
    .expect("grouped false loops should not merge move state from unreachable bodies");

    let repeated_move = crate::check_source(
        "class Name:\n    value: String\n\ndef main():\n    name = Name(value=\"aurora\")\n    while not false:\n        moved = name.value\n",
    )
    .expect_err("negated false loops may execute and should reject repeated moves");
    assert!(repeated_move
        .message
        .contains("`while` loop body partially moves `name` and may execute more than once"));
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

    let borrowed_return_checker = checker.with_return_type(
        Type::named("String"),
        ReceiverKind::Borrow,
        Some("source".to_string()),
    );
    let mut owned_return_locals = HashMap::from([(
        "owned".to_string(),
        local_binding(
            Type::named("String"),
            true,
            false,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    let unborrowed_return = borrowed_return_checker
        .check_block(
            &[Stmt::Return(ReturnStmt {
                value: Some(expr(ExprKind::Name("owned".to_string()))),
                span,
            })],
            &mut owned_return_locals,
            &Type::named("String"),
            0,
            true,
        )
        .expect_err("borrowed returns should require a borrowed source expression");
    assert!(unborrowed_return
        .message
        .contains("borrowed return expression must come from a borrowed parameter or receiver"));

    let missing_source_checker =
        checker.with_return_type(Type::named("String"), ReceiverKind::Borrow, None);
    let mut borrowed_return_locals = HashMap::from([(
        "source".to_string(),
        LocalBinding {
            passing: ReceiverKind::Borrow,
            borrow_origin: Some("source".to_string()),
            ..local_binding(
                Type::named("String"),
                true,
                false,
                ReceiverKind::Borrow,
                false,
                &[],
            )
        },
    )]);
    let missing_source = missing_source_checker
        .check_block(
            &[Stmt::Return(ReturnStmt {
                value: Some(expr(ExprKind::Name("source".to_string()))),
                span,
            })],
            &mut borrowed_return_locals,
            &Type::named("String"),
            0,
            true,
        )
        .expect_err("borrowed return source should be resolved before block checking");
    assert!(missing_source
        .message
        .contains("internal error: borrowed return source was not resolved"));

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
            "def main() -> int32:\n    text = \"  Aurora  \"\n    pieces: Vec[String] = text.split(\"u\")\n    replaced: String = text.replace(\"Aurora\", \"language\")\n    lowered: String = text.to_lower()\n    raised: String = text.to_upper()\n    prefix: Option[String] = text.strip_prefix(\"  \")\n    suffix: Option[String] = text.strip_suffix(\"  \")\n    text_len: int32 = text.len()\n    text_has: bool = text.contains(\"Aur\")\n    text_start: bool = text.starts_with(\"  A\")\n    text_end: bool = text.ends_with(\"  \")\n    parsed_i32: Result[int32, String] = parse_int32(text=\"7\")\n    parsed_i64: Result[int64, String] = parse_int64(text=\"9\")\n    parsed_f64: Result[float64, String] = parse_float64(text=\"3.5\")\n    negative: int32 = -7\n    one: int32 = 1\n    two: int32 = 2\n    abs_i32: int32 = abs(value=negative)\n    min_i32: int32 = min(left=one, right=two)\n    max_i32: int32 = max(left=one, right=two)\n    root: float64 = sqrt(value=9.0)\n    mut values: Vec[int32] = [1, 2, 3]\n    popped: Option[int32] = values.pop()\n    gotten: Option[int32] = values.get(index=0)\n    inserted: bool = values.insert(index=0, value=9)\n    mut counts: Map[String, int32] = {\"a\": 1}\n    keys: Vec[String] = counts.keys()\n    vals: Vec[int32] = counts.values()\n    entries: Vec[MapEntry[String, int32]] = counts.items()\n    mut names = Set{\"ada\"}\n    has_name: bool = names.contains(\"ada\")\n    inserted_name: bool = names.insert(\"bob\")\n    removed_name: bool = names.remove(\"ada\")\n    return text_len + abs_i32 + min_i32 + max_i32 + (root as int32)\n",
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
            "TaskGroup[]",
            expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("TaskGroup".to_string()))),
                type_args: Vec::new(),
            }),
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
            Type::named("int64"),
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
fn checker_builtin_constructor_and_variant_error_edges_cover_direct_paths() {
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
    let int_ty = Type::named("int32");
    let string_ty = Type::named("String");
    let bool_ty = Type::named("bool");
    let vec_int_ty = Type::Named("Vec".to_string(), vec![int_ty.clone()]);
    let vec_type_param_ty = Type::Named("Vec".to_string(), vec![Type::TypeParam("T".to_string())]);
    let vec_task_int_ty = Type::Named(
        "Vec".to_string(),
        vec![Type::Named("Task".to_string(), vec![int_ty.clone()])],
    );
    let queue_int_ty = Type::Named("Queue".to_string(), vec![int_ty.clone()]);
    let mut locals = HashMap::from([
        (
            "numbers".to_string(),
            local_binding(vec_int_ty, false, false, ReceiverKind::Value, false, &[]),
        ),
        (
            "generic_values".to_string(),
            local_binding(
                vec_type_param_ty,
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
                vec_task_int_ty,
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "queue".to_string(),
            local_binding(queue_int_ty, false, false, ReceiverKind::Value, false, &[]),
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
            "flag".to_string(),
            local_binding(bool_ty, false, false, ReceiverKind::Value, false, &[]),
        ),
    ]);

    let name = |name: &str| expr(ExprKind::Name(name.to_string()));
    let specialize = |name: &str, type_args: Vec<TypeRef>| {
        expr(ExprKind::Specialize {
            expr: Box::new(expr(ExprKind::Name(name.to_string()))),
            type_args,
        })
    };
    let member = |object: Expr, field: &str| {
        expr(ExprKind::Member {
            object: Box::new(object),
            field: field.to_string(),
        })
    };
    let mut expect_error =
        |callee: Expr, args: Vec<Argument>, expected: Option<Type>, text: &str| {
            let error = checker
                .type_of_call(&callee, &args, span, &mut locals, expected.as_ref())
                .expect_err("checker direct path should report a diagnostic");
            assert!(
                error.message.contains(text),
                "expected diagnostic containing `{text}`, got `{}`",
                error.message
            );
        };

    for (callee, args, expected) in [
        (
            name("TaskGroup"),
            vec![arg(expr(ExprKind::Int(1)))],
            "`TaskGroup` does not take constructor arguments",
        ),
        (
            specialize("TaskGroup", vec![type_ref("int32")]),
            Vec::new(),
            "`TaskGroup` does not take type arguments",
        ),
        (
            specialize("TaskGroup", Vec::new()),
            vec![arg(expr(ExprKind::Int(1)))],
            "`TaskGroup` does not take constructor arguments",
        ),
        (
            specialize("Queue", vec![type_ref("int32"), type_ref("String")]),
            Vec::new(),
            "class `Queue` expects exactly one type argument, found 2",
        ),
        (
            specialize("Queue", vec![type_ref("int32")]),
            vec![named_arg("capacity", expr(ExprKind::Bool(true)))],
            "field `capacity` expects `int32`, found `bool`",
        ),
        (
            specialize("Vec", vec![type_ref("int32"), type_ref("String")]),
            Vec::new(),
            "class `Vec` expects exactly one type argument, found 2",
        ),
        (
            specialize("Vec", vec![type_ref("int32")]),
            vec![arg(expr(ExprKind::Int(1)))],
            "class `Vec` does not take constructor arguments",
        ),
        (
            specialize("Set", vec![type_ref("String"), type_ref("int32")]),
            Vec::new(),
            "class `Set` expects exactly one type argument, found 2",
        ),
        (
            specialize("Set", vec![type_ref("String")]),
            vec![arg(expr(ExprKind::String("ada".to_string())))],
            "class `Set` does not take constructor arguments",
        ),
        (
            specialize("Map", vec![type_ref("String")]),
            Vec::new(),
            "class `Map` expects exactly two type arguments, found 1",
        ),
        (
            specialize("Map", vec![type_ref("String"), type_ref("int32")]),
            vec![arg(expr(ExprKind::Int(1)))],
            "class `Map` does not take constructor arguments",
        ),
    ] {
        expect_error(callee, args, None, expected);
    }

    for (callee, args, expected) in [
        (
            name("wait_any"),
            vec![arg(name("queue"))],
            "`wait_any` expects `Vec[Task[T]]`, found `Queue[int32]`",
        ),
        (
            name("wait_all"),
            vec![arg(name("numbers"))],
            "`wait_all` expects `Vec[Task[T]]`, found `Vec[int32]`",
        ),
        (
            name("wait_any"),
            vec![arg(name("generic_values"))],
            "`wait_any` expects `Vec[Task[T]]`, found `Vec[T]`",
        ),
        (
            name("wait_all"),
            vec![
                arg(name("tasks")),
                named_arg("timeout", expr(ExprKind::Int(1))),
            ],
            "`wait_all(timeout=...)` expects `Duration`, found `int64`",
        ),
    ] {
        expect_error(callee, args, None, expected);
    }

    expect_error(
        name("Some"),
        vec![arg(expr(ExprKind::Int(1)))],
        None,
        "bare enum variants require an expected enum type",
    );
    expect_error(
        name("Some"),
        vec![arg(expr(ExprKind::Int(1)))],
        Some(Type::Unit),
        "bare enum variants require an expected enum type",
    );
    expect_error(
        name("Closed"),
        Vec::new(),
        Some(Type::Named("Option".to_string(), vec![int_ty.clone()])),
        "bare enum variants require an expected enum type",
    );
    expect_error(
        member(name("Option"), "None"),
        Vec::new(),
        None,
        "cannot infer type parameter `T` for enum variant `Option.None`",
    );
    drop(expect_error);

    assert_eq!(
        checker
            .type_of_call(
                &member(name("Option"), "Some"),
                &[arg(name("text"))],
                span,
                &mut locals,
                None,
            )
            .expect("bare Option.Some should infer from payload"),
        Type::Named("Option".to_string(), vec![string_ty])
    );
}

#[test]
fn checker_class_constructor_direct_errors_cover_field_binding_edges() {
    let span = Span::new(1, 1);
    let type_names = BTreeMap::from([("Pair".to_string(), span), ("Widget".to_string(), span)]);
    let type_arities = BTreeMap::from([("Pair".to_string(), 0usize), ("Widget".to_string(), 0)]);
    let mut widget = class_info(
        "Widget",
        false,
        vec![
            ("public_value", Type::named("int32"), false),
            ("secret", Type::named("int32"), false),
        ],
    );
    widget.module_name = "pkg".to_string();
    widget.fields.get_mut("secret").unwrap().public = false;
    widget.decl.fields[1].public = false;
    let classes = BTreeMap::from([
        (
            "Pair".to_string(),
            class_info(
                "Pair",
                true,
                vec![
                    ("left", Type::named("int32"), false),
                    ("right", Type::named("int32"), false),
                ],
            ),
        ),
        ("Widget".to_string(), widget),
    ]);
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let imported_modules = BTreeMap::new();
    let module_registry = BTreeMap::new();
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
        &module_registry,
    );
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
    let pair = expr(ExprKind::Name("Pair".to_string()));
    let widget = expr(ExprKind::Name("Widget".to_string()));

    for (callee, args, expected) in [
        (
            pair.clone(),
            vec![
                named_arg("left", expr(ExprKind::Int(1))),
                arg(expr(ExprKind::Int(2))),
            ],
            "positional class constructor arguments must come before named arguments",
        ),
        (
            pair.clone(),
            vec![
                arg(expr(ExprKind::Int(1))),
                arg(expr(ExprKind::Int(2))),
                arg(expr(ExprKind::Int(3))),
            ],
            "class constructor `Pair` received too many positional arguments",
        ),
        (
            pair.clone(),
            vec![named_arg("missing", expr(ExprKind::Int(1)))],
            "class `Pair` has no field named `missing`",
        ),
        (
            pair.clone(),
            vec![
                named_arg("left", expr(ExprKind::Int(1))),
                named_arg("left", expr(ExprKind::Int(2))),
            ],
            "field `left` was provided more than once",
        ),
        (
            pair.clone(),
            vec![
                named_arg("left", expr(ExprKind::Bool(true))),
                named_arg("right", expr(ExprKind::Int(2))),
            ],
            "field `left` expects `int32`, found `bool`",
        ),
        (
            pair,
            vec![named_arg("left", expr(ExprKind::Int(1)))],
            "class constructor `Pair` is missing required field `right`",
        ),
        (
            widget.clone(),
            vec![named_arg("public_value", expr(ExprKind::Int(1)))],
            "class constructor `Widget` cannot initialize private field `secret` from another module",
        ),
        (
            widget,
            vec![
                named_arg("public_value", expr(ExprKind::Int(1))),
                named_arg("secret", expr(ExprKind::Int(2))),
            ],
            "field `secret` is private on `Widget`",
        ),
    ] {
        let error = checker
            .type_of_call(&callee, &args, span, &mut locals, None)
            .expect_err("class constructor should report a diagnostic");
        assert!(
            error.message.contains(expected),
            "expected diagnostic containing `{expected}`, got `{}`",
            error.message
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
                "def main() -> int32:\n    match true:\n        case 1:\n            return 1\n        case _:\n            return 0\n",
                "literal pattern `1` does not match scrutinee type `bool`",
            ),
            (
                "def main() -> int32:\n    match 1:\n        case 1.0:\n            return 1\n        case _:\n            return 0\n",
                "does not match scrutinee type `int64`",
            ),
            (
                "def main() -> int32:\n    match 1:\n        case _:\n            return 1\n        case 2:\n            return 2\n",
                "wildcard match arm must be the final `case`",
            ),
            (
                "def main() -> int32:\n    match 1:\n        case true:\n            return 1\n        case _:\n            return 0\n",
                "literal pattern `true` does not match scrutinee type `int64`",
            ),
            (
                "def main() -> int32:\n    match 1:\n        case \"aurora\":\n            return 1\n        case _:\n            return 0\n",
                "literal pattern \"aurora\" does not match scrutinee type `int64`",
            ),
            (
                "def main() -> int32:\n    match true:\n        case true:\n            return 1\n",
                "non-exhaustive match over `bool`: missing `false`",
            ),
            (
                "def main() -> int32:\n    match true:\n        case true:\n            return 1\n        case false:\n            return 0\n        case _:\n            return 2\n",
                "unreachable match arm",
            ),
            (
                "def main() -> int32:\n    match 1:\n        case 1:\n            return 1\n",
                "`match` over `int64` with literal patterns requires a final `case _:` arm",
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
                "enum Status:\n    Ready\n    Done\n\ndef main() -> int32:\n    status = Status.Ready\n    match status:\n        case _:\n            return 0\n        case Status.Done:\n            return 1\n",
                "wildcard match arm must be the final `case`",
            ),
            (
                "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    match status:\n        case value:\n            return 1\n",
                "top-level binding patterns are not yet supported",
            ),
            (
                "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    match status:\n        case Other.Ready:\n            return 1\n        case _:\n            return 0\n",
                "unknown enum `Other` in match pattern",
            ),
            (
                "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    match status:\n        case Status.Missing:\n            return 1\n        case _:\n            return 0\n",
                "enum `Status` has no variant `Missing`",
            ),
            (
                "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    match status:\n        case Status.Ready:\n            return 1\n        case Status.Ready:\n            return 2\n",
                "duplicate match arm for `Status.Ready`",
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
                "class Packet:\n    value: int32\n\ndef main() -> int32:\n    packet = Packet(value=1)\n    match packet:\n        case _:\n            return 0\n",
                "`match` currently requires an enum, bool, integer, float, or String scrutinee",
            ),
            (
                "enum Status:\n    Ready\n\ndef main() -> int32:\n    match 1:\n        case Status.Ready:\n            return 1\n        case _:\n            return 0\n",
                "match over `int64` only supports literal patterns and `_`",
            ),
            (
                "def main() -> int32:\n    match 1:\n        case value:\n            return 1\n",
                "top-level binding patterns are not yet supported",
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
                "`get(timeout=...)` expects `Duration`, found `int64`",
            ),
            (
                "def main() -> int32:\n    return sleep(duration=1)\n",
                "`sleep(...)` expects a `Duration`, found `int64`",
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
                "`max` arguments must match, found `int64` and `float64`",
            ),
            (
                "def main() -> int32:\n    return sqrt(value=9)\n",
                "`sqrt(...)` expects `float32` or `float64`, found `int64`",
            ),
            (
                "def main() -> int32:\n    return parse_int32(text=1)\n",
                "`parse_int32(...)` expects `String`, found `int64`",
            ),
            (
                "def main() -> int32:\n    return parse_int64(text=1)\n",
                "`parse_int64(...)` expects `String`, found `int64`",
            ),
            (
                "def main() -> int32:\n    return parse_float64(text=1)\n",
                "`parse_float64(...)` expects `String`, found `int64`",
            ),
            (
                "def main() -> int32:\n    text = \"aurora\"\n    ok: bool = text.contains(1)\n    return 0\n",
                "`contains` expects `String`, found `int64`",
            ),
            (
                "def main() -> int32:\n    text = \"aurora\"\n    replaced: String = text.replace(1, \"x\")\n    return 0\n",
                "`replace` expects `String` for `from`, found `int64`",
            ),
            (
                "def main() -> int32:\n    text = \"aurora\"\n    replaced: String = text.replace(\"a\", 1)\n    return 0\n",
                "`replace` expects `String` for `to`, found `int64`",
            ),
            (
                "def main() -> None:\n    mut values = [1]\n    values.push(\"x\")\n",
                "`push` expects `int64`, found `String`",
            ),
            (
                "import fs\n\ndef main() -> int32:\n    file = fs.File()\n    return 0\n",
                "builtin resource `fs.File` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    stream = net.TcpStream()\n    return 0\n",
                "builtin resource `net.TcpStream` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    listener = net.TcpListener()\n    return 0\n",
                "builtin resource `net.TcpListener` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    socket = net.UdpSocket()\n    return 0\n",
                "builtin resource `net.UdpSocket` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    datagram = net.UdpDatagram()\n    return 0\n",
                "builtin resource `net.UdpDatagram` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    listener = net.HttpListener()\n    return 0\n",
                "builtin resource `net.HttpListener` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    exchange = net.HttpExchange()\n    return 0\n",
                "builtin resource `net.HttpExchange` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    response = net.HttpResponse()\n    return 0\n",
                "builtin resource `net.HttpResponse` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    listener = net.WebSocketListener()\n    return 0\n",
                "builtin resource `net.WebSocketListener` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    socket = net.WebSocket()\n    return 0\n",
                "builtin resource `net.WebSocket` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    listener = net.UnixListener()\n    return 0\n",
                "builtin resource `net.UnixListener` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    stream = net.UnixStream()\n    return 0\n",
                "builtin resource `net.UnixStream` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    listener = net.TlsListener()\n    return 0\n",
                "builtin resource `net.TlsListener` must be created through its module functions",
            ),
            (
                "import net\n\ndef main() -> int32:\n    stream = net.TlsStream()\n    return 0\n",
                "builtin resource `net.TlsStream` must be created through its module functions",
            ),
            (
                "class Resource[T]:\n    value: T\n\n    def close(borrow mut self):\n        pass\n\ndef main() -> None:\n    resource = Resource[int32](value=1)\n    with resource as handle:\n        pass\n",
                "`with` does not yet support generic resource types",
            ),
            (
                "class Resource:\n    value: int32\n\ndef main() -> None:\n    resource = Resource(value=1)\n    with resource as handle:\n        pass\n",
                "does not define `close(borrow mut self)`",
            ),
            (
                "class Resource:\n    value: int32\n\n    def close(self) -> int32:\n        return 0\n\ndef main() -> None:\n    resource = Resource(value=1)\n    with resource as handle:\n        pass\n",
                "`with` resources must define `close(borrow mut self)` returning `None`",
            ),
            (
                "def make() -> Option[int32]:\n    return Option[int32].Missing()\n\ndef main():\n    pass\n",
                "enum `Option` has no variant `Missing`",
            ),
            (
                "def make() -> Option[int32]:\n    return Option[int32].None(1)\n\ndef main():\n    pass\n",
                "variant `None` of enum `Option` does not take a payload",
            ),
            (
                "def make() -> Option[int32]:\n    return Option[int32].Some()\n\ndef main():\n    pass\n",
                "variant `Some` of enum `Option` expects 1 payload argument, found 0",
            ),
            (
                "def make() -> Option[int32]:\n    return Option[int32].Some(\"x\")\n\ndef main():\n    pass\n",
                "variant `Some` of enum `Option` expects `int32`, found `String`",
            ),
            (
                "def make() -> Option[int32]:\n    return Option.None(1)\n\ndef main():\n    pass\n",
                "variant `None` of enum `Option` does not take a payload",
            ),
            (
                "def make() -> Option[int32]:\n    return Option[int32].Some(value=1, extra=2)\n\ndef main():\n    pass\n",
                "variant `Some` of enum `Option` expects 1 payload argument, found 2",
            ),
            (
                "enum Pair:\n    Both(int32, int32)\n\ndef make() -> Pair:\n    return Pair.Both(left=1, right=2)\n\ndef main():\n    pass\n",
                "variant `Both` of enum `Pair` uses positional payloads and cannot be constructed with named arguments",
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

    for (source, expected) in [
        (
            "class Packet:\n    value: int32\n\ndef main() -> int32:\n    packet = Packet(value=1)\n    return match packet:\n        case _: 0\n",
            "`match` currently requires an enum, bool, integer, float, or String scrutinee",
        ),
        (
            "enum Status:\n    Ready\n\ndef main() -> int32:\n    return match 1:\n        case Status.Ready: 1\n        case _: 0\n",
            "match over `int64` only supports literal patterns and `_`",
        ),
        (
            "def main() -> int32:\n    return match 1:\n        case value: 1\n",
            "top-level binding patterns are not yet supported",
        ),
        (
            "def main() -> int32:\n    return match 1:\n        case _: 0\n        case 1: 1\n",
            "wildcard match arm must be the final `case`",
        ),
        (
            "def main() -> int32:\n    return match 1:\n        case 1: 1\n        case 1: 2\n        case _: 3\n",
            "unreachable match arm",
        ),
        (
            "def main() -> int32:\n    return match true:\n        case true: 1\n",
            "non-exhaustive bool match: missing `false`",
        ),
        (
            "def main() -> int32:\n    return match 1:\n        case 1: 1\n",
            "match over `int64` requires a final wildcard arm because the domain is open-ended",
        ),
        (
            "def main() -> int32:\n    return match true:\n        case true: 1\n        case false: \"no\"\n",
            "match arm expression expects `int32`, found `String`",
        ),
        (
            "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    return match status:\n        case 1: 1\n        case _: 0\n",
            "match over `Status` expects enum variant patterns, not literal `1`",
        ),
        (
            "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    return match status:\n        case value: 1\n",
            "top-level binding patterns are not yet supported",
        ),
        (
            "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    return match status:\n        case Other.Ready: 1\n        case _: 0\n",
            "unknown enum `Other` in match pattern",
        ),
        (
            "enum Status:\n    Ready\n\nenum Other:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    return match status:\n        case Other.Ready: 1\n        case _: 0\n",
            "match arm expects enum `Status`, found pattern for `Other`",
        ),
        (
            "enum Status:\n    Ready\n    Done\n\ndef main() -> int32:\n    status = Status.Ready\n    return match status:\n        case _: 0\n        case Status.Done: 1\n",
            "wildcard match arm must be the final `case`",
        ),
        (
            "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    return match status:\n        case Status.Missing: 1\n        case _: 0\n",
            "enum `Status` has no variant `Missing`",
        ),
        (
            "enum Status:\n    Done(int32)\n\ndef main() -> int32:\n    status = Status.Done(1)\n    return match status:\n        case Status.Done: 1\n        case _: 0\n",
            "variant `Status.Done` carries a payload and must bind it",
        ),
        (
            "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    return match status:\n        case Status.Ready(value): 1\n        case _: 0\n",
            "variant `Status.Ready` does not carry a payload",
        ),
        (
            "enum Status:\n    Ready\n\ndef main() -> int32:\n    status = Status.Ready\n    return match status:\n        case Status.Ready: 1\n        case Status.Ready: 2\n",
            "unreachable match arm",
        ),
        (
            "enum Status:\n    Ready\n    Done\n\ndef main() -> int32:\n    status = Status.Ready\n    return match status:\n        case Status.Ready: 1\n        case Status.Done: \"no\"\n",
            "match arm expression expects `int32`, found `String`",
        ),
        (
            "enum Status:\n    Ready\n    Done\n\ndef main() -> int32:\n    status = Status.Ready\n    return match status:\n        case Status.Ready: 1\n",
            "non-exhaustive match over `Status`: missing `Done`",
        ),
    ] {
        let error = crate::check_source(source)
            .expect_err("checker match expression surface should report a diagnostic");
        assert!(
            error.message.contains(expected),
            "expected diagnostic containing `{expected}`, got `{}` for source:\n{}",
            error.message,
            source
        );
    }

    let span = Span::new(1, 1);
    let enums = BTreeMap::from([
        ("Other".to_string(), enum_info("Other", None)),
        (
            "PayloadStatus".to_string(),
            enum_info("PayloadStatus", Some(Type::named("int32"))),
        ),
        ("Status".to_string(), enum_info("Status", None)),
    ]);
    let type_names = BTreeMap::from([
        ("Other".to_string(), span),
        ("PayloadStatus".to_string(), span),
        ("Status".to_string(), span),
    ]);
    let type_arities = BTreeMap::from([
        ("Other".to_string(), 0usize),
        ("PayloadStatus".to_string(), 0usize),
        ("Status".to_string(), 0usize),
    ]);
    let classes = BTreeMap::new();
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
    let mut locals = HashMap::from([(
        "status".to_string(),
        local_binding(
            Type::named("Status"),
            false,
            false,
            ReceiverKind::Value,
            false,
            &[],
        ),
    )]);
    let empty_match_expr = expr(ExprKind::Match {
        scrutinee: Box::new(expr(ExprKind::Name("status".to_string()))),
        borrow_mode: None,
        arms: Vec::new(),
    });
    assert!(checker
        .type_of_expr(&empty_match_expr, &mut locals)
        .expect_err("empty enum match expression should be rejected")
        .message
        .contains("`match` requires at least one `case` arm"));

    let variant_pattern =
        |enum_name: Option<&str>, variant_name: &str, subpatterns: Vec<Pattern>| {
            Pattern::Variant(crate::ast::VariantPattern {
                enum_name: enum_name.map(str::to_string),
                variant_name: variant_name.to_string(),
                subpatterns,
                span,
            })
        };
    for (pattern, expected_ty, expected) in [
        (
            variant_pattern(None, "Value", Vec::new()),
            Type::named("int32"),
            "pattern `Value` expects an enum scrutinee, found `int32`",
        ),
        (
            variant_pattern(Some("Other"), "Value", Vec::new()),
            Type::named("Status"),
            "match arm expects enum `Status`, found pattern for `Other`",
        ),
        (
            variant_pattern(None, "Missing", Vec::new()),
            Type::named("Status"),
            "enum `Status` has no variant `Missing`",
        ),
        (
            variant_pattern(None, "Value", Vec::new()),
            Type::named("PayloadStatus"),
            "variant `PayloadStatus.Value` carries a payload and must bind it",
        ),
        (
            variant_pattern(None, "Value", vec![Pattern::Wildcard(span)]),
            Type::named("Status"),
            "variant `Status.Value` does not carry a payload",
        ),
    ] {
        assert!(
            checker
                .bind_pattern_locals(&pattern, &expected_ty, &mut locals, None, None)
                .expect_err("direct pattern binding diagnostic should be reported")
                .message
                .contains(expected),
            "expected direct pattern diagnostic containing `{expected}`"
        );
    }

    let empty_match = crate::ast::Stmt::Match(crate::ast::MatchStmt {
        scrutinee: expr(ExprKind::Name("status".to_string())),
        borrow_mode: None,
        arms: Vec::new(),
        span,
    });
    assert!(checker
        .check_block(&[empty_match], &mut locals, &Type::named("int32"), 0, true)
        .expect_err("empty enum match should be rejected")
        .message
        .contains("`match` requires at least one `case` arm"));
}

#[test]
fn checker_module_member_type_edges_cover_private_and_uncalled_members() {
    let span = Span::new(1, 1);
    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();
    let enums = BTreeMap::new();
    let functions = BTreeMap::new();
    let traits = BTreeMap::new();
    let mut imported_modules = BTreeMap::new();
    let mut module_registry = BTreeMap::new();
    let mut root = namespace("pkg");
    root.functions.insert(
        "make".to_string(),
        FunctionInfo {
            module_name: "pkg".to_string(),
            decl: function_decl("make"),
            signature: function_signature(Vec::new(), Type::Unit),
            type_param_bounds: BTreeMap::new(),
        },
    );
    let mut widget = class_info(
        "Widget",
        false,
        vec![
            ("value", Type::named("int32"), false),
            ("secret", Type::named("String"), false),
        ],
    );
    widget.module_name = "pkg".to_string();
    widget.fields.get_mut("secret").unwrap().public = false;
    widget.decl.fields[1].public = false;
    let mut hidden = function_decl("hidden");
    hidden.public = false;
    hidden.receiver = Some(ReceiverKind::Borrow);
    widget.methods.insert(
        "hidden".to_string(),
        MethodInfo {
            decl: hidden,
            signature: function_signature(Vec::new(), Type::named("String")),
            type_param_bounds: BTreeMap::new(),
        },
    );
    root.classes.insert("Widget".to_string(), widget.clone());
    root.enums
        .insert("Status".to_string(), enum_info("Status", None));
    imported_modules.insert("pkg".to_string(), root.clone());
    module_registry.insert("pkg".to_string(), root);
    let classes = BTreeMap::from([("Widget".to_string(), widget)]);
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
        &module_registry,
    );

    for (object_ty, field, expected) in [
        (
            Type::Module("pkg".to_string()),
            "make",
            "function `make` from module `pkg` must be called with `(...)`",
        ),
        (
            Type::Module("pkg".to_string()),
            "Widget",
            "class `Widget` from module `pkg` must be constructed with `(...)`",
        ),
        (
            Type::Module("pkg".to_string()),
            "missing",
            "module `pkg` has no member `missing`",
        ),
        (
            Type::Named("Option".to_string(), vec![Type::named("int32")]),
            "Some",
            "variant `Some` of enum `Option` requires a payload",
        ),
        (
            Type::named("MapEntry"),
            "other",
            "type `MapEntry` has no field `other`",
        ),
        (
            Type::named("Widget"),
            "secret",
            "field `secret` is private on `Widget`",
        ),
        (
            Type::named("Widget"),
            "hidden",
            "method `hidden` is private on `Widget`",
        ),
    ] {
        let error = checker
            .resolve_member_type(&object_ty, field, span)
            .expect_err("member type lookup should report the expected diagnostic");
        assert!(
            error.message.contains(expected),
            "expected diagnostic containing `{expected}`, got `{}`",
            error.message
        );
    }
}

#[test]
fn operator_trait_and_bound_helpers_cover_checker_resolution_paths() {
    let bad_ord = crate::check_source(
        "\
trait Ord[Rhs]:
    def lt(borrow self, rhs: Rhs) -> Score

class Score:
    value: int32

impl Ord[Score] for Score:
    def lt(borrow self, rhs: Score) -> Score:
        return self

def main() -> int32:
    left = Score(value=1)
    right = Score(value=2)
    if left < right:
        return 1
    return 0
",
    )
    .expect_err("ordering operator traits must return bool");
    assert!(bad_ord
        .message
        .contains("operator trait `Ord` for `lt` must return `bool`"));

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

class Box[T]:
    value: T

impl Named for User:
    def name(borrow self) -> String:
        return self.label.clone()

impl Add[Point, Point] for Point:
    def add(borrow self, rhs: Point) -> Point:
        return Point(x=self.x + rhs.x)

impl Neg[Point] for Point:
    def neg(borrow self) -> Point:
        return Point(x=0 - self.x)

impl[T: Named] Add[Box[T], Box[T]] for Box[T]:
    def add(borrow self, rhs: Box[T]) -> Box[T]:
        return rhs

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
    let box_user = Type::Named("Box".to_string(), vec![Type::named("User")]);
    assert_eq!(
        base_checker
            .type_of_binary_operator_via_trait(span, BinaryOp::Add, &box_user, &box_user)
            .expect("generic add impl should satisfy its Named bound for User"),
        Some(box_user.clone())
    );
    let box_point = Type::Named("Box".to_string(), vec![Type::named("Point")]);
    assert_eq!(
        base_checker
            .type_of_binary_operator_via_trait(span, BinaryOp::Add, &box_point, &box_point)
            .expect("generic add impl with unsatisfied bounds should be ignored"),
        None
    );
    assert_eq!(
        base_checker
            .type_of_binary_operator_via_trait(
                span,
                BinaryOp::And,
                &Type::named("bool"),
                &Type::named("bool"),
            )
            .expect("boolean operators do not resolve through traits"),
        None
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

    let no_traits = BTreeMap::new();
    let no_trait_impls = Vec::new();
    let no_trait_checker = checker(
        &program.module_name,
        &type_names,
        &type_arities,
        &program.classes,
        &program.enums,
        &program.functions,
        &no_traits,
        &no_trait_impls,
        &program.imported_modules,
        &program.module_registry,
    )
    .with_type_params(BTreeMap::from([("T".to_string(), ())]), BTreeMap::new());
    assert!(no_trait_checker
        .operator_method_from_type_param("T", "Add", "add", Some(&Type::named("Point")))
        .expect("missing operator traits should be ignored for type params")
        .is_none());

    let mut broken_add_trait = program.traits["Add"].clone();
    broken_add_trait.methods.clear();
    let broken_traits = BTreeMap::from([("Add".to_string(), broken_add_trait)]);
    let broken_trait_checker = checker(
        &program.module_name,
        &type_names,
        &type_arities,
        &program.classes,
        &program.enums,
        &program.functions,
        &broken_traits,
        &program.trait_impls,
        &program.imported_modules,
        &program.module_registry,
    )
    .with_type_params(BTreeMap::from([("T".to_string(), ())]), BTreeMap::new());
    let missing_method = match broken_trait_checker.operator_method_from_type_param(
        "T",
        "Add",
        "add",
        Some(&Type::named("Point")),
    ) {
        Ok(_) => panic!("operator traits must expose the expected method"),
        Err(error) => error,
    };
    assert!(missing_method
        .message
        .contains("operator trait `Add` must define method `add`"));

    let named_only_checker = base_checker.with_type_params(
        BTreeMap::from([("T".to_string(), ())]),
        BTreeMap::from([(
            "T".to_string(),
            vec![TraitBound {
                trait_name: "Named".to_string(),
                trait_args: Vec::new(),
            }],
        )]),
    );
    assert!(named_only_checker
        .operator_method_from_type_param("T", "Add", "add", Some(&Type::named("Point")))
        .expect("unrelated type-param bounds should not match Add")
        .is_none());

    let wrong_rhs_checker = base_checker.with_type_params(
        BTreeMap::from([("T".to_string(), ())]),
        BTreeMap::from([(
            "T".to_string(),
            vec![TraitBound {
                trait_name: "Add".to_string(),
                trait_args: vec![Type::named("String"), Type::named("Point")],
            }],
        )]),
    );
    assert!(wrong_rhs_checker
        .operator_method_from_type_param("T", "Add", "add", Some(&Type::named("Point")))
        .expect("type-param Add bounds with the wrong rhs should not match")
        .is_none());

    let add_point_impl = program
        .trait_impls
        .iter()
        .find(|trait_impl| {
            trait_impl.trait_name == "Add" && trait_impl.for_type == Type::named("Point")
        })
        .expect("Point Add impl should be present")
        .clone();

    let mut missing_impl_method = add_point_impl.clone();
    missing_impl_method.methods.clear();
    let missing_impl_methods = vec![missing_impl_method];
    let missing_impl_checker = checker(
        &program.module_name,
        &type_names,
        &type_arities,
        &program.classes,
        &program.enums,
        &program.functions,
        &program.traits,
        &missing_impl_methods,
        &program.imported_modules,
        &program.module_registry,
    );
    assert!(missing_impl_checker
        .operator_method_for_concrete_type(
            span,
            &Type::named("Point"),
            "Add",
            "add",
            Some(&Type::named("Point")),
        )
        .expect("impls missing the operator method should be skipped")
        .is_none());

    let mut wrong_rhs_impl = add_point_impl.clone();
    wrong_rhs_impl.trait_args = vec![Type::named("String"), Type::named("Point")];
    let wrong_rhs_impls = vec![wrong_rhs_impl];
    let wrong_rhs_impl_checker = checker(
        &program.module_name,
        &type_names,
        &type_arities,
        &program.classes,
        &program.enums,
        &program.functions,
        &program.traits,
        &wrong_rhs_impls,
        &program.imported_modules,
        &program.module_registry,
    );
    assert!(wrong_rhs_impl_checker
        .operator_method_for_concrete_type(
            span,
            &Type::named("Point"),
            "Add",
            "add",
            Some(&Type::named("Point")),
        )
        .expect("impls with mismatched rhs patterns should be skipped")
        .is_none());
    assert!(base_checker
        .operator_method_for_concrete_type(span, &Type::named("Point"), "Add", "add", None)
        .expect("binary traits should not match unary lookup shapes")
        .is_none());

    let mut unbound_generic_impl = add_point_impl;
    unbound_generic_impl.type_param_bounds = BTreeMap::from([(
        "T".to_string(),
        vec![TraitBound {
            trait_name: "Named".to_string(),
            trait_args: Vec::new(),
        }],
    )]);
    let unbound_generic_impls = vec![unbound_generic_impl];
    let unbound_generic_checker = checker(
        &program.module_name,
        &type_names,
        &type_arities,
        &program.classes,
        &program.enums,
        &program.functions,
        &program.traits,
        &unbound_generic_impls,
        &program.imported_modules,
        &program.module_registry,
    );
    assert!(unbound_generic_checker
        .operator_method_for_concrete_type(
            span,
            &Type::named("Point"),
            "Add",
            "add",
            Some(&Type::named("Point")),
        )
        .expect("impl bounds for unbound type params should invalidate the impl")
        .is_none());
}

#[test]
fn concrete_operator_trait_resolution_reports_ambiguity_for_equal_specificity_impls() {
    let error = crate::check_source(
        "\
trait Add[Rhs, Out]:
    def add(borrow self, rhs: Rhs) -> Out

class Pair[A, B]:
    left: A
    right: B

impl[T] Add[Pair[int32, T], Pair[int32, T]] for Pair[int32, T]:
    def add(borrow self, rhs: Pair[int32, T]) -> Pair[int32, T]:
        return Pair(left=self.left + rhs.left, right=rhs.right)

impl[T] Add[Pair[T, int32], Pair[T, int32]] for Pair[T, int32]:
    def add(borrow self, rhs: Pair[T, int32]) -> Pair[T, int32]:
        return Pair(left=rhs.left, right=self.right + rhs.right)

def main() -> int32:
    left: Pair[int32, int32] = Pair(left=1, right=2)
    right: Pair[int32, int32] = Pair(left=3, right=4)
    total = left + right
    return total.left
",
    )
    .expect_err("equally specific concrete operator impls should be ambiguous");
    assert!(error
        .message
        .contains("operator trait `Add` is ambiguous for type `Pair[int32, int32]`"));
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
    let string_ty = Type::named("String");
    let int_ty = Type::named("int32");
    let builtin_payload_cases = [
        (
            Type::Named(
                "Result".to_string(),
                vec![int_ty.clone(), string_ty.clone()],
            ),
            "Result",
            "Ok",
            vec![int_ty.clone()],
        ),
        (
            Type::Named(
                "Result".to_string(),
                vec![int_ty.clone(), string_ty.clone()],
            ),
            "Result",
            "Err",
            vec![string_ty.clone()],
        ),
        (
            Type::Named("SendError".to_string(), vec![int_ty.clone()]),
            "SendError",
            "Closed",
            vec![int_ty.clone()],
        ),
        (
            Type::Named("QueueReceive".to_string(), vec![int_ty.clone()]),
            "QueueReceive",
            "Item",
            vec![int_ty.clone()],
        ),
        (
            Type::Named("QueueReceive".to_string(), vec![int_ty.clone()]),
            "QueueReceive",
            "TimedOut",
            Vec::new(),
        ),
        (
            Type::Named("TaskResult".to_string(), vec![int_ty.clone()]),
            "TaskResult",
            "Ready",
            vec![int_ty.clone()],
        ),
        (
            Type::Named("TaskResult".to_string(), vec![int_ty.clone()]),
            "TaskResult",
            "Error",
            vec![string_ty.clone()],
        ),
        (
            Type::Named("TaskResult".to_string(), vec![int_ty.clone()]),
            "TaskResult",
            "Cancelled",
            Vec::new(),
        ),
        (
            Type::Named("WaitAny".to_string(), vec![int_ty.clone()]),
            "WaitAny",
            "Ready",
            vec![int_ty.clone(), int_ty.clone()],
        ),
        (
            Type::Named("WaitAny".to_string(), vec![int_ty.clone()]),
            "WaitAny",
            "Error",
            vec![int_ty.clone(), string_ty.clone()],
        ),
        (
            Type::Named("WaitAny".to_string(), vec![int_ty.clone()]),
            "WaitAny",
            "TimedOut",
            Vec::new(),
        ),
        (
            Type::Named(
                "WaitAll".to_string(),
                vec![Type::Named("Vec".to_string(), vec![int_ty.clone()])],
            ),
            "WaitAll",
            "Ready",
            vec![Type::Named("Vec".to_string(), vec![int_ty.clone()])],
        ),
        (
            Type::Named("WaitAll".to_string(), vec![int_ty.clone()]),
            "WaitAll",
            "Error",
            vec![int_ty.clone(), string_ty.clone()],
        ),
        (
            Type::Named("WaitAll".to_string(), vec![int_ty.clone()]),
            "WaitAll",
            "Cancelled",
            Vec::new(),
        ),
    ];
    for (expected, enum_name, variant_name, payload) in builtin_payload_cases {
        assert_eq!(
            checker.builtin_enum_variant_payload(&expected, enum_name, variant_name),
            Some(payload),
            "{enum_name}.{variant_name}"
        );
    }
    assert_eq!(
        checker.builtin_enum_variant_payload(&Type::Unit, "Option", "Some"),
        None
    );
    assert_eq!(
        checker.builtin_enum_variant_payload(
            &Type::Named("Option".to_string(), vec![int_ty.clone()]),
            "Result",
            "Ok",
        ),
        None
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
    for (name, args) in [
        ("SendError", vec![int_ty.clone()]),
        ("QueueReceive", vec![int_ty.clone()]),
        ("TaskResult", vec![int_ty.clone()]),
        ("WaitAny", vec![int_ty.clone()]),
        ("WaitAll", vec![int_ty.clone()]),
    ] {
        assert_eq!(
            checker
                .explicit_builtin_type(name, &args, span)
                .expect("builtin enum specialization should accept the maintained arity"),
            Type::Named(name.to_string(), args),
            "{name}"
        );
    }
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
    let qualified_missing_enum_value = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
                field: "tools".to_string(),
            })),
            field: "NotAnEnum".to_string(),
        })),
        field: "Value".to_string(),
    });

    let missing_variant_expr = checker
        .type_of_expr(&qualified_status_missing, &mut locals)
        .expect_err("missing qualified variants should fail as expressions");
    assert!(missing_variant_expr
        .message
        .contains("enum `Status` has no variant `Missing`"));

    let missing_enum_expr = checker
        .type_of_expr(&qualified_missing_enum_value, &mut locals)
        .expect_err("qualified non-enum members should fall through to module member errors");
    assert!(missing_enum_expr
        .message
        .contains("module `pkg.tools` has no member `NotAnEnum`"));

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
    let mut merge_decl = function_decl("merge");
    merge_decl.params = vec![
        Param {
            name: "left".to_string(),
            ty: type_ref("Widget"),
            passing: ReceiverKind::BorrowMut,
            borrow_label: None,
            default: None,
            span,
        },
        Param {
            name: "right".to_string(),
            ty: type_ref("Widget"),
            passing: ReceiverKind::BorrowMut,
            borrow_label: None,
            default: None,
            span,
        },
    ];
    math_widget.methods.insert(
        "merge".to_string(),
        MethodInfo {
            decl: merge_decl.clone(),
            signature: function_signature(
                vec![Type::named("Widget"), Type::named("Widget")],
                Type::Unit,
            ),
            type_param_bounds: BTreeMap::new(),
        },
    );
    math.classes.insert("Widget".to_string(), math_widget);
    let mut math_secret = class_info(
        "SecretBox",
        false,
        vec![("secret", Type::named("int32"), false)],
    );
    math_secret.module_name = "helpers.math".to_string();
    math_secret.decl.fields[0].public = false;
    math_secret
        .fields
        .get_mut("secret")
        .expect("secret field should exist")
        .public = false;
    math.classes.insert("SecretBox".to_string(), math_secret);
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
    let secret_expr = expr(ExprKind::Member {
        object: Box::new(math_expr.clone()),
        field: "SecretBox".to_string(),
    });
    let qualified_status_value_expr = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Member {
            object: Box::new(math_expr.clone()),
            field: "Status".to_string(),
        })),
        field: "Value".to_string(),
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
        root_checker.infer_module_path(&expr(ExprKind::Index {
            object: Box::new(math_expr.clone()),
            index: Box::new(expr(ExprKind::Int(0))),
        })),
        Some("helpers.math".to_string())
    );
    assert_eq!(
        root_checker.infer_module_path(&expr(ExprKind::Bool(true))),
        None
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
    assert!(root_checker
        .resolve_class_info("helpers.math.Missing")
        .is_none());
    assert!(root_checker
        .resolve_enum_info("helpers.math.Missing")
        .is_none());
    assert_eq!(
        root_checker.canonical_enum_name("helpers.math.Missing"),
        "Missing"
    );
    let mut root_locals = HashMap::from([
        (
            "helpers".to_string(),
            local_binding(
                Type::Module("helpers".to_string()),
                false,
                false,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "left".to_string(),
            local_binding(
                Type::named("Widget"),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
        (
            "right".to_string(),
            local_binding(
                Type::named("Widget"),
                true,
                true,
                ReceiverKind::Value,
                false,
                &[],
            ),
        ),
    ]);
    assert_eq!(
        root_checker
            .type_of_call(
                &widget_expr,
                &[arg(expr(ExprKind::Int(1)))],
                span,
                &mut root_locals,
                None,
            )
            .expect("module-qualified class constructors should type check"),
        Type::named("Widget")
    );
    assert!(root_checker
        .type_of_call(
            &widget_expr,
            &[arg(expr(ExprKind::Int(1))), arg(expr(ExprKind::Int(2)))],
            span,
            &mut root_locals,
            None,
        )
        .expect_err("module-qualified constructors should reject extra positional arguments")
        .message
        .contains("received too many positional arguments"));
    assert!(root_checker
        .type_of_call(
            &secret_expr,
            &[named_arg("secret", expr(ExprKind::Int(1)))],
            span,
            &mut root_locals,
            None,
        )
        .expect_err("external module constructors should reject private fields")
        .message
        .contains("field `secret` is private on `SecretBox`"));
    assert!(root_checker
        .type_of_call(&secret_expr, &[], span, &mut root_locals, None)
        .expect_err("external module constructors should not infer private fields")
        .message
        .contains("cannot initialize private field `secret` from another module"));
    assert!(root_checker
        .type_of_expr(&qualified_status_value_expr, &mut root_locals)
        .expect_err("module-qualified payload variants should require construction")
        .message
        .contains("requires a payload"));

    let merge_expr = expr(ExprKind::Member {
        object: Box::new(widget_expr.clone()),
        field: "merge".to_string(),
    });
    let mut borrowed_places = Vec::new();
    root_checker
        .collect_call_borrowed_places(
            &merge_expr,
            &[
                named_arg("left", expr(ExprKind::Name("left".to_string()))),
                named_arg("right", expr(ExprKind::Name("right".to_string()))),
            ],
            &root_locals,
            &mut borrowed_places,
        )
        .expect("module-qualified class methods should collect borrowed arguments");
    assert_eq!(borrowed_places.len(), 2);
    assert_eq!(borrowed_places[0].path, place_path("left"));
    assert_eq!(borrowed_places[1].path, place_path("right"));

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
fn spawn_callable_resolution_covers_module_and_associated_targets() {
    let type_names = BTreeMap::new();
    let type_arities = BTreeMap::new();
    let mut worker = class_info("Worker", false, vec![]);
    worker.methods.insert(
        "make".to_string(),
        MethodInfo {
            decl: function_decl("make"),
            signature: function_signature(Vec::new(), Type::named("int32")),
            type_param_bounds: BTreeMap::new(),
        },
    );
    let mut touch_decl = function_decl("touch");
    touch_decl.receiver = Some(ReceiverKind::Borrow);
    worker.methods.insert(
        "touch".to_string(),
        MethodInfo {
            decl: touch_decl,
            signature: function_signature(Vec::new(), Type::Unit),
            type_param_bounds: BTreeMap::new(),
        },
    );
    let classes = BTreeMap::from([("Worker".to_string(), worker)]);
    let enums = BTreeMap::new();
    let functions = BTreeMap::from([(
        "job".to_string(),
        FunctionInfo {
            module_name: "<main>".to_string(),
            decl: function_decl("job"),
            signature: function_signature(Vec::new(), Type::named("int32")),
            type_param_bounds: BTreeMap::new(),
        },
    )]);
    let traits = BTreeMap::new();

    let remote_job = FunctionInfo {
        module_name: "pkg.tools".to_string(),
        decl: function_decl("remote_job"),
        signature: function_signature(Vec::new(), Type::Unit),
        type_param_bounds: BTreeMap::new(),
    };
    let mut remote_worker = class_info("RemoteWorker", false, vec![]);
    remote_worker.module_name = "pkg.tools".to_string();
    remote_worker.methods.insert(
        "make".to_string(),
        MethodInfo {
            decl: function_decl("make"),
            signature: function_signature(Vec::new(), Type::named("bool")),
            type_param_bounds: BTreeMap::new(),
        },
    );
    let mut tools = namespace("pkg.tools");
    tools
        .all_functions
        .insert("remote_job".to_string(), remote_job);
    tools
        .classes
        .insert("RemoteWorker".to_string(), remote_worker);
    let mut pkg = namespace("pkg");
    pkg.modules.insert("tools".to_string(), tools.clone());
    let imported_modules = BTreeMap::from([("pkg".to_string(), pkg.clone())]);
    let module_registry =
        BTreeMap::from([("pkg".to_string(), pkg), ("pkg.tools".to_string(), tools)]);

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

    let local_job = checker
        .resolve_spawn_callable(&expr(ExprKind::Name("job".to_string())))
        .expect("named local functions should be task-start targets");
    assert_eq!(local_job.display_name, "job");
    assert_eq!(local_job.signature.return_type, Type::named("int32"));

    let local_static = checker
        .resolve_spawn_callable(&expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Name("Worker".to_string()))),
            field: "make".to_string(),
        }))
        .expect("static associated methods should be task-start targets");
    assert_eq!(local_static.display_name, "Worker.make");
    assert!(checker
        .resolve_spawn_callable(&expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Worker".to_string()))),
                type_args: Vec::new(),
            })),
            field: "make".to_string(),
        }))
        .is_ok());

    let pkg_tools = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
        field: "tools".to_string(),
    });
    let remote_function = checker
        .resolve_spawn_callable(&expr(ExprKind::Member {
            object: Box::new(pkg_tools.clone()),
            field: "remote_job".to_string(),
        }))
        .expect("module-qualified functions should be task-start targets");
    assert_eq!(remote_function.display_name, "pkg.tools.remote_job");
    let missing_remote_function = match checker.resolve_spawn_callable(&expr(ExprKind::Member {
        object: Box::new(pkg_tools.clone()),
        field: "missing".to_string(),
    })) {
        Ok(_) => panic!("missing module-qualified functions should not be task-start targets"),
        Err(error) => error,
    };
    assert!(missing_remote_function
        .message
        .contains("task starting currently supports named functions"));

    let remote_static = checker
        .resolve_spawn_callable(&expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Member {
                object: Box::new(pkg_tools),
                field: "RemoteWorker".to_string(),
            })),
            field: "make".to_string(),
        }))
        .expect("module-qualified static methods should be task-start targets");
    assert_eq!(remote_static.display_name, "RemoteWorker.make");
    assert_eq!(remote_static.signature.return_type, Type::named("bool"));

    let receiver_method = match checker.resolve_spawn_callable(&expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("Worker".to_string()))),
        field: "touch".to_string(),
    })) {
        Ok(_) => panic!("receiver methods should not be task-start targets"),
        Err(error) => error,
    };
    assert!(receiver_method
        .message
        .contains("task starting currently supports named functions"));
    let missing_name =
        match checker.resolve_spawn_callable(&expr(ExprKind::Name("missing".to_string()))) {
            Ok(_) => panic!("unknown names should not be task-start targets"),
            Err(error) => error,
        };
    assert!(missing_name
        .message
        .contains("task start target must be a callable function"));
    let non_callable = match checker.resolve_spawn_callable(&expr(ExprKind::Int(1))) {
        Ok(_) => panic!("non-call expressions should not be task-start targets"),
        Err(error) => error,
    };
    assert!(non_callable
        .message
        .contains("task starting currently supports named functions"));
    assert!(checker
        .resolve_spawn_callable(&expr(ExprKind::Specialize {
            expr: Box::new(expr(ExprKind::Name("job".to_string()))),
            type_args: Vec::new(),
        }))
        .is_ok());
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
                moved_fields: BTreeSet::from([
                    projection_path("other"),
                    projection_path("value.inner"),
                ]),
                frozen_places: BTreeSet::new(),
            },
        ),
        (
            "items".to_string(),
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
            "moved".to_string(),
            local_binding(
                Type::named("Counter"),
                true,
                true,
                ReceiverKind::Value,
                true,
                &[],
            ),
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
        Some(place_path("counter.value"))
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
        Some("self".to_string())
    );
    assert_eq!(
        checker.member_access_path(&member_expr),
        Some(place_path("counter.value"))
    );
    assert_eq!(
        checker.member_target_path(&expr(ExprKind::Name("counter".to_string())), "value"),
        Some(place_path("counter.value"))
    );
    assert!(FunctionChecker::field_path_is_moved(
        locals.get("counter").unwrap(),
        &projection_path("value")
    ));
    let binding = locals.get_mut("counter").unwrap();
    FunctionChecker::clear_moved_field_path(binding, &projection_path("value"));
    assert!(!FunctionChecker::field_path_is_moved(
        binding,
        &projection_path("value")
    ));
    assert!(binding.moved_fields.contains(&projection_path("other")));

    assert_eq!(
        checker
            .type_of_member_object_expr(
                &expr(ExprKind::Group(Box::new(member_expr.clone()))),
                &mut locals
            )
            .expect("grouped member objects should resolve through the inner object"),
        Type::named("int32")
    );
    assert_eq!(
        checker
            .type_of_member_object_expr(
                &expr(ExprKind::Cast {
                    expr: Box::new(expr(ExprKind::Name("counter".to_string()))),
                    ty: type_ref("Counter"),
                }),
                &mut locals,
            )
            .expect("cast member objects should resolve through the inner object"),
        Type::named("Counter")
    );
    assert_eq!(
        checker
            .type_of_member_object_expr(
                &expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("counter".to_string()))),
                    type_args: Vec::new(),
                }),
                &mut locals,
            )
            .expect("specialized member objects should resolve through the inner object"),
        Type::named("Counter")
    );
    assert_eq!(
        checker
            .type_of_member_object_expr(
                &expr(ExprKind::Index {
                    object: Box::new(expr(ExprKind::Name("items".to_string()))),
                    index: Box::new(expr(ExprKind::Int(0))),
                }),
                &mut locals,
            )
            .expect("indexed member objects should resolve through expression typing"),
        Type::named("int32")
    );
    assert_eq!(
        checker
            .type_of_member_object_expr(&expr(ExprKind::Bool(true)), &mut locals)
            .expect("fallback member objects should resolve through expression typing"),
        Type::named("bool")
    );
    let missing_object = checker
        .type_of_member_object_expr(&expr(ExprKind::Name("missing".to_string())), &mut locals)
        .expect_err("missing member objects should report unknown names");
    assert!(missing_object.message.contains("unknown name `missing`"));
    let moved_object = checker
        .type_of_member_object_expr(&expr(ExprKind::Name("moved".to_string())), &mut locals)
        .expect_err("moved member objects should report moved values");
    assert!(moved_object.message.contains("use of moved value `moved`"));

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
    let primitive_with = checker
        .require_with_resource(&Type::named("int32"), span)
        .expect_err("primitive values cannot be with resources");
    assert!(primitive_with.message.contains("requires a class resource"));
    let unit_with = checker
        .require_with_resource(&Type::Unit, span)
        .expect_err("unit values cannot be with resources");
    assert!(unit_with.message.contains("requires a class resource"));

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

    let consumed_then_borrowed = checker
        .reject_overlapping_borrow(
            &[BorrowedCallPlace {
                path: place_path("counter"),
                passing: ReceiverKind::Value,
                param_name: "owned".to_string(),
            }],
            &place_path("counter"),
            ReceiverKind::Borrow,
            "borrowed",
            "function `use_counter`",
            span,
        )
        .expect_err("borrows should not overlap an already consumed argument");
    assert!(consumed_then_borrowed
        .message
        .contains("overlaps consumed argument"));

    let infer_missing = checker
        .type_check_callable_args(
            "function `make`",
            &["T".to_string()],
            &[],
            &[],
            &Type::TypeParam("T".to_string()),
            ReceiverKind::Value,
            &BTreeMap::new(),
            &[],
            span,
            &mut HashMap::new(),
            None,
            HashMap::new(),
        )
        .expect_err("generic functions without evidence should report missing inference");
    assert!(infer_missing
        .message
        .contains("cannot infer type parameter `T`"));

    let infer_unresolved = checker
        .type_check_callable_args_seeded(
            "function `make`",
            &["T".to_string()],
            &[],
            &[],
            &Type::TypeParam("T".to_string()),
            ReceiverKind::Value,
            &BTreeMap::new(),
            &[],
            span,
            &mut HashMap::new(),
            None,
            HashMap::from([("T".to_string(), Type::TypeParam("T".to_string()))]),
            Vec::new(),
        )
        .expect_err("self-referential inferred type parameters should be rejected");
    assert!(infer_unresolved
        .message
        .contains("cannot infer type parameter `T`"));

    let payload_arity = checker
        .variant_payload_argument(
            &[
                named_arg("value", expr(ExprKind::Int(1))),
                named_arg("extra", expr(ExprKind::Int(2))),
            ],
            span,
            "Some",
            "Option",
        )
        .expect_err("single-payload helper should reject extra named arguments");
    assert!(payload_arity
        .message
        .contains("expects exactly one payload argument"));
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
        is_entry_module: true,
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
            is_entry_module: true,
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
            is_entry_module: true,
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
            is_entry_module: true,
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

    for source in [
        "trait Show:\n    def show(borrow self, text: borrow String) -> int32\n\nclass Box:\n    value: int32\n\nimpl Show for Box:\n    def show(borrow self, text: String) -> int32:\n        return self.value\n",
        "trait Named:\n    def name(borrow self) -> borrow String\n\nclass User:\n    name: String\n\nimpl Named for User:\n    def name(borrow self) -> String:\n        return self.name.clone()\n",
        "trait Choose:\n    def choose(borrow self, left: borrow[left] String, right: borrow[right] String) -> borrow[left] String\n\nclass Picker:\n    value: int32\n\nimpl Choose for Picker:\n    def choose(borrow self, left: borrow[left] String, right: borrow[right] String) -> borrow[right] String:\n        return right\n",
    ] {
        let error = crate::check_source(source)
            .expect_err("trait impl passing and borrow-source mismatches should fail");
        assert!(
            error
                .message
                .contains("does not match the trait signature"),
            "unexpected diagnostic: {}",
            error.message
        );
    }

    crate::check_source(
        "trait Identity:\n    def identity(borrow self, value: borrow[source] String) -> borrow[source] String\n\nclass Picker:\n    value: int32\n\nimpl Identity for Picker:\n    def identity(borrow self, renamed: borrow[origin] String) -> borrow[origin] String:\n        return renamed\n",
    )
    .expect("equivalent borrowed return sources may use different parameter names and labels");

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
fn check_reports_top_level_lowering_errors_from_source() {
    for (source, expected) in [
        (
            "trait Child: Missing:\n    def label() -> String\n\ndef main():\n    pass\n",
            "unknown trait `Missing`",
        ),
        (
            "trait Bad:\n    def value() -> Missing\n\ndef main():\n    pass\n",
            "unknown type `Missing`",
        ),
        (
            "trait Bad:\n    def value() -> borrow[missing] int32\n\ndef main():\n    pass\n",
            "borrow source `missing` must name a borrowed parameter",
        ),
        (
            "trait Bad:\n    def value[T: Missing](value: T) -> T\n\ndef main():\n    pass\n",
            "unknown trait `Missing`",
        ),
        (
            "trait Bad:\n    def value() -> int32:\n        pass\n\ndef main():\n    pass\n",
            "method `value` is missing a return",
        ),
        (
            "enum Bad[T: Missing]:\n    Value(T)\n\ndef main():\n    pass\n",
            "unknown trait `Missing`",
        ),
        (
            "enum Bad:\n    Value(Missing)\n\ndef main():\n    pass\n",
            "unknown type `Missing`",
        ),
        (
            "class Bad[T: Missing]:\n    value: T\n\ndef main():\n    pass\n",
            "unknown trait `Missing`",
        ),
        (
            "class Bad:\n    value: Missing\n\ndef main():\n    pass\n",
            "unknown type `Missing`",
        ),
        (
            "class Bad:\n    def value[T: Missing](self, value: T) -> T:\n        return value\n\ndef main():\n    pass\n",
            "unknown trait `Missing`",
        ),
        (
            "class Bad:\n    def value(self) -> Missing:\n        pass\n\ndef main():\n    pass\n",
            "unknown type `Missing`",
        ),
        (
            "class Bad:\n    def value(self) -> borrow[missing] int32:\n        return 1\n\ndef main():\n    pass\n",
            "borrow source `missing` must name a borrowed parameter",
        ),
        (
            "def value[T: Missing](value: T) -> T:\n    return value\n\ndef main():\n    pass\n",
            "unknown trait `Missing`",
        ),
        (
            "def value() -> Missing:\n    pass\n\ndef main():\n    pass\n",
            "unknown type `Missing`",
        ),
        (
            "trait Show:\n    def render() -> String\n\nclass Box:\n    value: int32\n\nimpl[T: Missing] Show for Box:\n    def render() -> String:\n        return \"x\"\n\ndef main():\n    pass\n",
            "unknown trait `Missing`",
        ),
        (
            "trait Pair[A, B]:\n    def render() -> String\n\nclass Box:\n    value: int32\n\nimpl Pair for Box:\n    def render() -> String:\n        return \"x\"\n\ndef main():\n    pass\n",
            "expects exactly 2 type arguments",
        ),
        (
            "trait Transform:\n    def map[T](value: T) -> T\n\nclass Box:\n    value: int32\n\nimpl Transform for Box:\n    def map[T: Missing](value: T) -> T:\n        return value\n\ndef main():\n    pass\n",
            "unknown trait `Missing`",
        ),
        (
            "trait Show:\n    def render() -> String\n\nclass Box:\n    value: int32\n\nimpl Show for Box:\n    def render() -> Missing:\n        pass\n\ndef main():\n    pass\n",
            "unknown type `Missing`",
        ),
        (
            "trait Ref:\n    def get(self) -> borrow[self] int32\n\nclass Box:\n    value: int32\n\nimpl Ref for Box:\n    def get(self) -> borrow[missing] int32:\n        return self.value\n\ndef main():\n    pass\n",
            "borrow source `missing` must name a borrowed parameter",
        ),
    ] {
        let error = crate::check_source(source).expect_err("invalid program should fail checking");
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

    let supertrait_program = check(
        crate::parser::parse(include_str!("../../../examples/traits/supertraits.au"))
            .expect("supertraits example should parse"),
    )
    .expect("supertraits example should type check");
    let labelled = supertrait_program
        .traits
        .get("Labelled")
        .expect("Labelled trait should lower");
    assert_eq!(labelled.supertraits.len(), 1);
    assert_eq!(labelled.supertraits[0].trait_name, "Named");
    assert!(labelled.methods.contains_key("label"));

    let bounded_program = check(
        crate::parser::parse(include_str!("../../../examples/generics/bounded_types.au"))
            .expect("bounded generics example should parse"),
    )
    .expect("bounded generics example should type check");
    let wrapper = bounded_program
        .classes
        .get("Wrapper")
        .expect("Wrapper class should lower");
    assert_eq!(wrapper.type_param_bounds["T"][0].trait_name, "Named");
    let maybe_named = bounded_program
        .enums
        .get("MaybeNamed")
        .expect("MaybeNamed enum should lower");
    assert_eq!(maybe_named.type_param_bounds["T"][0].trait_name, "Named");

    let default_method_program = crate::check_source(
        "\
trait DefaultMapper[T]:
    def identity(self, value: T) -> T:
        return value

class Box:
    value: int32

impl DefaultMapper[int32] for Box:
    pass

def main():
    pass
",
    )
    .expect("impls should inherit default trait methods with substituted signatures");
    let default_impl = default_method_program
        .trait_impls
        .iter()
        .find(|info| info.trait_name == "DefaultMapper")
        .expect("DefaultMapper impl should exist");
    let identity = default_impl
        .methods
        .get("identity")
        .expect("default method should be inherited by the impl");
    assert_eq!(identity.signature.params, vec![Type::named("int32")]);
    assert_eq!(identity.signature.return_type, Type::named("int32"));

    let default_associated_program = crate::check_source(
        "\
trait Factory:
    def answer() -> int32:
        return 42

def main():
    pass
",
    )
    .expect("default associated trait methods should be checked in trait scope");
    let factory = default_associated_program
        .traits
        .get("Factory")
        .expect("Factory trait should exist");
    let answer = factory
        .methods
        .get("answer")
        .expect("default associated method should exist");
    assert!(answer.signature.params.is_empty());
    assert_eq!(answer.signature.return_type, Type::named("int32"));
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
            is_entry_module: true,
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
