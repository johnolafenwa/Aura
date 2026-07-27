use aurora_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION;

#[test]
fn capability_migration_has_a_compiler_owned_semantic_interface_schema() {
    assert_eq!(
        SEMANTIC_INTERFACE_SCHEMA_VERSION, 2,
        "ADR-0022 must use a new schema identity rather than the pre-migration v1 interface"
    );
}
