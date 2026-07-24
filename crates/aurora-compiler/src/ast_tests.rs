use super::{
    BindingTarget, ClassDecl, EnumDecl, Expr, ExprKind, FieldDecl, ForStmt, FunctionDecl, ImplDecl,
    Item, ReceiverKind, TraitDecl, TypeRef,
};
use crate::diag::Span;
use std::collections::BTreeMap;

fn dummy_type(name: &str) -> TypeRef {
    TypeRef::named(name, vec![], false, Span::new(1, 1))
}

fn dummy_function(name: &str) -> FunctionDecl {
    FunctionDecl {
        public: false,
        name: name.to_string(),
        type_params: vec![],
        type_param_bounds: BTreeMap::new(),
        receiver: Some(ReceiverKind::Borrow),
        params: vec![],
        return_passing: ReceiverKind::Value,
        return_borrow_source: None,
        return_type: dummy_type("None"),
        body: vec![],
        span: Span::new(1, 1),
    }
}

#[test]
fn item_name_matches_decl_name() {
    let class_item = Item::Class(ClassDecl {
        public: true,
        copy: false,
        name: "Point".to_string(),
        type_params: vec![],
        type_param_bounds: BTreeMap::new(),
        fields: vec![FieldDecl {
            public: true,
            name: "x".to_string(),
            ty: dummy_type("int32"),
            default: None,
            span: Span::new(1, 1),
        }],
        methods: vec![],
        span: Span::new(1, 1),
    });
    let enum_item = Item::Enum(EnumDecl {
        public: true,
        name: "Status".to_string(),
        type_params: vec![],
        type_param_bounds: BTreeMap::new(),
        variants: vec![],
        span: Span::new(1, 1),
    });
    let function_item = Item::Function(dummy_function("main"));
    let trait_item = Item::Trait(TraitDecl {
        public: true,
        name: "Display".to_string(),
        type_params: vec![],
        supertraits: vec![],
        methods: vec![dummy_function("show")],
        span: Span::new(1, 1),
    });
    let impl_item = Item::Impl(ImplDecl {
        type_params: vec![],
        type_param_bounds: BTreeMap::new(),
        trait_name: "Display".to_string(),
        trait_args: vec![dummy_type("String")],
        for_type: dummy_type("Point"),
        methods: vec![dummy_function("show")],
        span: Span::new(1, 1),
    });

    assert_eq!(class_item.name(), "Point");
    assert_eq!(enum_item.name(), "Status");
    assert_eq!(function_item.name(), "main");
    assert_eq!(trait_item.name(), "Display");
    assert_eq!(impl_item.name(), "Display");
}

#[test]
fn dummy_helpers_cover_receiver_and_none_defaults() {
    let ty = dummy_type("String");
    assert!(matches!(ty.named_parts(), Some(("String", args)) if args.is_empty()));
    assert!(!ty.indirect);
    assert_eq!(ty.span, Span::new(1, 1));

    let function = dummy_function("render");
    assert_eq!(function.name, "render");
    assert_eq!(function.receiver, Some(ReceiverKind::Borrow));
    assert!(matches!(
        function.return_type.named_parts(),
        Some(("None", args)) if args.is_empty()
    ));
    assert!(function.type_params.is_empty());
    assert!(function.type_param_bounds.is_empty());
    assert!(function.params.is_empty());
    assert!(function.body.is_empty());
    assert_eq!(function.span, Span::new(1, 1));
}

#[test]
fn tuple_ast_nodes_are_structural_and_keep_binding_spans() {
    let span = Span::new(3, 5);
    let tuple_ty = TypeRef {
        kind: super::TypeRefKind::Tuple(vec![
            TypeRef::named("int32", vec![], false, span),
            TypeRef::named("String", vec![], false, Span::new(3, 12)),
        ]),
        indirect: false,
        span,
    };
    assert!(matches!(
        tuple_ty.kind,
        super::TypeRefKind::Tuple(ref elements) if elements.len() == 2
    ));
    assert!(tuple_ty.named_parts().is_none());
    assert_eq!(tuple_ty.elements().map(<[TypeRef]>::len), Some(2));

    let target = super::BindingTarget::Tuple {
        elements: vec![
            super::BindingTarget::Name {
                name: "left".to_string(),
                span,
            },
            super::BindingTarget::Tuple {
                elements: vec![super::BindingTarget::Name {
                    name: "right".to_string(),
                    span: Span::new(3, 15),
                }],
                span: Span::new(3, 14),
            },
        ],
        span,
    };
    assert_eq!(target.span(), span);
    assert!(target.name().is_none());

    let name_target = super::BindingTarget::Name {
        name: "value".to_string(),
        span,
    };
    assert_eq!(name_target.span(), span);
    assert_eq!(name_target.name(), Some("value"));

    let named_ty = TypeRef::named("int32", vec![], false, span);
    assert_eq!(named_ty.named_parts(), Some(("int32", &[][..])));
    assert!(named_ty.elements().is_none());
}

#[test]
fn type_ref_json_preserves_named_shape_and_exposes_tuple_elements() {
    let span = Span::new(3, 5);
    let named = TypeRef::named(
        "Option",
        vec![TypeRef::named("int32", vec![], false, span)],
        false,
        span,
    );
    assert_eq!(
        serde_json::to_value(&named).expect("named type reference should serialize"),
        serde_json::json!({
            "name": "Option",
            "args": [{
                "name": "int32",
                "args": [],
                "indirect": false,
                "span": {"line": 3, "column": 5}
            }],
            "indirect": false,
            "span": {"line": 3, "column": 5}
        })
    );

    let tuple = TypeRef::tuple(
        vec![TypeRef::named("String", vec![], false, span)],
        false,
        span,
    );
    assert_eq!(
        serde_json::to_value(&tuple).expect("tuple type reference should serialize"),
        serde_json::json!({
            "elements": [{
                "name": "String",
                "args": [],
                "indirect": false,
                "span": {"line": 3, "column": 5}
            }],
            "indirect": false,
            "span": {"line": 3, "column": 5}
        })
    );
}

#[test]
fn for_stmt_json_preserves_simple_binding_shape_and_exposes_tuple_targets() {
    let span = Span::new(4, 5);
    let iterable = Expr {
        kind: ExprKind::Name("rows".to_string()),
        span: Span::new(4, 24),
    };
    let simple = ForStmt {
        target: BindingTarget::Name {
            name: "row".to_string(),
            span,
        },
        iterable: iterable.clone(),
        borrow_mode: None,
        body: vec![],
        span,
    };
    let simple_json = serde_json::to_value(&simple).expect("simple for statement should serialize");
    assert_eq!(simple_json.get("binding"), Some(&serde_json::json!("row")));
    assert!(simple_json.get("target").is_none());

    let tuple = ForStmt {
        target: BindingTarget::Tuple {
            elements: vec![
                BindingTarget::Name {
                    name: "left".to_string(),
                    span,
                },
                BindingTarget::Name {
                    name: "right".to_string(),
                    span: Span::new(4, 11),
                },
            ],
            span,
        },
        iterable,
        borrow_mode: None,
        body: vec![],
        span,
    };
    let tuple_json =
        serde_json::to_value(&tuple).expect("tuple-target for statement should serialize");
    assert!(tuple_json.get("binding").is_none());
    assert!(matches!(
        tuple_json.get("target"),
        Some(serde_json::Value::Object(target)) if target.contains_key("Tuple")
    ));
}
