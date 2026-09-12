//! Public semantic contracts pinned before the Batch 1 module extraction.

use aura_compiler::ast::ReceiverKind;
use aura_compiler::sema::{FunctionParamContract, Type};

#[test]
fn exported_type_serialization_preserves_names_modes_defaults_and_nested_types() {
    let ty = Type::Function {
        params: vec![FunctionParamContract {
            keyword_only: false,
            name: "input".into(),
            ty: Type::Named("Option".into(), vec![Type::TypeParam("T".into())]),
            passing: ReceiverKind::Value,
            has_default: false,
        }],
        return_type: Box::new(Type::Tuple(vec![Type::named("int32"), Type::Unit])),
    };
    let expected = concat!(
        "{\"Function\":{\"params\":[{\"keyword_only\":false,\"name\":\"input\",",
        "\"ty\":{\"Named\":[\"Option\",[{\"TypeParam\":\"T\"}]]},",
        "\"passing\":\"Value\",\"has_default\":false}],",
        "\"return_type\":{\"Tuple\":[{\"Named\":[\"int32\",[]]},\"Unit\"]}}}"
    );
    assert_eq!(serde_json::to_string(&ty).unwrap(), expected);
    let decoded: Type = serde_json::from_str(expected).unwrap();
    assert_eq!(decoded, ty);
    assert_eq!(serde_json::to_string(&decoded).unwrap(), expected);
    assert!(decoded.is_copy(), "thin function storage remains Copy");
}

#[test]
fn checked_program_preserves_generic_ownership_and_default_declarations() {
    let program = aura_compiler::check_source(
        "public enum Envelope[T]:\n    Item(value: T)\n\n\
         public def identity[T](value: own T) -> T:\n    return value\n\n\
         public def defaulted(value: int32 = 7) -> int32:\n    return value\n\n\
         def main():\n    print(defaulted())\n",
    )
    .unwrap();
    let identity = &program.functions["identity"];
    assert_eq!(identity.signature.params, vec![Type::TypeParam("T".into())]);
    assert_eq!(identity.signature.param_passings, vec![ReceiverKind::Value]);
    assert_eq!(identity.signature.return_type, Type::TypeParam("T".into()));
    let defaulted = &program.functions["defaulted"];
    assert_eq!(
        defaulted.signature.param_passings,
        vec![ReceiverKind::Borrow]
    );
    assert!(defaulted.decl.params[0].default.is_some());
    assert_eq!(defaulted.signature.return_type, Type::named("int32"));
    assert_eq!(
        program.enums["Envelope"].variants["Item"].payloads[0].ty,
        Type::TypeParam("T".into())
    );
    assert!(program.closures.is_empty());
    assert!(program.canonical_type_names.contains_key("Envelope"));
}
