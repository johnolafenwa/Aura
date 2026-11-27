//! Attach the runtime representation to compiler-emitted immutable data.
//! The program stores each encoded value in a read-only object section.

use super::*;
use crate::frozen_value::FrozenValue;
use serde::Deserialize;
use std::sync::atomic::AtomicUsize;

/// Returns the one frozen box for this binding. The first reader decodes
/// its payload. Concurrent readers publish through the same atomic cell.
///
/// # Safety
///
/// The compiler supplies a program-lifetime cell, a serialized FrozenValue
/// and a canonical type descriptor. Both pointers are valid for their lengths.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_frozen_constant(
    cell: &'static AtomicUsize,
    data: *const u8,
    len: usize,
    name: *const u8,
    name_len: usize,
) -> *mut OpaqueValue {
    frozen_program_value(cell, || {
        // SAFETY: generated code supplies these read-only objects.
        let data = unsafe { slice::from_raw_parts(data, len) };
        let name = unsafe { slice::from_raw_parts(name, name_len) };
        // Typed, compiler-emitted data already satisfies Aura's nesting limits.
        let mut input = serde_json::Deserializer::from_slice(data);
        input.disable_recursion_limit();
        let frozen = FrozenValue::deserialize(&mut input)
            .unwrap_or_else(|error| runtime_error(format!("invalid compiled constant: {error}")));
        let tag = interned_runtime_type_tag(name);
        let boxed = boxed_value(frozen.thaw());
        // SAFETY: this thread exclusively owns the newly allocated box.
        unsafe { set_explicit_runtime_type_tag(boxed, &tag) };
        boxed
    })
}
