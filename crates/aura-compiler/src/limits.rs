// Keep this conservative. The parser still uses real Rust recursion for several
// nested constructs, so raising this much further can hit the host stack before
// Aura reports a diagnostic.
pub const RECURSION_LIMIT: usize = 128;

/// The largest docstring, in UTF-8 bytes after escape processing.
pub const MAX_DOCSTRING_BYTES: usize = 65_536;

/// The largest total of one module's public documentation, in UTF-8 bytes.
pub const MAX_MODULE_DOCUMENTATION_BYTES: usize = 4_194_304;
