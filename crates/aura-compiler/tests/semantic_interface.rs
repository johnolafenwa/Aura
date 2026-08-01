use aura_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION;

#[test]
fn function_values_have_a_compiler_owned_semantic_interface_schema() {
    assert_eq!(
        SEMANTIC_INTERFACE_SCHEMA_VERSION, 3,
        "function types and MIR function operands require a new schema identity rather than the pre-function-value v2 interface"
    );
}
