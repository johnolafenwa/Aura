use aura_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION;

#[test]
fn the_current_language_surface_has_a_compiler_owned_semantic_interface_schema() {
    assert_eq!(
        SEMANTIC_INTERFACE_SCHEMA_VERSION, 18,
        "class initializers change what a class call and a method named `__init__` mean, so schema 18 is required across compiler services and native cache keys"
    );
}
