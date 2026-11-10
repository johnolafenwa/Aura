//! What generated code calls around a direct foreign function call.
//!
//! Generated code calls a C function itself: it passes scalars in the C
//! types the declaration names and reads the result the same way. The
//! helpers here cover the rest of the boundary. They find the bytes a `str`
//! or `list[uint8]` argument lends for the call, read the address an opaque
//! handle carries, box a returned address as a handle, and report a result
//! the declaration rules out.
//!
//! A view lends the value's own storage, so the foreign function reads the
//! bytes in place. The value's caller keeps it alive and unchanged for the
//! whole call: a shared argument has no writer, and a `mut` argument has no
//! other reader.

use std::ffi::c_void;
use std::slice;
use std::str;

use aura_core::diag::Diagnostic;

use super::{
    boxed_value, runtime_diagnostic_error, runtime_error, task_runtime_boundary, value_mut,
    value_type_name, with_value, OpaqueValue,
};
use crate::runtime_value::{FfiHandleValue, Value};

/// A pointer-length view as generated code reads it back: the address of
/// the first byte, or null when there are none, and the byte count.
#[repr(C)]
pub struct ForeignView {
    pub pointer: *const u8,
    pub length: usize,
}

/// # Safety
///
/// `out` is null or points to a writable [`ForeignView`].
unsafe fn write_view(out: *mut ForeignView, bytes: &[u8]) {
    let Some(out) = (unsafe { out.as_mut() }) else {
        runtime_error("a foreign call view has no place to go");
    };
    out.pointer = if bytes.is_empty() {
        std::ptr::null()
    } else {
        bytes.as_ptr()
    };
    out.length = bytes.len();
}

/// # Safety
///
/// `pointer` and `length` describe UTF-8 text generated code holds in its
/// data.
unsafe fn metadata_text<'a>(pointer: *const u8, length: usize) -> &'a str {
    if pointer.is_null() || length == 0 {
        return "";
    }
    // SAFETY: the caller's contract. The emitter writes these names from
    // `String`s, so they are UTF-8.
    unsafe { str::from_utf8_unchecked(slice::from_raw_parts(pointer, length)) }
}

fn foreign_call_failure(symbol: &str, code: &'static str, detail: impl std::fmt::Display) -> ! {
    runtime_diagnostic_error(Diagnostic::coded(
        code,
        format!("FFI call to `{symbol}` failed: {detail}"),
    ))
}

/// Writes the UTF-8 bytes of the `str` `value` to `out`.
///
/// # Safety
///
/// `value` is a live value and `out` points to a writable [`ForeignView`].
/// The view is valid while `value` is alive and unchanged.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_str_view(
    value: *mut OpaqueValue,
    out: *mut ForeignView,
) {
    task_runtime_boundary(|| unsafe {
        with_value(value, |value| match value {
            Value::String(text) => write_view(out, text.as_bytes()),
            other => runtime_error(format!(
                "FFI argument expected str view, but received `{}`",
                value_type_name(other)
            )),
        })
    })
}

/// Writes the bytes of the `list[uint8]` `value` to `out`. A list that
/// keeps its bytes unboxed lends them and the result is null. Any other
/// list is copied, and the result is the copy, which the caller frees with
/// [`aura_direct_ffi_release_bytes`] once the foreign call returns.
///
/// # Safety
///
/// `value` is a live value and `out` points to a writable [`ForeignView`].
/// A lent view is valid while `value` is alive and unchanged.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_bytes_view(
    value: *mut OpaqueValue,
    out: *mut ForeignView,
) -> *mut u8 {
    task_runtime_boundary(|| unsafe {
        with_value(value, |value| {
            let Value::Vec(vector) = value else {
                runtime_error(format!(
                    "FFI argument expected list[uint8] view, but received `{}`",
                    value_type_name(value)
                ));
            };
            if let Some(bytes) = vector.elements.as_uint8() {
                write_view(out, bytes);
                return std::ptr::null_mut();
            }
            let Some(bytes) = vector.elements.to_uint8() else {
                runtime_error(
                    "FFI argument expected list[uint8] view, but an element is not a uint8",
                );
            };
            if bytes.is_empty() {
                write_view(out, &[]);
                return std::ptr::null_mut();
            }
            let copy = Box::leak(bytes.into_boxed_slice());
            write_view(out, copy);
            copy.as_mut_ptr()
        })
    })
}

/// Frees a copy [`aura_direct_ffi_bytes_view`] returned.
///
/// # Safety
///
/// `copy` and `length` are one non-null result of that function and the
/// length it wrote, not freed before.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_release_bytes(copy: *mut u8, length: usize) {
    if copy.is_null() {
        return;
    }
    // SAFETY: the caller's contract names a boxed slice of this length.
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(copy, length)) });
}

/// Writes the bytes of the `list[uint8]` `value` to `out` for a foreign
/// function that may change them in place. The list keeps its length.
///
/// # Safety
///
/// `value` is a live value its caller holds exclusively, and `out` points to
/// a writable [`ForeignView`]. The view is valid until the caller next
/// reaches the list.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_bytes_view_mut(
    value: *mut OpaqueValue,
    out: *mut ForeignView,
) {
    task_runtime_boundary(|| unsafe {
        value_mut(value, |value| {
            let Value::Vec(vector) = value else {
                runtime_error(format!(
                    "FFI argument expected mut list[uint8] view, but received `{}`",
                    value_type_name(value)
                ));
            };
            match vector.elements.uint8_mut() {
                Some(bytes) => write_view(out, bytes),
                None => runtime_error(
                    "FFI argument expected mut list[uint8] view, but an element is not a uint8",
                ),
            }
        })
    })
}

/// The address the opaque handle `value` carries.
///
/// # Safety
///
/// `value` is a live value.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_handle_address(
    value: *mut OpaqueValue,
) -> *mut c_void {
    task_runtime_boundary(|| unsafe {
        with_value(value, |value| match value {
            Value::FfiHandle(handle) => handle.as_ptr(),
            other => runtime_error(format!(
                "FFI argument expected opaque handle, but received `{}`",
                value_type_name(other)
            )),
        })
    })
}

/// A new handle of the opaque type the name spells for the address a
/// foreign function returned, or null for a null address.
///
/// # Safety
///
/// The name is UTF-8 text in generated code's data.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_handle(
    address: *mut c_void,
    type_name: *const u8,
    type_name_length: usize,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        // SAFETY: the caller's contract.
        let type_name = unsafe { metadata_text(type_name, type_name_length) };
        match FfiHandleValue::new(type_name.to_string(), address) {
            Some(handle) => boxed_value(Value::FfiHandle(handle)),
            None => std::ptr::null_mut(),
        }
    })
}

/// A new `None` value, the payload of a nullable handle result whose
/// address was null.
#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aura_direct_ffi_none() -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(Value::Unit))
}

/// Ends the task with `AU4005`: the foreign function `symbol` returned a
/// null address where its declaration promises a handle.
///
/// # Safety
///
/// The symbol name is UTF-8 text in generated code's data.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_null_handle(
    symbol: *const u8,
    symbol_length: usize,
) {
    task_runtime_boundary(|| {
        // SAFETY: the caller's contract.
        let symbol = unsafe { metadata_text(symbol, symbol_length) };
        foreign_call_failure(
            symbol,
            "AU4005",
            crate::ffi::FfiError::NullOpaqueHandleReturn,
        )
    })
}

/// Ends the task with `AU4001`: the foreign function `symbol` returned
/// `byte` where its declaration promises a `bool`, which is 0 or 1.
///
/// # Safety
///
/// The symbol name is UTF-8 text in generated code's data.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_bad_bool(
    byte: i64,
    symbol: *const u8,
    symbol_length: usize,
) {
    task_runtime_boundary(|| {
        // SAFETY: the caller's contract.
        let symbol = unsafe { metadata_text(symbol, symbol_length) };
        foreign_call_failure(
            symbol,
            "AU4001",
            crate::ffi::FfiError::NonCanonicalBoolReturn(byte as u8),
        )
    })
}

#[cfg(test)]
#[path = "native_foreign_calls_tests.rs"]
mod tests;
