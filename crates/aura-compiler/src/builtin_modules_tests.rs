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
                    keyword_only: false,
                    name: String::new(),
                    ty: Type::named("Duration"),
                    passing: ReceiverKind::Borrow,
                    has_default: false,
                },
                FunctionParamContract {
                    keyword_only: false,
                    name: String::new(),
                    ty: Type::named("str"),
                    passing: ReceiverKind::BorrowMut,
                    has_default: false,
                },
                FunctionParamContract {
                    keyword_only: false,
                    name: String::new(),
                    ty: Type::Tuple(vec![Type::named("int32")]),
                    passing: ReceiverKind::Value,
                    has_default: false,
                },
            ],
            return_type: Box::new(Type::Function {
                params: vec![FunctionParamContract {
                    keyword_only: false,
                    name: String::new(),
                    ty: Type::named("Duration"),
                    passing: ReceiverKind::Borrow,
                    has_default: false,
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

    let bytes_path = ["bytes".to_string()];
    let error = builtin_imported_binding(&bytes_path, "Error", Span::new(1, 8))
        .expect("builtin enum exports should resolve");
    let ImportedBinding::Enum(error) = error else {
        panic!("bytes.Error should resolve as an enum export");
    };
    assert_eq!(error.decl.name, "Error");
    assert_eq!(
        error
            .variants
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "InvalidBase64",
            "InvalidHexDigit",
            "InvalidHexLength",
            "InvalidUtf8"
        ]
    );

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
        "bytes::hmac_sha256",
        "bytes::constant_time_eq",
        "gzip::compress",
        "gzip::decompress",
        "regex::__aura_compile",
        "regex::__aura_pattern",
        "regex::__aura_escape",
        "regex::__aura_is_match",
        "regex::__aura_count",
        "regex::__aura_find",
        "regex::__aura_matches",
        "regex::__aura_find_all",
        "regex::__aura_captures",
        "regex::__aura_group_index",
        "regex::__aura_split",
        "regex::__aura_replace",
        "regex::__aura_replace_all",
        "time::__aura_now",
        "time::__aura_monotonic",
        "time::__aura_local_offset_minutes",
        "time::__aura_duration_seconds",
        "time::__aura_duration_nanos",
        "time::__aura_duration",
        "time::__aura_rfc3339",
        "time::__aura_format",
        "time::__aura_parse_rfc3339",
        "time::__aura_parse",
        "time::__aura_failed",
        "time::__aura_error",
        "time::__aura_parsed_seconds",
        "time::__aura_parsed_nanos",
        "time::__aura_parsed_offset",
        "http::__aura_send",
        "http::__aura_read_chunk",
        "http::__aura_read_line",
        "http::__aura_read_lines",
        "http::__aura_read_event",
        "http::__aura_read_all",
        "http::__aura_read_text",
        "http::__aura_close",
        "http::__aura_idle_connections",
        "http::__aura_close_idle",
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
            "constant_time_eq",
            "ends_with",
            "find",
            "hex_decode",
            "hex_encode",
            "hmac_sha256",
            "sha256",
            "sha256_string",
            "starts_with",
        ]
    );
    for function in namespace.functions.values() {
        assert!(function
            .decl
            .params
            .iter()
            .all(|param| param.mode == ParamMode::Default));
        assert!(
            function
                .signature
                .param_passings
                .iter()
                .all(|passing| *passing == ReceiverKind::Borrow),
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

    let find = host_builtin_metadata("bytes::find").expect("byte search metadata");
    assert_eq!(
        find.params
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        vec!["value", "needle"]
    );
    assert_eq!(
        find.return_type,
        crate::sema::optional_type(Type::named("int64"))
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
            crate::sema::optional_type(Type::named("int64"))
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
        ("as_bool", crate::sema::optional_type(Type::named("bool"))),
        ("as_int", crate::sema::optional_type(Type::named("int64"))),
        (
            "as_float",
            crate::sema::optional_type(Type::named("float64")),
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
            crate::sema::optional_type(inner_type),
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

#[test]
fn union_annotation_helpers_preserve_normalized_members() {
    let ty = crate::ast::TypeRef::union(
        vec![
            crate::ast::TypeRef::named("None", vec![], false, crate::diag::Span::new(1, 1)),
            crate::ast::TypeRef::named("int", vec![], false, crate::diag::Span::new(1, 1)),
            crate::ast::TypeRef::named("int64", vec![], false, crate::diag::Span::new(1, 1)),
        ],
        crate::diag::Span::new(1, 1),
    );
    assert_eq!(super::lower_type_ref(&ty).to_string(), "int64 | None");
}

#[test]
fn builtin_callable_type_refs_lower_to_callable_storage_types() {
    let span = Span::new(1, 1);
    let signature = TypeRef::function(
        vec![TypeRef::named("int64", Vec::new(), false, span)],
        TypeRef::named("str", Vec::new(), false, span),
        span,
    );
    let lowered = lower_type_ref(&TypeRef::callable(
        true,
        ReceiverKind::BorrowMut,
        signature,
        span,
    ));
    let Type::Callable(callable) = lowered else {
        panic!("expected a callable, found {lowered}");
    };
    assert!(callable.task);
    assert_eq!(
        callable.call_kind,
        crate::sema::closure_call_kind_for(ReceiverKind::BorrowMut)
    );
    assert_eq!(callable.params.len(), 1);
    assert_eq!(callable.params[0].ty, Type::named("int64"));
    assert_eq!(callable.return_type, Type::named("str"));
    assert_eq!(
        lower_type_ref(&TypeRef::callable(
            false,
            ReceiverKind::Borrow,
            TypeRef::named("int64", Vec::new(), false, span),
            span,
        )),
        Type::named("Unknown")
    );
}

#[test]
fn runtime_dynamic_host_abi_matches_builtin_signatures() {
    let table = crate::native_runtime::DYNAMIC_HOST_ABI;
    assert!(!table.is_empty());
    for abi in table {
        let metadata = host_builtin_metadata(abi.name)
            .unwrap_or_else(|| panic!("`{}` has no builtin signature", abi.name));
        let expected = metadata
            .params
            .iter()
            .map(|param| (param.name.as_str(), param.passing))
            .collect::<Vec<_>>();
        let actual = abi
            .params
            .iter()
            .map(|param| (param.name, param.passing))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "runtime ABI for `{}`", abi.name);
    }
    let names = table
        .iter()
        .map(|abi| abi.name)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(names.len(), table.len(), "runtime ABI names must be unique");
    assert_eq!(
        crate::native_runtime::dynamic_host_abi("bytes::find").map(|abi| abi.params.len()),
        Some(2)
    );
    assert_eq!(
        crate::native_runtime::dynamic_host_abi("toml::is_valid"),
        None
    );
    assert_eq!(
        ReceiverKind::Value,
        crate::native_runtime::dynamic_host_abi("json::into_object")
            .unwrap()
            .params[0]
            .passing
    );
}

#[test]
fn every_host_builtin_has_its_own_native_entry_symbol() {
    let metadata = super::build_host_builtin_metadata();
    let table = crate::native_runtime::HOST_BUILTIN_SYMBOLS;
    let table_names = table
        .iter()
        .map(|(name, _)| *name)
        .collect::<std::collections::BTreeSet<_>>();
    // Secure randomness lowers to its own scalar runtime ABI instead.
    let metadata_names = metadata
        .keys()
        .map(String::as_str)
        .filter(|name| !matches!(*name, "random::secure_int" | "random::secure_bytes"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(table_names, metadata_names);
    let symbols = table
        .iter()
        .map(|(_, symbol)| *symbol)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        symbols.len(),
        table.len(),
        "native entry symbols must be unique"
    );
    for (name, symbol) in table {
        assert!(
            symbol.starts_with("aura_host_"),
            "`{name}` symbol `{symbol}`"
        );
        assert_eq!(
            crate::native_runtime::host_builtin_symbol(name),
            Some(*symbol)
        );
    }
    assert_eq!(
        crate::native_runtime::host_builtin_symbol("toml::is_valid"),
        Some("aura_host_toml_is_valid")
    );
    assert_eq!(
        crate::native_runtime::host_builtin_symbol("io::print"),
        None
    );
}

#[test]
fn builtin_root_module_names_match_their_namespaces() {
    for name in super::builtin_module_registry().keys() {
        assert!(
            super::is_builtin_root_module(name),
            "{name} is a root module"
        );
    }
    for name in ["main", "app", "list", "io.extra", ""] {
        assert!(
            !super::is_builtin_root_module(name),
            "{name} is not a root module"
        );
        assert!(super::builtin_module_namespace(&[name.to_string()]).is_none());
    }
}

#[test]
fn gzip_namespace_declares_bounded_decompression_and_a_typed_error() {
    use crate::sema::Type;

    let namespace =
        builtin_module_namespace(&["gzip".to_string()]).expect("gzip should be builtin");
    assert!(super::is_builtin_root_module("gzip"));
    assert!(super::builtin_module_registry().contains_key("gzip"));
    assert_eq!(
        namespace
            .functions
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["compress", "decompress"]
    );
    let bytes = Type::Named("list".to_string(), vec![Type::named("uint8")]);
    let compress = &namespace.functions["compress"];
    assert_eq!(compress.signature.return_type, bytes);
    assert_eq!(
        compress
            .decl
            .params
            .iter()
            .map(|param| (param.name.as_str(), param.default.is_some()))
            .collect::<Vec<_>>(),
        vec![("data", false), ("level", true)]
    );
    let decompress = &namespace.functions["decompress"];
    assert_eq!(
        decompress.signature.return_type,
        Type::Named("Result".to_string(), vec![bytes, Type::named("gzip.Error")])
    );
    assert_eq!(
        decompress
            .decl
            .params
            .iter()
            .map(|param| (param.name.as_str(), param.default.is_some()))
            .collect::<Vec<_>>(),
        vec![("data", false), ("max_bytes", true)]
    );
    let error = &namespace.enums["Error"];
    assert_eq!(
        error
            .decl
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec!["InvalidData", "OutputTooLarge"]
    );

    let checked = crate::check_source(
        r#"import gzip

def unpack(data: list[uint8]) -> str:
    match own gzip.decompress(gzip.compress(data, level=1), max_bytes=64):
        case Result.Ok(plain):
            return f"{plain.len()}"
        case Result.Err(gzip.Error.InvalidData(message)):
            return message
        case Result.Err(gzip.Error.OutputTooLarge(limit_bytes)):
            return f"{limit_bytes}"
"#,
    );
    checked.expect("gzip calls with defaults and named arguments should type-check");
    let error = crate::check_source(
        "import gzip\n\ndef pack(text: str) -> list[uint8]:\n    return gzip.compress(text)\n",
    )
    .expect_err("gzip.compress takes bytes, not text");
    assert!(error.message.contains("list[uint8]"), "{}", error.message);
}

#[test]
fn regex_module_type_checks_its_whole_api_and_keeps_regex_construction_private() {
    assert!(super::is_builtin_root_module("regex"));
    assert!(super::builtin_module_registry().contains_key("regex"));
    let find = host_builtin_metadata("regex::__aura_find").expect("regex find metadata");
    assert_eq!(find.params.len(), 2);

    crate::check_source(
        r#"import regex

def scan(pattern: regex.Regex, line: str) -> int64:
    mut total = pattern.count(line)
    if pattern.is_match(line):
        total += 1
    match pattern.find(line):
        case regex.Match as found:
            total += found.end - found.start + found.text.len()
        case None:
            pass
    for found in pattern.matches(line):
        total += found.start
    match pattern.captures(line):
        case regex.Captures as groups:
            total += groups.len()
            match groups.get(1):
                case str as text:
                    total += text.len()
                case None:
                    pass
            match groups.name("key"):
                case str as text:
                    total += text.len()
                case None:
                    pass
        case None:
            pass
    total += pattern.find_all(line).len() + pattern.split(line).len()
    total += pattern.replace(line, "$1").len() + pattern.replace_all(line, "").len()
    other = pattern
    total += pattern.count(line)
    return total + other.pattern().len() + regex.escape(line).len()

def build(text: str) -> Result[regex.Regex, regex.Error]:
    return regex.compile(text)

def reason(error: regex.Error) -> str:
    match error:
        case regex.Error.Invalid(message):
            return message.clone()
"#,
    )
    .expect("the regex API should type-check");

    let error = crate::check_source(
        "import regex\n\ndef forge() -> regex.Regex:\n    return regex.Regex(number=0)\n",
    )
    .expect_err("only regex.compile builds a Regex");
    assert!(error.message.contains("number"), "{}", error.message);
    let error = crate::check_source(
        "import regex\n\ndef bad(pattern: regex.Regex) -> bool:\n    return pattern.is_match(3)\n",
    )
    .expect_err("a search takes text");
    assert!(error.message.contains("str"), "{}", error.message);
}

#[test]
fn time_module_type_checks_its_whole_api_and_keeps_date_time_fields_private() {
    assert!(super::is_builtin_root_module("time"));
    assert!(super::builtin_module_registry().contains_key("time"));
    let format = host_builtin_metadata("time::__aura_format").expect("time format metadata");
    assert_eq!(format.params.len(), 4);

    crate::check_source(
        r#"import time

def describe(moment: time.DateTime) -> Result[str, time.Error]:
    (year, month, day) = moment.date()
    total = (
        year + month + day + moment.year() + moment.month() + moment.day()
        + moment.hour() + moment.minute() + moment.second() + moment.nanosecond()
        + moment.weekday() + moment.day_of_year() + moment.offset_minutes()
        + moment.unix() + moment.unix_ms()
    )
    shifted = try moment.to_offset(60)
    later = shifted.to_utc().to_local().add(Duration.seconds(5)).sub(Duration.ms(1))
    elapsed: Duration = later.since(moment)
    copy = later
    if later.is_before(moment) or copy.is_after(moment) or later.same_instant(moment):
        return Result.Ok(f"{total} {elapsed.to_ms()} {later}")
    text = try later.format("%Y-%m-%d")
    return Result.Ok(text + later.to_rfc3339())

def build(text: str) -> Result[time.DateTime, time.Error]:
    first = try time.utc(2026, 1, 2)
    second = try time.utc(2026, 1, 2, 3, 4, 5, 6)
    third = try time.from_unix(0)
    fourth = try time.from_unix(0, 5)
    fifth = try time.from_unix_ms(1)
    sixth = try time.parse(text, "%Y")
    seventh = try time.parse_rfc3339(text)
    waited: Duration = time.monotonic() - time.monotonic()
    if first.is_before(second) and third.is_before(fourth) and fifth.is_before(sixth):
        return Result.Ok(time.now())
    if waited.to_ms() > 0.0:
        return Result.Ok(time.now_local())
    return Result.Ok(seventh)

def reason(error: time.Error) -> str:
    match error:
        case time.Error.Invalid(message):
            return message.clone()
"#,
    )
    .expect("the time API should type-check");

    let error = crate::check_source(
        "import time\n\ndef forge() -> time.DateTime:\n    return time.DateTime(unix_seconds=0, subsecond_nanos=0, utc_offset_minutes=0)\n",
    )
    .expect_err("only the time functions build a DateTime");
    assert!(error.message.contains("unix_seconds"), "{}", error.message);
    let error = crate::check_source(
        "import time\n\ndef bad(moment: time.DateTime) -> time.DateTime:\n    return moment.add(5)\n",
    )
    .expect_err("arithmetic takes a Duration");
    assert!(error.message.contains("Duration"), "{}", error.message);
}

#[test]
fn http_module_type_checks_its_api_and_keeps_the_stream_private() {
    assert!(super::is_builtin_root_module("http"));
    assert!(super::builtin_module_registry().contains_key("http"));
    let send = host_builtin_metadata("http::__aura_send").expect("http send metadata");
    assert_eq!(send.params.len(), 10);

    crate::check_source(
        r#"import http
import io

def settings() -> http.Client:
    return http.Client(timeout=2s, read_timeout=None, max_redirects=3, decompress=false, keep_alive=false, ca_file="ca.pem")

def first_line(url: str) -> Result[str | None, io.Error]:
    with response = try settings().get(url):
        if not response.ok():
            return Result.Ok(None)
        return response.read_line()

def retry_delay(event: http.Event) -> Duration | None:
    return event.retry
"#,
    )
    .expect("the http API should type-check");

    let error = crate::check_source(
        "import http\n\ndef peek(response: http.Response) -> int64:\n    return response.fields.len()\n",
    )
    .expect_err("the header list is private");
    assert!(error.message.contains("fields"), "{}", error.message);
    let error = crate::check_source(
        "import http\n\ndef read(response: http.Response) -> None:\n    _ = response.read_line()\n",
    )
    .expect_err("reading a body needs mutable access");
    assert!(error.message.contains("mut"), "{}", error.message);
    let error = crate::check_source(
        "import net\n\ndef old() -> None:\n    _ = net.http_request_text(\"GET\", \"http://h/\", \"\", {})\n",
    )
    .expect_err("the old request functions are gone");
    assert!(
        error.message.contains("http_request_text"),
        "{}",
        error.message
    );
}

#[test]
fn http_module_runs_requests_streams_redirects_and_events_on_the_interpreter() {
    let output = crate::run_source(HTTP_PROGRAM).expect("the http program should run");
    assert_eq!(
        output.stdout,
        "200 OK true true\na, b\nNone\n[a, b]\na, b\nalpha\n[98, 101]\n[ta]\ngamma\nNone\n1\n\
         200 true\nalpha\nbeta\r\ngamma\n302 /text\n0\ndelta|one\ntwo|7|None\nmessage|three|7|250ms\n\
         404 false\nnone\n17\n17\n17\n1\n0\n"
    );
}

/// A loopback server written on `net.listen` and a client that uses every
/// public member of the `http` module against it.
const HTTP_PROGRAM: &str = r#"import http
import io
import net

def chunk(piece: str) -> str:
    digits = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f"]
    size = piece.byte_len()
    return digits[size // 16] + digits[size % 16] + "\r\n" + piece + "\r\n"

def reply(target: str) -> str:
    if target == "/moved":
        return "HTTP/1.1 302 Found\r\nLocation: /text\r\nContent-Length: 0\r\n\r\n"
    if target == "/events":
        head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n"
        first = chunk(": hello\n\nevent: delta\nid: 7\ndata: one\ndata: two\n\n")
        second = chunk("retry: 250\ndata: three\n\n")
        return head + first + second + "0\r\n\r\n"
    if target == "/missing":
        return "HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\n\r\nnone"
    body = "alpha\nbeta\r\ngamma"
    return f"HTTP/1.1 200 OK\r\nX-Tag: a\r\nx-tag: b\r\nContent-Length: {body.byte_len()}\r\n\r\n" + body

def serve(addresses: Queue[str], requests: int64) -> Result[int64, io.Error]:
    mut accepted = 0
    mut served = 0
    with listener = try net.listen("127.0.0.1:0"):
        _ = addresses.put(try listener.local_addr())
        while served < requests:
            with conn = try listener.accept():
                accepted += 1
                while served < requests:
                    mut target = ""
                    match own try conn.read_line():
                        case str as start:
                            target = start.split(" ")[1].clone()
                        case None:
                            break
                    while true:
                        match own try conn.read_line():
                            case str as header:
                                if header == "" or header == "\r":
                                    break
                            case None:
                                break
                    try conn.write_all(reply(target))
                    served += 1
    return Result.Ok(accepted)

def run() -> Result[None, io.Error]:
    with TaskGroup() as group:
        addresses = Queue[str](capacity=1)
        server = group.start(serve, addresses, 9)
        mut base = ""
        match own addresses.get(timeout=5s):
            case QueueReceive.Item(address):
                base = "http://" + address
            case _:
                return Result.Err(io.Error.TimedOut)

        mut text = try http.get(base + "/text")
        print(f"{text.status} {text.reason} {text.ok()} {text.url == base + "/text"}")
        print(text.header("X-TAG"))
        print(text.header("absent"))
        print(text.header_values("x-tag"))
        print(text.headers()["x-tag"])
        print(try text.read_line())
        print(try text.read_chunk(2))
        print(try text.read_lines(8))
        print(try text.read_line())
        print(try text.read_chunk(2))
        text.close()
        print(http.idle_connections())

        with moved = try http.get(base + "/moved"):
            print(f"{moved.status} {moved.url == base + "/text"}")
            print(try moved.text_up_to(64))

        client = http.Client(timeout=5s, read_timeout=5s, max_redirects=0, decompress=false)
        with held = try client.get(base + "/moved"):
            print(f"{held.status} {held.header("location")}")
            print((try held.bytes_up_to(8)).len())

        with events = try http.request("GET", base + "/events", "", {"Accept": "text/event-stream"}):
            while true:
                event = try events.read_event()
                if event is None:
                    break
                print(f"{event.event}|{event.data}|{event.id}|{event.retry}")

        with missing = try client.post(base + "/missing", "body", {}):
            print(f"{missing.status} {missing.ok()}")
            print(try missing.text())

        with raw = try client.request_bytes("PUT", base + "/text", [1 as uint8], {}):
            print((try raw.bytes()).len())
        with posted = try http.post(base + "/text", "x", {}):
            print((try posted.text_up_to(64)).len())
        with again = try http.request_bytes("PUT", base + "/text", [1 as uint8], {}):
            print((try again.text()).len())

        match own server.result():
            case TaskResult.Ready(result):
                print(try result)
            case _:
                print("server failed")
        http.close_idle_connections()
        print(http.idle_connections())
    return Result.Ok(None)

def main() -> int32:
    match run():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#;

#[test]
fn tensor_module_runs_its_whole_api_on_the_cpu_device() {
    assert!(super::is_builtin_root_module("tensor"));
    assert!(super::builtin_module_registry().contains_key("tensor"));
    let binary = host_builtin_metadata("tensor::__aura_binary").expect("tensor binary metadata");
    assert_eq!(binary.params.len(), 3);

    let output = crate::run_source(
        r#"from tensor import Device, Tensor, where

def main():
    device = Device.cpu()
    print(f"{device.name()} {device.is_gpu()} {Device.default().name() != ""}")
    a = Array[float32].from_list([1.0, 2.0, 3.0, 4.0, 5.0, 6.0], [2, 3])
    x = Tensor[float32].from_array(a, device)
    y = Tensor[float32].full([2, 3], 2.0, device)
    z = x * y + x - y / y
    print(z.to_array())
    print(f"{z.shape()} {z.len()} {z.device().name()}")
    print(f"{z.sum().item()} {z.min().item()} {z.max().item()} {z.mean().item()}")
    print(z.sum(dim=0).to_array())
    print(x.matmul(y.transpose()).to_array())
    print(x.minimum(y).maximum(-x).to_array())
    print(x.pow(y).sqrt().abs().to_array())
    round_trip = (x.exp().log() - x).abs().max().item()
    unit = (x.sin() * x.sin() + x.cos() * x.cos()).sum().item()
    print(f"{round_trip < 0.0001} {unit > 5.9999 and unit < 6.0001}")
    mask = x.greater(x.scalar(3.0))
    print(mask.to_array())
    print(where(mask, x, -x).to_array())
    print(f"{x.less(y).to_array()} {x.less_equal(y).to_array()}")
    print(f"{x.greater_equal(y).to_array()} {x.equal(y).to_array()} {x.not_equal(y).to_array()}")
    print(x.reshape([3, 2]).flatten().permute([0]).to_array())
    i = Tensor[int64].full([4], 7, device)
    print((i * i.scalar(3) - i).to_array())
    print(f"{i.to_float32().to_array()} {i.to_int32().to_array()} {i.to_float64().to_array()}")
    print(x.to_int64().to_array())
    zero = Tensor[int32].zeros([2], device)
    print(f"{zero.to_array()} {Tensor[bool].zeros([2], device).to_array()}")
    print(Tensor[float64].full([1], 0.5, device).item())
    w = z.clone().to(device)
    w.eval()
    print(w.max(dim=1).to_array())
    match Device.metal():
        case Device as gpu:
            print(gpu.is_gpu())
        case None:
            print(true)
"#,
    )
    .expect("the tensor API should run");
    assert_eq!(
        output.stdout,
        "cpu false true\n\
         Array[float32](shape=[2, 3], values=[2.0, 5.0, 8.0, 11.0, 14.0, 17.0])\n\
         [2, 3] 6 cpu\n\
         57.0 2.0 17.0 9.5\n\
         Array[float32](shape=[3], values=[13.0, 19.0, 25.0])\n\
         Array[float32](shape=[2, 2], values=[12.0, 12.0, 30.0, 30.0])\n\
         Array[float32](shape=[2, 3], values=[1.0, 2.0, 2.0, 2.0, 2.0, 2.0])\n\
         Array[float32](shape=[2, 3], values=[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])\n\
         true true\n\
         Array[bool](shape=[2, 3], values=[false, false, false, true, true, true])\n\
         Array[float32](shape=[2, 3], values=[-1.0, -2.0, -3.0, 4.0, 5.0, 6.0])\n\
         Array[bool](shape=[2, 3], values=[true, false, false, false, false, false]) Array[bool](shape=[2, 3], values=[true, true, false, false, false, false])\n\
         Array[bool](shape=[2, 3], values=[false, true, true, true, true, true]) Array[bool](shape=[2, 3], values=[false, true, false, false, false, false]) Array[bool](shape=[2, 3], values=[true, false, true, true, true, true])\n\
         Array[float32](shape=[6], values=[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])\n\
         Array[int64](shape=[4], values=[14, 14, 14, 14])\n\
         Array[float32](shape=[4], values=[7.0, 7.0, 7.0, 7.0]) Array[int32](shape=[4], values=[7, 7, 7, 7]) Array[float64](shape=[4], values=[7.0, 7.0, 7.0, 7.0])\n\
         Array[int64](shape=[2, 3], values=[1, 2, 3, 4, 5, 6])\n\
         Array[int32](shape=[2], values=[0, 0]) Array[bool](shape=[2], values=[false, false])\n\
         0.5\n\
         Array[float32](shape=[2], values=[8.0, 17.0])\n\
         true\n"
    );

    let error = crate::check_source(
        "import tensor\n\ndef forge() -> tensor.Node:\n    return tensor.Node()\n",
    )
    .expect_err("only the tensor module makes a node");
    assert!(error.message.contains("Node"), "{}", error.message);
    let error = crate::check_source(
        "from tensor import Device, Tensor\n\ndef bad(x: Tensor[float32], y: Tensor[int64]) -> Tensor[float32]:\n    return x + y\n",
    )
    .expect_err("operands have one dtype");
    assert!(error.message.contains("Tensor"), "{}", error.message);
    let error = crate::run_source(
        "from tensor import Device, Tensor\n\ndef main():\n    x = Tensor[float32].zeros([2, 3], Device.cpu())\n    y = Tensor[float32].zeros([4], Device.cpu())\n    print((x + y).len())\n",
    )
    .expect_err("shapes must broadcast");
    assert_eq!(error.code, "AU4007");
}

#[test]
fn an_associated_trait_call_runs_the_implementation_for_the_written_type_arguments() {
    let output = crate::run_source(
        r#"trait Make[E]:
    def make(value: E) -> Self
    def what(self) -> str

class Box[T]:
    size: int64

impl Make[float32] for Box[float32]:
    def make(value: float32) -> Box[float32]:
        return Box[float32](size=1)
    def what(self) -> str:
        return "float32"

impl Make[int64] for Box[int64]:
    def make(value: int64) -> Box[int64]:
        return Box[int64](size=2)
    def what(self) -> str:
        return "int64"

def main():
    a = Box[float32].make(1.5)
    b = Box[int64].make(3)
    print(f"{a.what()} {a.size} {b.what()} {b.size}")
"#,
    )
    .expect("each call names one implementation");
    assert_eq!(output.stdout, "float32 1 int64 2\n");
}

#[test]
fn tensors_are_indexed_joined_and_reduced_with_kept_axes() {
    let output = crate::run_source(
        r#"import tensor
from tensor import Device, Tensor

def main():
    device = Device.cpu()
    x = Tensor[float32].from_array(Array[float32].from_list([1.0, -2.0, 3.0, 4.5, 0.25, 6.0], [2, 3]), device)
    rows = Tensor[int64].from_array(Array[int64].from_list([1, 1, 0], [3]), device)
    print(x.slice(1, 1, 3).to_array())
    print(x.slice(1, 0, -1, 2).to_array())
    print(x.index_select(0, rows).shape())
    print(x.index_select(1, rows).to_array())
    print(tensor.cat([x.clone(), x.clone()], 1).shape())
    print(tensor.cat([x.clone(), x.clone(), x.clone()]).shape())
    print(x.index_add(Tensor[int64].from_array(Array[int64].from_list([1, 1], [2]), device), 3).to_array())
    print(x.argmax().to_array())
    print(x.argmin(0).to_array())
    print(x.sum(1, true).to_array())
    print(x.max(keepdim=true).shape())
    print(x.mean(-2, true).shape())
    print(x.slice(0, 0, 1).expand([2, 2, 3]).shape())
    print(tensor.arange(4, device).to_array())
    print(tensor.rand([2, 2], 7, device).shape())
    drift = (tensor.randn([3], 7, device) - tensor.randn([3], 7, device)).abs().max().item()
    print(drift == 0.0)
    print(x.softmax().sum(1).to_array())
    print(f"{x.tanh().shape()} {x.sigmoid().shape()} {x.erf().shape()} {x.gelu().shape()}")
"#,
    )
    .expect("the indexed members run");
    assert_eq!(
        output.stdout,
        "Array[float32](shape=[2, 2], values=[-2.0, 3.0, 0.25, 6.0])\n\
         Array[float32](shape=[2, 1], values=[1.0, 4.5])\n\
         [3, 3]\n\
         Array[float32](shape=[2, 3], values=[-2.0, -2.0, 1.0, 0.25, 0.25, 4.5])\n\
         [2, 6]\n\
         [6, 3]\n\
         Array[float32](shape=[3, 3], values=[0.0, 0.0, 0.0, 5.5, -1.75, 9.0, 0.0, 0.0, 0.0])\n\
         Array[int64](shape=[2], values=[2, 2])\n\
         Array[int64](shape=[3], values=[0, 0, 0])\n\
         Array[float32](shape=[2, 1], values=[2.0, 10.75])\n\
         [1, 1]\n\
         [1, 3]\n\
         [2, 2, 3]\n\
         Array[int64](shape=[4], values=[0, 1, 2, 3])\n\
         [2, 2]\n\
         true\n\
         Array[float32](shape=[2], values=[1.0, 1.0])\n\
         [2, 3] [2, 3] [2, 3] [2, 3]\n"
    );
}

#[test]
fn a_tensor_program_learns_by_gradient_descent() {
    let output = crate::run_source(
        r#"import tensor
from tensor import Device, Tensor

def main():
    device = Device.cpu()
    # y = 2 * x0 - 3 * x1 + 1
    inputs = Tensor[float32].from_array(Array[float32].from_list([1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, -1.0], [4, 2]), device)
    wanted = Tensor[float32].from_array(Array[float32].from_list([3.0, -2.0, 0.0, 8.0], [4, 1]), device)
    mut weights = Tensor[float32].zeros([2, 1], device).requires_grad()
    mut bias = Tensor[float32].zeros([1], device).requires_grad()
    mut step = 0
    while step < 300:
        error = inputs.matmul(weights) + bias - wanted
        loss = (error * error).mean()
        weights = (weights.detach() - loss.gradient(weights) * 0.1).requires_grad()
        bias = (bias.detach() - loss.gradient(bias) * 0.1).requires_grad()
        step += 1
    first = weights.slice(0, 0, 1).sum().item()
    second = weights.slice(0, 1, 2).sum().item()
    offset = bias.item()
    print(f"{first > 1.9 and first < 2.1} {second > -3.1 and second < -2.9} {offset > 0.9 and offset < 1.1}")

    logits = Tensor[float32].from_array(Array[float32].from_list([2.0, 0.0, 0.0, 0.0, 0.0, 2.0], [2, 3]), device).requires_grad()
    targets = Tensor[int64].from_array(Array[int64].from_list([0, 1], [2]), device)
    loss = tensor.cross_entropy(logits, targets)
    print(f"{loss.item() > 1.2} {loss.item() < 1.3}")
    drift = loss.gradient(logits).sum().item()
    print(drift > -0.000001 and drift < 0.000001)
    print(loss.gradient(logits).argmin().to_array())
"#,
    )
    .expect("a tensor program learns");
    assert_eq!(
        output.stdout,
        "true true true\ntrue true\ntrue\nArray[int64](shape=[2], values=[0, 1])\n"
    );
}

#[test]
fn a_small_transformer_learns_with_the_tensor_layers() {
    let output = crate::run_source(
        r#"import tensor
from tensor import Device, Tensor, Parameter, LayerNorm, MultiheadAttention, Linear, Embedding, AdamW

class Block:
    norm1: LayerNorm
    mix: MultiheadAttention
    norm2: LayerNorm
    wide: Linear
    narrow: Linear

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        attended = x + self.mix.forward(self.norm1.forward(x), true)
        return attended + self.narrow.forward(self.wide.forward(self.norm2.forward(attended)).gelu())

    def parameters(mut self) -> list[Parameter[float32]]:
        mut all = self.norm1.parameters()
        all.extend(self.mix.parameters())
        all.extend(self.norm2.parameters())
        all.extend(self.wide.parameters())
        all.extend(self.narrow.parameters())
        return all


def main():
    device = Device.cpu()
    width = 16
    mut tokens = tensor.Embedding(5, width, 1, device)
    mut places = tensor.Embedding(8, width, 2, device)
    mut block = Block(norm1=tensor.LayerNorm(width, device), mix=tensor.MultiheadAttention(width, 2, 10, device), norm2=tensor.LayerNorm(width, device), wide=tensor.Linear(width, 32, 20, device), narrow=tensor.Linear(32, width, 30, device))
    mut head = tensor.Linear(width, 5, 40, device)
    mut params = tokens.parameters()
    params.extend(places.parameters())
    params.extend(block.parameters())
    params.extend(head.parameters())
    mut optimizer = tensor.AdamW(params, lr=0.01)
    # Each row counts up by one, modulo five. The model learns the next token.
    inputs = Tensor[int64].from_array(Array[int64].from_list([0, 1, 2, 3, 4, 0, 1, 2, 2, 3, 4, 0, 1, 2, 3, 4], [2, 8]), device)
    wanted = Tensor[int64].from_array(Array[int64].from_list([1, 2, 3, 4, 0, 1, 2, 3, 3, 4, 0, 1, 2, 3, 4, 0], [16]), device)
    positions = tensor.arange(8, device)
    mut step = 0
    while step < 40:
        hidden = block.forward(tokens.forward(inputs) + places.forward(positions))
        logits = head.forward(hidden).reshape([16, 5])
        loss = tensor.cross_entropy(logits, wanted)
        optimizer.minimize(loss)
        if step == 0 or step == 39:
            print(loss.item() < 0.01)
        step += 1
    hidden = block.forward(tokens.forward(inputs) + places.forward(positions))
    print(head.forward(hidden).reshape([16, 5]).argmax().to_array())
    kept = tensor.dropout(Tensor[float32].full([1000], 1.0, device), 0.25, 3).sum().item()
    print(kept > 900.0 and kept < 1100.0)
"#,
    )
    .expect("a transformer trains");
    assert_eq!(
        output.stdout,
        "false\ntrue\nArray[int64](shape=[16], values=[1, 2, 3, 4, 0, 1, 2, 3, 3, 4, 0, 1, 2, 3, 4, 0])\ntrue\n"
    );
}

#[test]
fn a_mixture_of_experts_keeps_its_top_experts_and_learns() {
    // The program names neither `Linear` nor `Parameter`, which the
    // layer's own methods use.
    let output = crate::run_source(
        r#"import tensor
from tensor import Device, Tensor, MixtureOfExperts, AdamW

def main():
    device = Device.cpu()
    x = Tensor[float32].from_array(Array[float32].from_list([1.0, 5.0, 3.0, 4.0, 9.0, 2.0, 8.0, 7.0], [2, 4]), device)
    print(tensor.top_indices(x, 2).to_array())
    print(tensor.top_mask(x, 2).to_array())
    print(tensor.one_hot(x.argmax(), 4).to_array())
    mut layer = MixtureOfExperts(4, 8, 3, 2, 7, device)
    weights = layer.weights(x)
    one = Tensor[float32].full([1], 1.0, device)
    print(tensor.where(weights.greater(one * 0.0), one, one * 0.0).sum(1).to_array())
    total = weights.sum().item()
    print(total > 1.9999 and total < 2.0001)
    rows = tensor.randn([64, 4], 1, device)
    wanted = rows * rows - 1.0
    mut optimizer = AdamW(layer.parameters(), lr=0.01)
    mut first: float32 = 0.0
    mut last: float32 = 0.0
    for step in range(200):
        gap = layer.forward(rows) - wanted
        loss = (gap * gap).mean()
        optimizer.minimize(loss)
        if step == 0:
            first = loss.item()
        last = loss.item()
    print(last < first * 0.2)
"#,
    )
    .expect("a mixture of experts trains");
    assert_eq!(
        output.stdout,
        "Array[int64](shape=[2, 2], values=[1, 3, 0, 2])
Array[bool](shape=[2, 4], values=[false, true, false, true, true, false, true, false])
Array[bool](shape=[2, 4], values=[false, true, false, false, true, false, false, false])
Array[float32](shape=[2], values=[2.0, 2.0])
true
true
"
    );
}

#[test]
fn a_model_trains_with_backward_and_an_optimizer_step() {
    let output = crate::run_source(
        r#"import tensor
from tensor import Device, Tensor, Parameter, Linear, AdamW

class Net:
    hidden: Linear
    output: Linear

    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        return self.output.forward(self.hidden.forward(x).relu())

    def parameters(mut self) -> list[Parameter[float32]]:
        mut all = self.hidden.parameters()
        all.extend(self.output.parameters())
        return all


def main():
    device = Device.cpu()
    x = tensor.randn([64, 4], 1, device)
    y = (x * x).sum(1, true)
    mut model = Net(hidden=Linear(4, 16, 2, device), output=Linear(16, 1, 3, device))
    mut optimizer = AdamW(model.parameters(), lr=0.01)
    mut first: float32 = 0.0
    mut last: float32 = 0.0
    for step in range(300):
        gap = model.forward(x) - y
        loss = (gap * gap).mean()
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        if step == 0:
            first = loss.item()
        last = loss.item()
    print(last < first * 0.2)
    # Gradients add up until the parameter is replaced.
    mut w = Parameter[float32](Tensor[float32].full([2], 3.0, device))
    (w.value() * w.value()).sum().backward()
    print(w.grad.to_array())
    (w.value() * 2.0).sum().backward()
    print(w.grad.to_array())
    moved = w.value().detach() - w.grad * 0.5
    w.copy_(moved)
    print(w.value().to_array())
    print(w.grad.to_array())
"#,
    )
    .expect("a model trains");
    assert_eq!(
        output.stdout,
        "true\nArray[float32](shape=[2], values=[6.0, 6.0])\nArray[float32](shape=[2], values=[8.0, 8.0])\nArray[float32](shape=[2], values=[-1.0, -1.0])\nArray[float32](shape=[2], values=[0.0, 0.0])\n"
    );
}

#[test]
fn the_weights_of_a_model_not_declared_mut_cannot_change() {
    let check = |body: &str| {
        let source = format!(
            "import tensor\nfrom tensor import Device, Tensor, Parameter, Linear, SGD\n\ndef main():\n    device = Device.cpu()\n{body}"
        );
        crate::check_source(&source)
            .map(|_| ())
            .map_err(|error| error.message)
    };
    // A `mut` layer hands its parameters to an optimizer.
    check("    mut layer = Linear(2, 1, 7, device)\n    mut optimizer = SGD(layer.parameters(), lr=0.1)\n    optimizer.step()\n")
        .expect("a mut layer trains");
    // A layer not declared `mut` does not, and neither private handles
    // nor moves give a second way to its weights.
    let zeros = "Tensor[float32].zeros([2, 1], device)";
    let read = "    print(layer.forward(Tensor[float32].zeros([1, 2], device)).to_array())\n";
    let cases = [
        (
            "    layer = Linear(2, 1, 7, device)\n    optimizer = SGD(layer.parameters(), lr=0.1)\n".to_string(),
            "method `parameters` requires a mutable receiver",
        ),
        (
            format!("    layer = Linear(2, 1, 7, device)\n    layer.weight.copy_({zeros})\n"),
            "method `copy_` requires a mutable receiver",
        ),
        (
            format!("    mut layer = Linear(2, 1, 7, device)\n    layer.weight.assign({zeros})\n"),
            "method `assign` is private on `Parameter`",
        ),
        (
            "    mut layer = Linear(2, 1, 7, device)\n    other = layer.weight.share()\n".to_string(),
            "method `share` is private on `Parameter`",
        ),
        (
            "    mut layer = Linear(2, 1, 7, device)\n    layer.weight.zero_grad()\n".to_string(),
            "method `zero_grad` is private on `Parameter`",
        ),
        (
            "    mut layer = Linear(2, 1, 7, device)\n    optimizer = SGD(layer.parameters(), lr=0.1)\n    optimizer.step()\n".to_string(),
            "method `step` requires a mutable receiver",
        ),
        (
            format!("    layer = Linear(2, 1, 7, device)\n    mut stolen = layer.weight\n    stolen.copy_({zeros})\n{read}"),
            "use of partially moved value `layer`",
        ),
        (
            format!("    layer = Linear(2, 1, 7, device)\n    mut copy = layer\n    mut optimizer = SGD(copy.parameters(), lr=0.1)\n{read}"),
            "use of moved value `layer`",
        ),
        (
            format!("    layer = Linear(2, 1, 7, device)\n    mut twin = Linear(2, 1, 7, device)\n    twin.weight = layer.weight\n{read}"),
            "use of partially moved value `layer`",
        ),
    ];
    for (body, expected) in cases {
        assert_eq!(check(&body).expect_err(&body), expected);
    }
}

#[test]
fn a_tensor_operator_takes_a_number_on_either_side() {
    let output = crate::run_source(
        r#"from tensor import Device, Tensor

def main():
    device = Device.cpu()
    x = Tensor[float32].from_array(Array[float32].from_list([1.0, -2.0, 3.0], [3]), device)
    k = Tensor[int64].from_array(Array[int64].from_list([1, 2, 3], [3]), device)
    half: float32 = 0.5
    two: int32 = 2
    print((x * 2.0).to_array())
    print((x * half + 1).to_array())
    print((2.0 * x - 1.0).to_array())
    print((1 - x).to_array())
    print((6 / x).to_array())
    print((-x / two).to_array())
    print((k * 3 + 1).to_array())
    print((10 - k).to_array())
    print(x.relu().to_array())
    print(x.clamp(-1.0, 2.0).to_array())
    print(x.powf(2.0).to_array())
"#,
    )
    .expect("numbers combine with tensors");
    assert_eq!(
        output.stdout,
        "Array[float32](shape=[3], values=[2.0, -4.0, 6.0])\n\
         Array[float32](shape=[3], values=[1.5, 0.0, 2.5])\n\
         Array[float32](shape=[3], values=[1.0, -5.0, 5.0])\n\
         Array[float32](shape=[3], values=[0.0, 3.0, -2.0])\n\
         Array[float32](shape=[3], values=[6.0, -3.0, 2.0])\n\
         Array[float32](shape=[3], values=[-0.5, 1.0, -1.5])\n\
         Array[int64](shape=[3], values=[4, 7, 10])\n\
         Array[int64](shape=[3], values=[9, 8, 7])\n\
         Array[float32](shape=[3], values=[1.0, 0.0, 3.0])\n\
         Array[float32](shape=[3], values=[1.0, -1.0, 2.0])\n\
         Array[float32](shape=[3], values=[1.0, 4.0, 9.0])\n"
    );

    let error = crate::run_source(
        r#"from tensor import Device, Tensor

def main():
    k = Tensor[int64].full([2], 1, Device.cpu())
    print((k * 1.5).to_array())
"#,
    )
    .expect_err("a float does not combine with an integer tensor");
    assert_eq!(error.code, "AU4007");
    assert!(error
        .message
        .contains("cannot use a float with a `Tensor[int64]`"));
}

#[test]
fn tensors_of_two_dtypes_do_not_combine() {
    let error = crate::run_source(
        r#"from tensor import Device, Tensor

def main():
    device = Device.cpu()
    x = Tensor[float32].full([2], 1.0, device)
    k = Tensor[int64].full([2], 1, device)
    print((x + k).to_array())
"#,
    )
    .expect_err("the dtypes differ");
    assert!(error.message.contains("Tensor"), "{}", error.message);
}

#[test]
fn a_generic_implementation_satisfies_a_bound_for_any_trait_argument() {
    let output = crate::run_source(
        r#"trait Scale[T]:
    def factor(self) -> float64

class Box[T]:
    value: float64

impl[T] Scale[T] for float64:
    def factor(self) -> float64:
        return self

impl[T] Scale[T] for Box[T]:
    def factor(self) -> float64:
        return self.value

impl[T, R: Scale[T]] Add[R, Box[T]] for Box[T]:
    def add(self, rhs: R) -> Box[T]:
        return Box[T](value=self.value + rhs.factor())

def main():
    a = Box[int32](value=1.0)
    b = Box[int32](value=10.0)
    print(f"{(a + 2.5).value} {(a + b).value}")
"#,
    )
    .expect("float64 implements Scale[int32]");
    assert_eq!(output.stdout, "3.5 11.0\n");
}

#[test]
fn a_model_is_a_module_with_named_parameters_and_a_state_dict() {
    let output = crate::run_source(
        r#"import tensor
from tensor import AdamW, Collector, Device, Layer, Linear, Module, ReLU, Sequential, Tensor


class Net:
    hidden: Linear
    output: Linear


impl Module for Net:
    def collect(self, into: mut Collector) -> None:
        into.add("hidden", self.hidden)
        into.add("output", self.output)


impl Layer for Net:
    def forward(self, x: Tensor[float32]) -> Tensor[float32]:
        return self.output.forward(self.hidden.forward(x).relu())


def main() -> int32:
    device = Device.cpu()
    mut net = Net(hidden=Linear(4, 8, 1, device), output=Linear(8, 2, 2, device))
    x = tensor.randn([16, 4], 3, device)
    labels = x.matmul(tensor.randn([4, 2], 4, device)).argmax()
    mut optimizer = AdamW(net.parameters(), lr=0.05)
    mut first: float32 = 0.0
    mut last: float32 = 0.0
    for step in range(30):
        loss = tensor.cross_entropy(net.forward(x), labels)
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
        if step == 0:
            first = loss.item()
        last = loss.item()
    print(last < first)
    for name, param in net.named_parameters():
        print(f"{name} {param.value().shape()}")
    state = net.state_dict()
    print(state.len())
    mut other = Net(hidden=Linear(4, 8, 7, device), output=Linear(8, 2, 8, device))
    match other.load_state_dict(state):
        case Ok(_):
            print((other.forward(x) - net.forward(x)).abs().max().item())
        case Err(message):
            print(message)
    mut stack = Sequential([Linear(4, 8, 1, device) as dyn Layer, ReLU(), Linear(8, 2, 2, device)])
    print(stack.forward(x).shape())
    for name, param in stack.named_parameters():
        print(name)
    return 0
"#,
    )
    .expect("a module trains, names its parameters, and loads a state dict");
    assert_eq!(
        output.stdout,
        "true\nhidden.weight [4, 8]\nhidden.bias [8]\noutput.weight [8, 2]\noutput.bias [2]\n4\n0.0\n[16, 2]\n0.weight\n0.bias\n2.weight\n2.bias\n"
    );
}

#[test]
fn a_module_not_declared_mut_gives_out_no_parameters() {
    let check = |collect: &str, body: &str| {
        let source = format!(
            "import tensor\nfrom tensor import Collector, Device, Linear, Module, Parameter, SGD, Tensor\n\nclass Net:\n    hidden: Linear\n    output: Linear\n\nimpl Module for Net:\n    def collect(self, into: mut Collector) -> None:\n        into.add(\"hidden\", self.hidden)\n        into.add(\"output\", self.output)\n{collect}\ndef main():\n    device = Device.cpu()\n    net = Net(hidden=Linear(2, 2, 1, device), output=Linear(2, 1, 2, device))\n{body}"
        );
        crate::check_source(&source)
            .map(|_| ())
            .map_err(|error| error.message)
    };
    // Reading a model's weights needs no `mut`.
    check("", "    state = net.state_dict()\n").expect("a state dict reads the model");
    let cases = [
        (
            "",
            "    optimizer = SGD(net.parameters(), lr=0.1)\n",
            "method `parameters` requires a mutable receiver",
        ),
        (
            "",
            "    names = net.named_parameters()\n",
            "method `named_parameters` requires a mutable receiver",
        ),
        (
            "",
            "    done = net.load_state_dict(net.state_dict())\n",
            "method `load_state_dict` requires a mutable receiver",
        ),
        // `collect` runs on a model that is not `mut`, so what it gathers
        // stays inside the `tensor` module.
        (
            "        stolen = into.params\n",
            "",
            "field `params` is private on `Collector`",
        ),
        (
            "",
            "    into = Collector(names=list[str](), params=list[Parameter[float32]](), prefix=\"\")\n",
            "field `names` is private on `Collector`",
        ),
        (
            "",
            "    into = tensor.collected(net)\n",
            "item `collected` is private in module `tensor`",
        ),
    ];
    for (collect, body, expected) in cases {
        assert_eq!(check(collect, body).expect_err(body), expected);
    }
}
