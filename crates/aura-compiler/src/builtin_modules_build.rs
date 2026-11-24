use super::*;
use crate::ast::MapEntryExpr;

pub(super) fn namespace() -> ModuleNamespace {
    let config = crate::current_build_config();
    let mut namespace = function_only_namespace("build", Vec::new());
    let mut add = |name: &str, annotation: TypeRef, value: Expr| {
        let ty = lower_type_ref(&annotation);
        namespace.constants.insert(
            name.into(),
            crate::sema::ConstantInfo {
                module_name: "build".into(),
                decl: ConstantDecl {
                    required: true,
                    public: true,
                    name: name.into(),
                    annotation: Some(annotation),
                    value,
                    span: builtin_span(),
                },
                ty,
            },
        );
    };
    let string = || type_ref("str", Vec::new());
    let text = |value: String| Expr {
        kind: ExprKind::String(value),
        span: builtin_span(),
    };
    for (name, value) in [
        ("os", config.os),
        ("arch", config.arch),
        ("endianness", config.endianness),
    ] {
        add(name, string(), text(value));
    }
    add(
        "pointer_width",
        type_ref("int64", Vec::new()),
        int_expr(config.pointer_width.into()),
    );
    for (name, value) in [
        ("debug", config.debug),
        ("release", config.release),
        ("hosted", config.hosted),
        ("freestanding", config.freestanding),
    ] {
        add(name, type_ref("bool", Vec::new()), bool_expr(value));
    }
    let entries = config
        .cfg
        .into_iter()
        .map(|(key, value)| MapEntryExpr {
            key: text(key),
            value: text(value),
        })
        .collect();
    add(
        "cfg",
        type_ref("dict", vec![string(), string()]),
        Expr {
            kind: ExprKind::Map(entries),
            span: builtin_span(),
        },
    );
    namespace.all_constants.clone_from(&namespace.constants);
    namespace
}
