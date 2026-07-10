use super::builtin_imported_binding;
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
