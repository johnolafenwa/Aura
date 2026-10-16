//! Names of synthetic MIR place segments the runtime also decodes.

/// Prefix of the place segment that projects an enum variant's payload.
pub const ENUM_PAYLOAD_PROJECTION_PREFIX: &str = "__variant_payload_";

/// Decode `__variant_payload_<Variant>_<index>` into the variant and index.
pub fn enum_payload_projection(segment: &str) -> Option<(&str, usize)> {
    let payload = segment.strip_prefix(ENUM_PAYLOAD_PROJECTION_PREFIX)?;
    let (variant, index) = payload.rsplit_once('_')?;
    if variant.is_empty() || index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = index.parse::<usize>().ok()?;
    (parsed.to_string() == index).then_some((variant, parsed))
}
