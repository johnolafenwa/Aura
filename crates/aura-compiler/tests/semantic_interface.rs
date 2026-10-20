use aura_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION;

#[test]
fn the_current_language_surface_has_a_compiler_owned_semantic_interface_schema() {
    assert_eq!(
        SEMANTIC_INTERFACE_SCHEMA_VERSION, 20,
        "decorators rewrite a decorated function into a private function and a module constant, so schema 20 is required across compiler services and native cache keys"
    );
}
