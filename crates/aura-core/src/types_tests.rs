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

#[test]
fn a_trait_object_type_is_dyn_over_one_trait() {
    let tool = Type::trait_object("Tool", Vec::new());
    assert_eq!(tool.to_string(), "dyn Tool");
    assert_eq!(tool.as_trait_object(), Some(("Tool", &[][..])));
    assert!(tool.is_trait_object());
    let reader = Type::trait_object("io.Reader", vec![Type::named("int64")]);
    assert_eq!(reader.to_string(), "dyn io.Reader[int64]");
    assert_eq!(
        reader.trait_object_bound(),
        Some(TraitBound {
            trait_name: "io.Reader".to_string(),
            trait_args: vec![Type::named("int64")],
        })
    );

    // A class named like the marker with any other shape is no trait object.
    for other in [
        Type::named("Tool"),
        Type::named(TRAIT_OBJECT_TYPE_NAME),
        Type::Named(
            TRAIT_OBJECT_TYPE_NAME.to_string(),
            vec![Type::TypeParam("T".to_string())],
        ),
        Type::Unit,
    ] {
        assert!(!other.is_trait_object(), "{other:?}");
        assert_eq!(other.trait_object_bound(), None);
    }

    // The marker is no nominal of any module: two modules that name the
    // same trait agree on the key.
    let names = BTreeMap::new();
    let imported = Type::trait_object("shapes.Shape", Vec::new());
    assert_eq!(
        imported.canonical_key("main", &names),
        imported.canonical_key("other", &names)
    );
    assert_ne!(
        tool.canonical_key("main", &names),
        tool.canonical_key("other", &names),
        "a bare trait name belongs to the module that reads it"
    );
}

#[test]
fn names_trait_object_finds_one_anywhere_in_a_type() {
    let tool = Type::trait_object("Tool", Vec::new());
    for plain in descriptor_corpus() {
        assert!(!plain.names_trait_object(), "{plain:?}");
    }
    let param = |ty: Type| FunctionParamContract {
        keyword_only: false,
        name: "value".to_string(),
        ty,
        passing: ReceiverKind::Borrow,
        has_default: false,
    };
    let holders = [
        tool.clone(),
        Type::Named("list".to_string(), vec![tool.clone()]),
        Type::Named(
            "dict".to_string(),
            vec![
                Type::named("str"),
                Type::Named("list".to_string(), vec![tool.clone()]),
            ],
        ),
        Type::Tuple(vec![Type::named("int64"), tool.clone()]),
        Type::normalize_union(vec![tool.clone(), Type::Unit], "main", &BTreeMap::new())
            .expect("a union with a trait object member"),
        Type::Function {
            params: vec![param(tool.clone())],
            return_type: Box::new(Type::Unit),
        },
        Type::Function {
            params: Vec::new(),
            return_type: Box::new(tool.clone()),
        },
        Type::Callable(Box::new(CallableType {
            task: false,
            call_kind: ClosureCallKind::Repeatable,
            params: vec![param(tool.clone())],
            return_type: Type::Unit,
        })),
        Type::Callable(Box::new(CallableType {
            task: false,
            call_kind: ClosureCallKind::Repeatable,
            params: Vec::new(),
            return_type: tool.clone(),
        })),
        Type::ReturnedView(Box::new(ReturnedViewType {
            mutable: false,
            pointee: tool.clone(),
            origin: 0,
        })),
        Type::Closure {
            params: Box::new(vec![param(tool.clone())]),
            return_type: Box::new(Type::Unit),
            captures: Box::new(Vec::new()),
            call_kind: ClosureCallKind::Repeatable,
        },
        Type::Closure {
            params: Box::new(Vec::new()),
            return_type: Box::new(tool.clone()),
            captures: Box::new(Vec::new()),
            call_kind: ClosureCallKind::Repeatable,
        },
        Type::Closure {
            params: Box::new(Vec::new()),
            return_type: Box::new(Type::Unit),
            captures: Box::new(vec![ClosureCapture {
                name: "tool".to_string(),
                ty: tool.clone(),
                mode: ClosureCaptureMode::Move,
                span: Span::new(1, 1),
                mutated: false,
            }]),
            call_kind: ClosureCallKind::Repeatable,
        },
    ];
    for holder in holders {
        assert!(holder.names_trait_object(), "{holder:?}");
    }
}
