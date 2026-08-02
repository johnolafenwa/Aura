use super::{
    builtin_imported_binding, builtin_module_namespace, host_builtin_metadata, lower_type_ref,
};
use crate::ast::{ExprKind, FunctionTypeParam, ParamMode, ReceiverKind, TypeRef};
use crate::diag::Span;
use crate::sema::{FunctionParamContract, ImportedBinding, Type};

#[test]
fn builtin_type_lowering_preserves_nested_function_signatures() {
    let span = Span::new(1, 1);
    let function = TypeRef::function_with_params(
        vec![
            FunctionTypeParam::new(
                ParamMode::Default,
                TypeRef::named("Duration", Vec::new(), false, span),
                span,
            ),
            FunctionTypeParam::new(
                ParamMode::BorrowMut,
                TypeRef::named("str", Vec::new(), false, span),
                span,
            ),
            FunctionTypeParam::new(
                ParamMode::Own,
                TypeRef::tuple(
                    vec![TypeRef::named("int32", Vec::new(), false, span)],
                    false,
                    span,
                ),
                span,
            ),
        ],
        TypeRef::function(
            vec![TypeRef::named("Duration", Vec::new(), false, span)],
            TypeRef::named("None", Vec::new(), false, span),
            span,
        ),
        span,
    );

    assert_eq!(
        lower_type_ref(&function),
        Type::Function {
            params: vec![
                FunctionParamContract {
                    name: String::new(),
                    ty: Type::named("Duration"),
                    passing: ReceiverKind::Borrow,
                    has_default: false,
                    default_erased: true,
                },
                FunctionParamContract {
                    name: String::new(),
                    ty: Type::named("str"),
                    passing: ReceiverKind::BorrowMut,
                    has_default: false,
                    default_erased: true,
                },
                FunctionParamContract {
                    name: String::new(),
                    ty: Type::Tuple(vec![Type::named("int32")]),
                    passing: ReceiverKind::Value,
                    has_default: false,
                    default_erased: true,
                },
            ],
            return_type: Box::new(Type::Function {
                params: vec![FunctionParamContract {
                    name: String::new(),
                    ty: Type::named("Duration"),
                    passing: ReceiverKind::Borrow,
                    has_default: false,
                    default_erased: true,
                }],
                return_type: Box::new(Type::Unit),
            }),
        }
    );
}

#[test]
fn builtin_imported_binding_reports_unknown_builtin_module_paths() {
    let module_path = ["io".to_string(), "nested".to_string()];
    let error = builtin_imported_binding(&module_path, "println", Span::new(3, 5))
        .expect_err("nested builtin module paths should not resolve");

    assert_eq!(error.span, Some(Span::new(3, 5)));
    assert!(
        error
            .message
            .contains("cannot resolve builtin module `io.nested`"),
        "unexpected diagnostic: {}",
        error.message
    );
}

#[test]
fn control_namespace_exposes_generic_retry_contract_and_defaults() {
    let namespace = builtin_module_namespace(&["control".to_string()])
        .expect("control should be a builtin module");
    let retry = namespace
        .functions
        .get("retry")
        .expect("control.retry should be available");

    assert_eq!(retry.decl.type_params, vec!["T", "E"]);
    assert_eq!(
        retry.signature.params,
        vec![
            Type::Function {
                params: Vec::new(),
                return_type: Box::new(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::TypeParam("T".to_string()),
                        Type::TypeParam("E".to_string())
                    ],
                )),
            },
            Type::named("int32"),
            Type::named("Duration"),
        ]
    );
    assert_eq!(
        retry.signature.param_passings,
        vec![
            ReceiverKind::Borrow,
            ReceiverKind::Borrow,
            ReceiverKind::Borrow
        ]
    );
    assert_eq!(
        retry.signature.return_type,
        Type::Named(
            "Result".to_string(),
            vec![
                Type::TypeParam("T".to_string()),
                Type::TypeParam("E".to_string())
            ],
        )
    );
    assert_eq!(
        retry
            .decl
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        vec!["worker", "max_attempts", "initial_backoff"]
    );
    assert!(retry.decl.params[0].default.is_none());
    assert!(matches!(
        retry.decl.params[1]
            .default
            .as_ref()
            .map(|default| &default.kind),
        Some(ExprKind::Int(3))
    ));
    assert!(matches!(
        retry.decl.params[2]
            .default
            .as_ref()
            .map(|default| &default.kind),
        Some(ExprKind::DurationNanos(0))
    ));
    assert!(
        host_builtin_metadata("control::retry").is_none(),
        "generic retry must not use fixed host-builtin metadata"
    );
}

#[test]
fn builtin_imported_binding_resolves_exports_and_reports_missing_names() {
    let fs_path = ["fs".to_string()];
    let exists = builtin_imported_binding(&fs_path, "exists", Span::new(1, 1))
        .expect("builtin function exports should resolve");
    assert!(matches!(exists, ImportedBinding::Function(_)));

    let missing = builtin_imported_binding(&fs_path, "missing", Span::new(2, 4))
        .expect_err("missing builtin exports should fail");
    assert_eq!(missing.span, Some(Span::new(2, 4)));
    assert!(missing
        .message
        .contains("module `fs` has no export named `missing`"));
}

#[test]
fn host_builtin_metadata_covers_module_functions_and_associated_string_codecs() {
    for name in [
        "sys::args",
        "sys::env",
        "sys::current_dir",
        "sys::unix_time_ms",
        "sys::monotonic_time_ms",
        "path::join",
        "path::parent",
        "path::file_name",
        "path::extension",
        "path::is_absolute",
        "bytes::hex_encode",
        "bytes::hex_decode",
        "bytes::base64_encode",
        "bytes::base64_decode",
        "bytes::sha256",
        "bytes::sha256_string",
        "str.to_bytes",
        "str.from_bytes",
        "json::is_valid",
        "json::stringify_map",
        "json::parse_string_map",
        "toml::is_valid",
        "toml::stringify_map",
        "toml::parse_string_map",
        "metrics::increment",
        "metrics::get",
        "metrics::reset",
        "log::debug",
        "log::info",
        "log::warn",
        "log::error",
        "trace::event",
    ] {
        assert!(
            host_builtin_metadata(name).is_some(),
            "{name} should be derived from its builtin FunctionInfo"
        );
    }

    let join = host_builtin_metadata("path::join").expect("path.join metadata should exist");
    assert_eq!(join.qualified_name, "path::join");
    assert_eq!(
        join.params
            .iter()
            .map(|param| (
                param.name.as_str(),
                &param.ty,
                param.passing,
                param.required
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "base",
                &crate::sema::Type::named("str"),
                ReceiverKind::Borrow,
                true
            ),
            (
                "child",
                &crate::sema::Type::named("str"),
                ReceiverKind::Borrow,
                true
            ),
        ]
    );
    assert_eq!(join.return_type, crate::sema::Type::named("str"));

    let increment =
        host_builtin_metadata("metrics::increment").expect("metrics.increment metadata");
    // Both are shared: ADR-0022 Q1 removed the copy-parameter snapshot, so a
    // copy-typed builtin argument is declared exactly like a non-copy one.
    assert_eq!(increment.params[0].passing, ReceiverKind::Borrow);
    assert_eq!(increment.params[1].passing, ReceiverKind::Borrow);
    assert_eq!(increment.return_type, crate::sema::Type::Unit);

    let secure_int =
        host_builtin_metadata("random::secure_int").expect("random.secure_int metadata");
    assert_eq!(secure_int.params.len(), 2);
    assert!(secure_int
        .params
        .iter()
        .all(|param| param.ty == crate::sema::Type::named("int64") && param.required));
    assert_eq!(secure_int.return_type, crate::sema::Type::named("int64"));

    let secure_bytes =
        host_builtin_metadata("random::secure_bytes").expect("random.secure_bytes metadata");
    assert_eq!(secure_bytes.params.len(), 1);
    assert_eq!(secure_bytes.params[0].ty, crate::sema::Type::named("int64"));
    assert_eq!(
        secure_bytes.return_type,
        crate::sema::Type::Named("list".to_string(), vec![crate::sema::Type::named("uint8")])
    );

    assert!(host_builtin_metadata("fs::exists").is_none());
    assert!(host_builtin_metadata("missing::function").is_none());
}

#[test]
fn math_namespace_exposes_the_exact_float64_function_contract() {
    let namespace =
        builtin_module_namespace(&["math".to_string()]).expect("math should be a builtin module");
    let expected = [
        ("ceil", vec!["value"], Type::named("int64")),
        ("cos", vec!["value"], Type::named("float64")),
        ("exp", vec!["value"], Type::named("float64")),
        ("floor", vec!["value"], Type::named("int64")),
        ("log", vec!["value"], Type::named("float64")),
        ("log10", vec!["value"], Type::named("float64")),
        ("log2", vec!["value"], Type::named("float64")),
        ("pow", vec!["base", "exponent"], Type::named("float64")),
        ("sin", vec!["value"], Type::named("float64")),
        ("tan", vec!["value"], Type::named("float64")),
        ("trunc", vec!["value"], Type::named("int64")),
    ];

    assert_eq!(namespace.functions.len(), expected.len());
    for (name, parameter_names, return_type) in expected {
        let function = namespace
            .functions
            .get(name)
            .unwrap_or_else(|| panic!("math.{name} should be available"));
        assert_eq!(
            function
                .decl
                .params
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            parameter_names
        );
        assert!(function
            .signature
            .params
            .iter()
            .all(|parameter| *parameter == Type::named("float64")));
        assert_eq!(function.signature.return_type, return_type);
        assert!(host_builtin_metadata(&format!("math::{name}")).is_some());
    }
}

#[test]
fn math_namespace_exposes_exact_generic_float64_constants() {
    let namespace =
        builtin_module_namespace(&["math".to_string()]).expect("math should be a builtin module");
    let expected = [
        ("e", 0x4005_bf0a_8b14_5769_u64),
        ("inf", 0x7ff0_0000_0000_0000_u64),
        ("nan", 0x7ff8_0000_0000_0000_u64),
        ("pi", 0x4009_21fb_5444_2d18_u64),
    ];

    assert_eq!(namespace.constants.len(), expected.len());
    assert_eq!(namespace.all_constants.len(), expected.len());
    for (name, bits) in expected {
        let constant = namespace
            .constants
            .get(name)
            .unwrap_or_else(|| panic!("math.{name} should be available"));
        assert_eq!(constant.module_name, "math");
        assert!(constant.decl.public);
        assert_eq!(constant.decl.name, name);
        assert_eq!(constant.ty, Type::named("float64"));
        let crate::ast::ExprKind::Float(value) = constant.decl.value.kind else {
            panic!("math.{name} should use the generic float-literal constant representation");
        };
        assert_eq!(value.to_bits(), bits, "math.{name} bits");
        assert_eq!(
            namespace.all_constants[name].decl.value.span,
            constant.decl.value.span
        );
    }

    assert!(matches!(
        builtin_imported_binding(&["math".to_string()], "pi", crate::diag::Span::new(7, 3))
            .expect("math.pi should be directly importable"),
        crate::sema::ImportedBinding::Constant(_)
    ));
}

#[test]
fn bytes_namespace_exposes_shared_byte_vector_codecs_and_typed_errors() {
    use crate::sema::Type;

    let namespace =
        builtin_module_namespace(&["bytes".to_string()]).expect("bytes should be builtin");
    assert_eq!(
        namespace
            .functions
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "base64_decode",
            "base64_encode",
            "hex_decode",
            "hex_encode",
            "sha256",
            "sha256_string",
        ]
    );
    for function in namespace.functions.values() {
        assert_eq!(function.decl.params[0].mode, ParamMode::Default);
        assert_eq!(
            function.signature.param_passings,
            vec![ReceiverKind::Borrow],
            "{} should retain its input",
            function.decl.name
        );
    }

    let error = namespace.enums.get("Error").expect("bytes.Error enum");
    assert_eq!(
        error
            .decl
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "InvalidUtf8",
            "InvalidHexLength",
            "InvalidHexDigit",
            "InvalidBase64",
        ]
    );
    assert_eq!(
        error.variants["InvalidHexDigit"]
            .payloads
            .iter()
            .map(|payload| (
                payload.name.as_deref().expect("named bytes.Error payload"),
                payload.ty.clone(),
            ))
            .collect::<Vec<_>>(),
        vec![
            ("index", Type::named("int32")),
            ("byte", Type::named("uint8")),
        ]
    );

    let from_bytes = host_builtin_metadata("str.from_bytes").expect("associated metadata");
    assert_eq!(from_bytes.params[0].passing, ReceiverKind::Borrow);
    assert_eq!(
        from_bytes.return_type,
        Type::Named(
            "Result".to_string(),
            vec![Type::named("str"), Type::named("bytes.Error")],
        )
    );
}

#[test]
fn random_namespace_exposes_one_opaque_rng_type_and_secure_functions() {
    let namespace = builtin_module_namespace(&["random".to_string()])
        .expect("random should be a builtin module");

    assert_eq!(namespace.classes.len(), 1);
    let rng = namespace.classes.get("Rng").expect("random.Rng class");
    assert!(!rng.decl.copy, "Rng state must be non-copy");
    assert!(rng.decl.fields.is_empty(), "Rng must remain host-opaque");
    assert!(
        rng.fields.is_empty(),
        "Rng must not expose a fake seed field"
    );
    assert!(
        !namespace.functions.contains_key("Rng"),
        "Rng must have one class binding, not a duplicate function binding"
    );

    let secure_int = &namespace.functions["secure_int"];
    assert_eq!(
        secure_int.signature.params,
        vec![
            crate::sema::Type::named("int64"),
            crate::sema::Type::named("int64")
        ]
    );
    assert_eq!(
        secure_int.signature.return_type,
        crate::sema::Type::named("int64")
    );

    let secure_bytes = &namespace.functions["secure_bytes"];
    assert_eq!(
        secure_bytes.signature.params,
        vec![crate::sema::Type::named("int64")]
    );
    assert_eq!(
        secure_bytes.signature.return_type,
        crate::sema::Type::Named("list".to_string(), vec![crate::sema::Type::named("uint8")])
    );
    assert!(!namespace.functions.contains_key("secure_float"));

    assert!(matches!(
        builtin_imported_binding(&["random".to_string()], "Rng", Span::new(1, 1))
            .expect("Rng should import"),
        ImportedBinding::Class(_)
    ));
}

#[test]
fn json_namespace_exposes_dynamic_tree_contract() {
    use crate::sema::Type;

    let namespace =
        builtin_module_namespace(&["json".to_string()]).expect("json should be a builtin module");

    let value = namespace.enums.get("Value").expect("json.Value enum");
    assert_eq!(
        value
            .decl
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Null", "Bool", "Int", "Float", "String", "Array", "Object"]
    );
    let value_payloads = [
        ("Null", Vec::new()),
        ("Bool", vec![Type::named("bool")]),
        ("Int", vec![Type::named("int64")]),
        ("Float", vec![Type::named("float64")]),
        ("String", vec![Type::named("str")]),
        (
            "Array",
            vec![Type::Named(
                "list".to_string(),
                vec![Type::named("json.Value")],
            )],
        ),
        (
            "Object",
            vec![Type::Named(
                "dict".to_string(),
                vec![Type::named("str"), Type::named("json.Value")],
            )],
        ),
    ];
    for (variant, expected) in value_payloads {
        let actual = value.variants[variant]
            .payloads
            .iter()
            .map(|payload| payload.ty.clone())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "json.Value.{variant} payloads");
        assert!(
            value.variants[variant]
                .payloads
                .iter()
                .all(|payload| payload.name.is_none()),
            "json.Value tuple variants must use positional payloads"
        );
    }

    let error = namespace.enums.get("Error").expect("json.Error enum");
    assert_eq!(
        error
            .decl
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Syntax",
            "NumberOutOfRange",
            "NestingTooDeep",
            "InputTooLarge"
        ]
    );
    let error_payloads = [
        (
            "Syntax",
            vec![
                ("message", Type::named("str")),
                ("line", Type::named("int32")),
                ("column", Type::named("int32")),
            ],
        ),
        (
            "NumberOutOfRange",
            vec![
                ("line", Type::named("int32")),
                ("column", Type::named("int32")),
            ],
        ),
        (
            "NestingTooDeep",
            vec![
                ("limit", Type::named("int32")),
                ("line", Type::named("int32")),
                ("column", Type::named("int32")),
            ],
        ),
        (
            "InputTooLarge",
            vec![
                ("actual_bytes", Type::named("int64")),
                ("limit_bytes", Type::named("int64")),
            ],
        ),
    ];
    for (variant, expected) in error_payloads {
        let actual = error.variants[variant]
            .payloads
            .iter()
            .map(|payload| {
                (
                    payload.name.as_deref().expect("named json.Error payload"),
                    payload.ty.clone(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "json.Error.{variant} payloads");
        assert!(error.variants[variant].named_payloads);
    }

    let parse = &namespace.functions["parse"];
    assert_eq!(parse.signature.params, vec![Type::named("str")]);
    assert_eq!(parse.decl.params[0].mode, ParamMode::Default);
    assert_eq!(parse.signature.param_passings, vec![ReceiverKind::Borrow]);
    assert_eq!(
        parse.signature.return_type,
        Type::Named(
            "Result".to_string(),
            vec![Type::named("json.Value"), Type::named("json.Error")]
        )
    );

    let dumps = &namespace.functions["dumps"];
    assert_eq!(
        dumps.signature.params,
        vec![
            Type::named("json.Value"),
            Type::Named("Option".to_string(), vec![Type::named("int64")])
        ]
    );
    // ADR-0022 Q1: the copy-typed `indent` is shared like every other bare
    // parameter. The ABI still copies its bits.
    assert_eq!(
        dumps.signature.param_passings,
        vec![ReceiverKind::Borrow, ReceiverKind::Borrow]
    );
    assert_eq!(dumps.decl.params[0].mode, ParamMode::Default);
    assert!(matches!(
        dumps.decl.params[1]
            .default
            .as_ref()
            .map(|default| &default.kind),
        Some(ExprKind::Name(name)) if name == "None"
    ));
    assert_eq!(dumps.signature.return_type, Type::named("str"));
    assert!(
        !host_builtin_metadata("json::dumps")
            .expect("json.dumps host metadata")
            .params[1]
            .required
    );

    for (name, return_type) in [
        ("is_null", Type::named("bool")),
        (
            "as_bool",
            Type::Named("Option".to_string(), vec![Type::named("bool")]),
        ),
        (
            "as_int",
            Type::Named("Option".to_string(), vec![Type::named("int64")]),
        ),
        (
            "as_float",
            Type::Named("Option".to_string(), vec![Type::named("float64")]),
        ),
    ] {
        let function = &namespace.functions[name];
        assert_eq!(function.decl.params[0].mode, ParamMode::Default, "{name}");
        assert_eq!(
            function.signature.param_passings,
            vec![ReceiverKind::Borrow],
            "{name}"
        );
        assert_eq!(function.signature.return_type, return_type, "{name}");
    }

    for (name, inner_type) in [
        ("into_string", Type::named("str")),
        (
            "into_array",
            Type::Named("list".to_string(), vec![Type::named("json.Value")]),
        ),
        (
            "into_object",
            Type::Named(
                "dict".to_string(),
                vec![Type::named("str"), Type::named("json.Value")],
            ),
        ),
    ] {
        let function = &namespace.functions[name];
        assert_eq!(function.decl.params[0].mode, ParamMode::Own, "{name}");
        assert_eq!(
            function.signature.param_passings,
            vec![ReceiverKind::Value],
            "{name}"
        );
        assert_eq!(
            function.signature.return_type,
            Type::Named("Option".to_string(), vec![inner_type]),
            "{name}"
        );
    }

    for legacy in ["is_valid", "stringify_map", "parse_string_map"] {
        assert!(
            namespace.functions.contains_key(legacy),
            "legacy json helper {legacy} must remain available"
        );
    }
}
