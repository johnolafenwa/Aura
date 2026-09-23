use aura_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION;

#[test]
fn the_current_language_surface_has_a_compiler_owned_semantic_interface_schema() {
    assert_eq!(
        SEMANTIC_INTERFACE_SCHEMA_VERSION, 17,
        "returned element and entry views add an element step to returned-view contracts, so schema 17 is required across compiler services and native cache keys"
    );
}
