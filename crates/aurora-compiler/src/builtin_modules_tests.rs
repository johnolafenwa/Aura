use super::{builtin_imported_binding, builtin_module_namespace, host_builtin_metadata};
use crate::ast::ReceiverKind;
use crate::diag::Span;
use crate::sema::ImportedBinding;

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
fn host_builtin_metadata_is_derived_from_function_namespaces() {
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
                &crate::sema::Type::named("String"),
                ReceiverKind::Borrow,
                true
            ),
            (
                "child",
                &crate::sema::Type::named("String"),
                ReceiverKind::Borrow,
                true
            ),
        ]
    );
    assert_eq!(join.return_type, crate::sema::Type::named("String"));

    let increment =
        host_builtin_metadata("metrics::increment").expect("metrics.increment metadata");
    assert_eq!(increment.params[0].passing, ReceiverKind::Borrow);
    assert_eq!(increment.params[1].passing, ReceiverKind::Value);
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
        crate::sema::Type::Named("Vec".to_string(), vec![crate::sema::Type::named("uint8")])
    );

    assert!(host_builtin_metadata("fs::exists").is_none());
    assert!(host_builtin_metadata("missing::function").is_none());
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
        crate::sema::Type::Named("Vec".to_string(), vec![crate::sema::Type::named("uint8")])
    );
    assert!(!namespace.functions.contains_key("secure_float"));

    assert!(matches!(
        builtin_imported_binding(&["random".to_string()], "Rng", Span::new(1, 1))
            .expect("Rng should import"),
        ImportedBinding::Class(_)
    ));
}
