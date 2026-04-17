// Keep this conservative. The parser still uses real Rust recursion for several
// nested constructs, so raising this much further can hit the host stack before
// Aurora reports a diagnostic.
pub const RECURSION_LIMIT: usize = 128;
