use aura_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION;

#[test]
fn the_current_language_surface_has_a_compiler_owned_semantic_interface_schema() {
    assert_eq!(
        SEMANTIC_INTERFACE_SCHEMA_VERSION, 16,
        "removing the builtin Option surface changes builtin signatures and enum shapes, so schema 16 is required across compiler services and native cache keys"
    );
}
