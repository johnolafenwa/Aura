use aura_compiler::SEMANTIC_INTERFACE_SCHEMA_VERSION;

#[test]
fn the_current_language_surface_has_a_compiler_owned_semantic_interface_schema() {
    assert_eq!(
        SEMANTIC_INTERFACE_SCHEMA_VERSION, 19,
        "context managers change what `with` means for a class with `__enter__` or `__exit__` and add typed cleanup actions to MIR, so schema 19 is required across compiler services and native cache keys"
    );
}
