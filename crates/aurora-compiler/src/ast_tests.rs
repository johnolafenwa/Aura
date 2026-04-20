use super::{
    ClassDecl, EnumDecl, FieldDecl, FunctionDecl, ImplDecl, Item, ReceiverKind, TraitDecl, TypeRef,
};
use crate::diag::Span;
use std::collections::BTreeMap;

fn dummy_type(name: &str) -> TypeRef {
    TypeRef {
        name: name.to_string(),
        args: vec![],
        indirect: false,
        span: Span::new(1, 1),
    }
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
    assert_eq!(ty.name, "String");
    assert!(ty.args.is_empty());
    assert!(!ty.indirect);
    assert_eq!(ty.span, Span::new(1, 1));

    let function = dummy_function("render");
    assert_eq!(function.name, "render");
    assert_eq!(function.receiver, Some(ReceiverKind::Borrow));
    assert_eq!(function.return_type.name, "None");
    assert!(function.type_params.is_empty());
    assert!(function.type_param_bounds.is_empty());
    assert!(function.params.is_empty());
    assert!(function.body.is_empty());
    assert_eq!(function.span, Span::new(1, 1));
}
