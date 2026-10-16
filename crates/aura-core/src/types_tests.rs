use std::collections::BTreeMap;

use super::*;
use crate::diag::Span;

fn legacy_canonical_key(
    ty: &Type,
    module_name: &str,
    canonical_names: &BTreeMap<String, String>,
) -> String {
    fn key(ty: &Type, module: &str, names: &BTreeMap<String, String>) -> serde_json::Value {
        use serde_json::json;
        match ty {
            Type::Unit => json!(["~unit"]),
            Type::Module(name) => json!(["module", name]),
            Type::TypeParam(name) => json!(["parameter", name]),
            Type::ReturnedView(view) => json!([
                "returned_view",
                view.mutable,
                view.origin,
                key(&view.pointee, module, names)
            ]),
            Type::Named(name, args) => {
                let resolved = names.get(name).unwrap_or(name);
                let nominal =
                    if is_builtin_type(resolved) || resolved.contains('.') || module.is_empty() {
                        resolved.clone()
                    } else {
                        format!("{module}.{resolved}")
                    };
                json!([
                    "named",
                    nominal,
                    args.iter()
                        .map(|arg| key(arg, module, names))
                        .collect::<Vec<_>>()
                ])
            }
            Type::Tuple(elements) => json!([
                "tuple",
                elements
                    .iter()
                    .map(|arg| key(arg, module, names))
                    .collect::<Vec<_>>()
            ]),
            Type::Union(union) => json!([
                "union",
                union
                    .members
                    .iter()
                    .map(|member| key(member, &union.module_name, &BTreeMap::new()))
                    .collect::<Vec<_>>()
            ]),
            Type::Function {
                params,
                return_type,
            } => json!([
                "function",
                params
                    .iter()
                    .map(|p| json!([
                        p.name,
                        p.passing,
                        p.has_default,
                        p.keyword_only,
                        key(&p.ty, module, names)
                    ]))
                    .collect::<Vec<_>>(),
                key(return_type, module, names)
            ]),
            Type::Callable(callable) => json!([
                "callable",
                callable.task,
                callable.call_kind,
                callable
                    .params
                    .iter()
                    .map(|p| json!([
                        p.name,
                        p.passing,
                        p.has_default,
                        p.keyword_only,
                        key(&p.ty, module, names)
                    ]))
                    .collect::<Vec<_>>(),
                key(&callable.return_type, module, names)
            ]),
            Type::Closure {
                params,
                return_type,
                captures,
                call_kind,
            } => json!([
                "closure",
                call_kind,
                params
                    .iter()
                    .map(|p| json!([
                        p.name,
                        p.passing,
                        p.has_default,
                        p.keyword_only,
                        key(&p.ty, module, names)
                    ]))
                    .collect::<Vec<_>>(),
                key(return_type, module, names),
                captures
                    .iter()
                    .map(|capture| json!([
                        capture.name,
                        capture.mode,
                        key(&capture.ty, module, names)
                    ]))
                    .collect::<Vec<_>>()
            ]),
        }
    }
    format!("aura-type-key-v1:{}", key(ty, module_name, canonical_names))
}

fn param(
    name: &str,
    ty: Type,
    passing: ReceiverKind,
    keyword_only: bool,
    has_default: bool,
) -> FunctionParamContract {
    FunctionParamContract {
        keyword_only,
        name: name.to_string(),
        ty,
        passing,
        has_default,
    }
}

pub(crate) fn descriptor_corpus() -> Vec<Type> {
    let union = Type::normalize_union(
        vec![Type::named("int32"), Type::named("str"), Type::Unit],
        "main",
        &BTreeMap::new(),
    )
    .expect("a three-member union normalizes");
    vec![
        Type::Unit,
        Type::named("int64"),
        Type::Named("list".to_string(), vec![Type::named("uint8")]),
        Type::Named("Point".to_string(), Vec::new()),
        Type::Named(
            "dict".to_string(),
            vec![Type::named("str"), Type::named("we\"ird\\name\n\u{1}é")],
        ),
        Type::Tuple(vec![Type::named("bool"), Type::Unit]),
        Type::Tuple(Vec::new()),
        union.clone(),
        Type::TypeParam("T".to_string()),
        Type::Module("json".to_string()),
        Type::Function {
            params: vec![
                param(
                    "value",
                    Type::named("int32"),
                    ReceiverKind::Borrow,
                    false,
                    false,
                ),
                param("", union.clone(), ReceiverKind::Value, true, true),
                param(
                    "sink",
                    Type::named("str"),
                    ReceiverKind::BorrowMut,
                    false,
                    false,
                ),
            ],
            return_type: Box::new(Type::ReturnedView(Box::new(ReturnedViewType {
                mutable: true,
                pointee: Type::named("str"),
                origin: 2,
            }))),
        },
        Type::Callable(Box::new(CallableType {
            task: true,
            call_kind: ClosureCallKind::Consuming,
            params: vec![param(
                "x",
                Type::named("int64"),
                ReceiverKind::Value,
                false,
                false,
            )],
            return_type: Type::Unit,
        })),
        Type::Closure {
            params: Box::new(vec![param(
                "x",
                Type::named("int64"),
                ReceiverKind::Borrow,
                false,
                true,
            )]),
            return_type: Box::new(Type::named("int64")),
            captures: Box::new(vec![
                ClosureCapture {
                    name: "total".to_string(),
                    ty: Type::named("int64"),
                    mode: ClosureCaptureMode::Copy,
                    span: Span::new(4, 9),
                    mutated: true,
                },
                ClosureCapture {
                    name: "items".to_string(),
                    ty: Type::Named("list".to_string(), vec![Type::named("str")]),
                    mode: ClosureCaptureMode::MutableView,
                    span: Span::new(5, 1),
                    mutated: false,
                },
            ]),
            call_kind: ClosureCallKind::MutableRepeatable,
        },
    ]
}

#[test]
fn canonical_keys_match_the_persisted_json_format() {
    let mut names = BTreeMap::new();
    names.insert("Point".to_string(), "geometry.Point".to_string());
    for ty in descriptor_corpus() {
        for (module, names) in [
            ("main", &names),
            ("", &BTreeMap::new()),
            ("app.util", &BTreeMap::new()),
        ] {
            assert_eq!(
                ty.canonical_key(module, names),
                legacy_canonical_key(&ty, module, names),
                "key for {ty:?} in `{module}`"
            );
        }
    }
}
