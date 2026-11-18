//! Native calls on a concrete int64 collection. Other methods still go through
//! its standard module, so these calls edit the very same class field.

use super::*;
use crate::runtime_value::ordered_collections::{I64Op, Store};
use crate::runtime_value::InstanceFields;

fn invalid_store() -> Diagnostic {
    Diagnostic::coded("AU4001", "invalid native ordered collection receiver")
}

fn field_mut<R>(
    value: &mut Value,
    name: &str,
    action: impl FnOnce(&mut Value) -> Result<R, Diagnostic>,
) -> Result<R, Diagnostic> {
    let Value::Instance(instance) = value else {
        return Err(invalid_store());
    };
    match &mut instance.fields {
        InstanceFields::Map(fields) => action(fields.get_mut(name).ok_or_else(invalid_store)?),
        InstanceFields::Typed(fields) => fields
            .with_field_mut(name, action)
            .map_err(|_| invalid_store())?,
    }
}

fn read(value: &Value, op: I64Op, key: i64, nested: bool) -> Result<Option<i64>, Diagnostic> {
    let Value::Instance(instance) = value else {
        return Err(invalid_store());
    };
    if nested {
        let map = instance.fields.get("map").ok_or_else(invalid_store)?;
        return read(&map, op, key, false);
    }
    let field = instance.fields.get("store").ok_or_else(invalid_store)?;
    let Value::Vec(list) = field.as_ref() else {
        return Err(invalid_store());
    };
    match list.elements.get(0).as_deref() {
        Some(Value::CollectionStore(store)) => store.read_i64(op, key),
        None => Ok(None),
        _ => Err(invalid_store()),
    }
}

fn edit_store<R>(
    value: &mut Value,
    op: I64Op,
    nested: bool,
    edit: impl FnOnce(&mut Store) -> Result<Option<R>, Diagnostic>,
) -> Result<Option<R>, Diagnostic> {
    if nested {
        return field_mut(value, "map", |map| edit_store(map, op, false, edit));
    }
    let descending = if matches!(op, I64Op::HeapPush | I64Op::HeapPop) {
        let Value::Instance(instance) = value else {
            return Err(invalid_store());
        };
        matches!(
            instance.fields.get("descending").as_deref(),
            Some(Value::Bool(true))
        )
    } else {
        false
    };
    field_mut(value, "store", |field| {
        let Value::Vec(list) = field else {
            return Err(invalid_store());
        };
        if list.elements.is_empty() {
            if op.returns_number() {
                let message = if op == I64Op::HeapPop {
                    "list swap indices `0` and `-1` are out of bounds for length `0`"
                } else {
                    "list pop index `-1` is out of bounds for length `0`"
                };
                return Err(Diagnostic::coded("AU4003", message));
            }
            if matches!(op, I64Op::MapRemove | I64Op::SetRemove) {
                return Ok(None);
            }
            list.elements
                .push(Value::CollectionStore(op.new_store(descending)));
        }
        let mut slot = list.elements.get_mut(0).ok_or_else(invalid_store)?;
        let Value::CollectionStore(store) = &mut *slot else {
            return Err(invalid_store());
        };
        edit(store)
    })
}

#[cold]
fn fail_operation(mut error: Diagnostic, op: I64Op, word: i64) -> ! {
    if let Some((method, span)) = op.pop_frame(word) {
        // The native operation replaces this standard method. Keep the
        // caller and task ancestry that normal runtime errors capture.
        capture_direct_runtime_frames_once(&mut error);
        let path = direct_program_path_frame_text().map(|path| path.materialize());
        error.call_frames.insert(
            0,
            RuntimeCallFrame {
                function: method.to_string(),
                span: RuntimeSourceSpan::point(path, span),
            },
        );
    }
    runtime_diagnostic_error(error)
}

fn operation(receiver: *mut OpaqueValue, code: i64, key: i64, payload: i64) -> Option<i64> {
    let op = I64Op::from_code(code).unwrap_or_else(|| runtime_diagnostic_error(invalid_store()));
    let result = if op.is_read() {
        // The caller keeps a live receiver for the duration of the read.
        unsafe { with_value(receiver, |value| read(value, op, key, op.is_set())) }
    } else {
        // The emitted instruction borrows the local receiver for this call.
        unsafe {
            value_mut(receiver, |value| {
                edit_store(value, op, op.is_set(), |store| {
                    store.write_i64(op, key, payload)
                })
            })
        }
    };
    result.unwrap_or_else(|error| fail_operation(error, op, key))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aura_direct_ordered_i64(
    receiver: *mut OpaqueValue,
    code: i64,
    key: i64,
    payload: i64,
) -> i64 {
    task_runtime_boundary(|| operation(receiver, code, key, payload).unwrap_or(0))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aura_direct_ordered_i64_lookup(
    receiver: *mut OpaqueValue,
    code: i64,
    key: i64,
    payload: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let value = operation(receiver, code, key, payload);
        boxed_value(value.map_or_else(lookup_missing, |v| {
            lookup_found(Value::Int(IntegerValue::from_i64(v)))
        }))
    })
}

/// Opens the packed storage of a fresh receiver that MIR proved is used only
/// by local deque or heap operations and release. A shared receiver declines
/// the fast path. Its ordinary methods continue to use the guarded entry.
#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aura_direct_ordered_local_words(
    receiver: *mut OpaqueValue,
    code: i64,
) -> *mut std::ffi::c_void {
    task_runtime_boundary(|| {
        let op = I64Op::from_code(code)
            .filter(|op| op.is_word_sequence())
            .unwrap_or_else(|| runtime_diagnostic_error(invalid_store()));
        // Generated code borrows its live receiver for this call.
        let Some(header) = (unsafe { receiver.as_ref() }) else {
            return std::ptr::null_mut();
        };
        if header.shared_lock().is_some() {
            return std::ptr::null_mut();
        }
        // The exclusive edit detaches both the field's element storage and
        // the collection snapshot before publishing its address.
        let result = unsafe {
            value_mut(receiver, |value| {
                edit_store(value, op, false, |store| Ok(store.local_words()))
            })
        };
        // The guarded fallback has the source position of the operation
        // and reports the full Aura frame if opening an empty pop failed.
        result.ok().flatten().unwrap_or(std::ptr::null_mut())
    })
}

/// Applies one operation to a collection held exclusively by generated code.
///
/// # Safety
/// `data` must be a non-null address obtained from
/// [`aura_direct_ordered_local_words`]. Its receiver must remain alive and
/// confined, and no operation may replace, clone or access its storage other
/// than calls to this function until the last use of the address.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ordered_local_step(
    data: *mut std::ffi::c_void,
    code: i64,
    word: i64,
) -> i64 {
    // SAFETY: the emitted code maintains the requirements above.
    if let Some(result) = unsafe { Store::local_step(data, code, word) } {
        return result;
    }
    task_runtime_boundary(|| {
        let op =
            I64Op::from_code(code).unwrap_or_else(|| runtime_diagnostic_error(invalid_store()));
        let diagnostic = match op {
            I64Op::PopFront | I64Op::PopBack => {
                Diagnostic::coded("AU4003", "cannot pop from an empty deque")
            }
            I64Op::HeapPop => Diagnostic::coded("AU4003", "cannot pop from an empty heap"),
            _ => invalid_store(),
        };
        fail_operation(diagnostic, op, word)
    })
}

#[cfg(test)]
#[path = "ordered_collections_native_tests.rs"]
mod tests;
