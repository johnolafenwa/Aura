//! What generated code calls around a direct foreign function call.
//!
//! Generated code calls a C function itself: it passes scalars in the C
//! types the declaration names and reads the result the same way. The
//! helpers here cover the rest of the boundary. They find the bytes a `str`
//! or `list[uint8]` argument lends for the call, read the address an opaque
//! handle carries, box a returned address as a handle, copy C text in and
//! out, and report a result the declaration rules out.
//!
//! A view lends the value's own storage, so the foreign function reads the
//! bytes in place. The value's caller keeps it alive and unchanged for the
//! whole call: a shared argument has no writer, and a `mut` argument has no
//! other reader.

use std::ffi::{c_char, c_void, CStr, CString};
use std::slice;
use std::str;

use aura_core::diag::Diagnostic;

use super::{
    allocate_box, boxed_value, register_direct_owned_value, release_untracked_value,
    runtime_diagnostic_error, runtime_error, task_runtime_boundary, value_mut, value_type_name,
    with_value, OpaqueValue,
};
use crate::c_layout::CStruct;
use crate::c_values::{self, CValueError};
use crate::list_elements::{ListElements, ListForm};
use crate::runtime_value::{
    ArrayValue, FfiHandleDrop, FfiHandleValue, InstanceFields, TypedFields, Value, VecValue,
};

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
            Value::FfiHandle(handle) => {
                let pointer = handle.as_ptr();
                if pointer.is_null() {
                    runtime_error(format!(
                        "FFI opaque handle `{}` has already been released",
                        handle.type_name()
                    ));
                }
                pointer
            }
            other => runtime_error(format!(
                "FFI argument expected opaque handle, but received `{}`",
                value_type_name(other)
            )),
        })
    })
}

/// The scalar list form generated code names by its list access code.
fn scalar_form(access: i64) -> ListForm {
    match access {
        2 => ListForm::Int64,
        3 => ListForm::Float64,
        4 => ListForm::Bool,
        6 => ListForm::Int8,
        7 => ListForm::Int16,
        8 => ListForm::Int32,
        9 => ListForm::Uint8,
        10 => ListForm::Uint16,
        11 => ListForm::Uint32,
        12 => ListForm::Uint64,
        13 => ListForm::Float32,
        other => runtime_error(format!("list access code {other} names no scalar form")),
    }
}

/// # Safety
///
/// `out` is null or points to a writable [`ForeignView`].
unsafe fn write_elements(out: *mut ForeignView, pointer: *const u8, length: usize) {
    let Some(out) = (unsafe { out.as_mut() }) else {
        runtime_error("a foreign call view has no place to go");
    };
    out.pointer = if length == 0 {
        std::ptr::null()
    } else {
        pointer
    };
    out.length = length;
}

/// The address of the first element of the row-major `array`, whose
/// dtype must be the scalar the list access code `access` names.
fn array_elements_address(array: &ArrayValue, access: i64) -> *const u8 {
    use crate::runtime_value::ArrayStorage;
    match (array.storage(), scalar_form(access)) {
        (ArrayStorage::Int32(values), ListForm::Int32) => values.as_ptr().cast(),
        (ArrayStorage::Int64(values), ListForm::Int64) => values.as_ptr().cast(),
        (ArrayStorage::Float32(values), ListForm::Float32) => values.as_ptr().cast(),
        (ArrayStorage::Float64(values), ListForm::Float64) => values.as_ptr().cast(),
        _ => runtime_error(format!(
            "FFI argument expected an Array of another element type, but received `Array[{}]`",
            array.dtype().runtime_type_name()
        )),
    }
}

/// Writes the address of the first element of the list `value` to `out`,
/// laid out as a C array of the scalar that `access` names, with the
/// element count. A list that keeps its elements in that form lends them
/// and the result is null. Any other list is copied into a new list in that
/// form, and the result is the copy, which the caller releases as a value
/// once the foreign call returns.
///
/// # Safety
///
/// `value` is a live value and `out` points to a writable [`ForeignView`].
/// Lent elements are valid while `value` is alive and unchanged.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_elements(
    value: *mut OpaqueValue,
    access: i64,
    out: *mut ForeignView,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| unsafe {
        // An `Array[T]` lends its own elements when they are one
        // row-major block, and a packed copy of a view otherwise.
        let packed = with_value(value, |value| {
            let Value::Array(array) = value else {
                return None;
            };
            Some(if array.is_contiguous() {
                write_elements(out, array_elements_address(array, access), array.len());
                std::ptr::null_mut()
            } else {
                let Ok(elements) = array.elements() else {
                    runtime_error("FFI argument could not copy an Array view");
                };
                let Ok(copy) = ArrayValue::new(array.shape.clone(), elements.into_owned()) else {
                    runtime_error("FFI argument could not copy an Array view");
                };
                write_elements(out, array_elements_address(&copy, access), copy.len());
                boxed_value(Value::Array(copy))
            })
        });
        if let Some(copy) = packed {
            return copy;
        }
        let form = scalar_form(access);
        with_value(value, |value| {
            let Value::Vec(vector) = value else {
                runtime_error(format!(
                    "FFI argument expected a list for a pointer, but received `{}`",
                    value_type_name(value)
                ));
            };
            let length = vector.elements.len();
            if let Some(pointer) = vector.elements.scalars(form) {
                write_elements(out, pointer, length);
                return std::ptr::null_mut();
            }
            let Some(copy) = vector.elements.in_scalar_form(form) else {
                runtime_error(
                    "FFI argument expected a list for a pointer, but an element is another type",
                );
            };
            if length == 0 {
                write_elements(out, std::ptr::null(), 0);
                return std::ptr::null_mut();
            }
            // The elements are on the heap, so they stay where they are
            // when the list moves into its box.
            write_elements(out, copy.as_ptr() as *const u8, length);
            boxed_value(Value::Vec(VecValue {
                element_type: vector.element_type.clone(),
                elements: copy,
                str_view: Default::default(),
            }))
        })
    })
}

/// Writes the address of the first element of the list `value` to `out`
/// for a foreign function that may change the elements in place, laid out
/// as a C array of the scalar that `access` names. The list keeps its
/// length.
///
/// # Safety
///
/// `value` is a live value its caller holds exclusively, and `out` points to
/// a writable [`ForeignView`]. The elements are valid until the caller next
/// reaches the list.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_elements_mut(
    value: *mut OpaqueValue,
    access: i64,
    out: *mut ForeignView,
) {
    task_runtime_boundary(|| unsafe {
        // An `Array[T]` is made one row-major block that no other Array
        // shares, and then lends its elements.
        let lent = value_mut(value, |value| {
            let Value::Array(array) = value else {
                return false;
            };
            if let Err(error) = array.make_writable() {
                runtime_diagnostic_error(error);
            }
            write_elements(out, array_elements_address(array, access), array.len());
            true
        });
        if lent {
            return;
        }
        let form = scalar_form(access);
        value_mut(value, |value| {
            let Value::Vec(vector) = value else {
                runtime_error(format!(
                    "FFI argument expected a list for a mut pointer, but received `{}`",
                    value_type_name(value)
                ));
            };
            let length = vector.elements.len();
            match vector.elements.scalars_mut(form) {
                Some(pointer) => write_elements(out, pointer, length),
                None => runtime_error(
                    "FFI argument expected a list for a mut pointer, but an element is another type",
                ),
            }
        })
    })
}

/// The value itself, or the active member of a union.
fn active_member(value: &Value) -> &Value {
    match value {
        Value::Union(union) => &union.payload,
        other => other,
    }
}

/// The address the `Handle | None` `value` carries: null for `None`.
///
/// # Safety
///
/// `value` is a live value.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_nullable_handle_address(
    value: *mut OpaqueValue,
) -> *mut c_void {
    task_runtime_boundary(|| unsafe {
        with_value(value, |value| match active_member(value) {
            Value::FfiHandle(handle) => {
                let pointer = handle.as_ptr();
                if pointer.is_null() {
                    runtime_error(format!(
                        "FFI opaque handle `{}` has already been released",
                        handle.type_name()
                    ));
                }
                pointer
            }
            Value::Unit => std::ptr::null_mut(),
            other => runtime_error(format!(
                "FFI argument expected opaque handle or None, but received `{}`",
                value_type_name(other)
            )),
        })
    })
}

/// A NUL-terminated copy of the `str` or `str | None` `value`, argument
/// `index` of a call of `symbol`, or null for `None`. The caller frees the
/// copy with [`aura_direct_ffi_release_c_string`] once the foreign call
/// returns. Text that holds a NUL byte ends the task with `AU4005`.
///
/// # Safety
///
/// `value` is a live value and the symbol name is UTF-8 text in generated
/// code's data.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_c_string(
    value: *mut OpaqueValue,
    index: usize,
    symbol: *const u8,
    symbol_length: usize,
) -> *mut c_char {
    task_runtime_boundary(|| unsafe {
        with_value(value, |value| match active_member(value) {
            Value::String(text) => match CString::new(text.as_bytes()) {
                Ok(copy) => copy.into_raw(),
                Err(_) => foreign_call_failure(
                    metadata_text(symbol, symbol_length),
                    "AU4005",
                    crate::ffi::FfiError::InteriorNul { index },
                ),
            },
            Value::Unit => std::ptr::null_mut(),
            other => runtime_error(format!(
                "FFI argument expected cstr, but received `{}`",
                value_type_name(other)
            )),
        })
    })
}

/// Frees a copy [`aura_direct_ffi_c_string`] returned. Null is no copy.
///
/// # Safety
///
/// `copy` is one result of that function, not freed before.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_release_c_string(copy: *mut c_char) {
    if copy.is_null() {
        return;
    }
    // SAFETY: the caller's contract names a `CString` given up with
    // `into_raw`.
    drop(unsafe { CString::from_raw(copy) });
}

/// A new `str` that copies the NUL-terminated text the foreign function
/// `symbol` returned. The text stays the foreign side's to free. A null
/// pointer gives null when `nullable` is not 0 and ends the task with
/// `AU4005` otherwise, as text that is not UTF-8 does.
///
/// # Safety
///
/// `text` is null or points to NUL-terminated bytes, and the symbol name is
/// UTF-8 text in generated code's data.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_c_string_result(
    text: *const c_char,
    nullable: i64,
    symbol: *const u8,
    symbol_length: usize,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| unsafe {
        let failure = if text.is_null() {
            if nullable != 0 {
                return std::ptr::null_mut();
            }
            crate::ffi::FfiError::NullCStringReturn
        } else {
            match CStr::from_ptr(text).to_str() {
                Ok(text) => return boxed_value(Value::String(text.into())),
                Err(_) => crate::ffi::FfiError::InvalidUtf8Return,
            }
        };
        foreign_call_failure(metadata_text(symbol, symbol_length), "AU4005", failure)
    })
}

/// A new handle of the opaque type the name spells for the address a
/// foreign function returned, or null for a null address. `drop` is the
/// function the type's declaration names to release the object, or `None`.
///
/// # Safety
///
/// The name is UTF-8 text in generated code's data, and `drop` releases an
/// object of the type.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_handle(
    address: *mut c_void,
    type_name: *const u8,
    type_name_length: usize,
    drop: Option<FfiHandleDrop>,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        // SAFETY: the caller's contract.
        let type_name = unsafe { metadata_text(type_name, type_name_length) };
        match FfiHandleValue::owned(type_name.to_string(), address, drop) {
            Some(handle) => boxed_value(Value::FfiHandle(handle)),
            None => std::ptr::null_mut(),
        }
    })
}

/// The handle a foreign function left through a `mut Handle | None`
/// parameter that held `before` when the call began, or null for a null
/// address. An address the function did not change keeps the handle the
/// place held. Any other address is a new handle, and the program no
/// longer owns the object the place held.
///
/// # Safety
///
/// `before` is a live value, the name is UTF-8 text in generated code's
/// data, and `drop` releases an object of the type.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_stored_handle(
    before: *mut OpaqueValue,
    address: *mut c_void,
    type_name: *const u8,
    type_name_length: usize,
    drop: Option<FfiHandleDrop>,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| unsafe {
        let kept = with_value(before, |value| match active_member(value) {
            Value::FfiHandle(held) if held.as_ptr() == address => Some(held.clone()),
            Value::FfiHandle(held) => {
                held.give_up();
                None
            }
            _ => None,
        });
        match kept {
            Some(held) => boxed_value(Value::FfiHandle(held)),
            None => aura_direct_ffi_handle(address, type_name, type_name_length, drop),
        }
    })
}

/// Ends the program's ownership of the object behind the handle `value`,
/// which a foreign function is about to take as `own`.
///
/// # Safety
///
/// `value` is a live value.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_give_up_handle(value: *mut OpaqueValue) {
    task_runtime_boundary(|| unsafe {
        with_value(value, |value| {
            if let Value::FfiHandle(handle) = value {
                if handle.take_ptr().is_null() {
                    runtime_error(format!(
                        "FFI opaque handle `{}` has already been released",
                        handle.type_name()
                    ));
                }
            }
        })
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

/// Sanitizes any non-canonical byte (`> 1`) left in the `list[bool]` `value`
/// to `1` (`true`). Returns the first offending byte (`2..=255`), or `0`
/// when every element was canonical (`0` or `1`) or `value` is not a bool list.
///
/// # Safety
///
/// `value` is a live value its caller holds exclusively.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_sanitize_flags(value: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| unsafe {
        let bad = value_mut(value, |value| {
            let Value::Vec(vector) = value else {
                return None;
            };
            vector.elements.sanitize_bool_bytes()
        });
        bad.map_or(0, i64::from)
    })
}

/// Ends the task with `AU4001` when the foreign function `symbol` left a
/// byte that is neither 0 nor 1 in the `list[bool]` `value` it wrote
/// through a `mut ptr[bool]`. The byte becomes `true` first, so the list
/// holds only values of its type while the task unwinds.
///
/// # Safety
///
/// `value` is a live value its caller holds exclusively, and the symbol
/// name is UTF-8 text in generated code's data.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_check_flags(
    value: *mut OpaqueValue,
    symbol: *const u8,
    symbol_length: usize,
) {
    task_runtime_boundary(|| unsafe {
        let bad = value_mut(value, |value| {
            let Value::Vec(vector) = value else {
                return None;
            };
            vector.elements.sanitize_bool_bytes()
        });
        if let Some(byte) = bad {
            foreign_call_failure(
                metadata_text(symbol, symbol_length),
                "AU4001",
                crate::ffi::FfiError::NonCanonicalBoolReturn(byte),
            )
        }
    })
}

thread_local! {
    /// Most workers never receive a failed C callback. This hint keeps
    /// ordinary foreign calls on those workers from looking up task state.
    static MAY_HAVE_CALLBACK_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs the body of a callback that C code called. `body` is generated
/// code that calls the Aura function with the arguments in `frame` and
/// leaves its results there.
///
/// A failure in the function must not unwind through the C frames between
/// the callback and the foreign call. It stops here and is raised again
/// after the foreign function returns. Until then, every callback returns
/// without running, with a zero result.
///
/// # Safety
///
/// `body` is the generated body for the callback, and `frame` is the frame
/// its entry code built.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C" fn aura_direct_ffi_callback_guard(
    body: unsafe extern "C-unwind" fn(*mut u64),
    frame: *mut u64,
) {
    with_callback_guard(|| unsafe { body(frame) });
}

/// Conservative stack size assumed for a foreign OS thread that invokes an
/// Aura callback without an active Aura task stack bound (fits macOS 512 KiB
/// secondary threads while leaving headroom above `DIRECT_STACK_RESERVE`).
const FOREIGN_THREAD_CALLBACK_STACK_SIZE: usize = 512 * 1024;

fn with_callback_guard(body: impl FnOnce()) {
    let foreign_stack_bounds = crate::runtime_value::current_task_stack_floor()
        .is_none()
        .then(|| {
            crate::runtime_value::enter_thread_stack_bounds(FOREIGN_THREAD_CALLBACK_STACK_SIZE)
        });
    let previous = super::with_direct_task_runtime_state(|state| {
        if state.foreign_callback_failure.is_some() {
            return None;
        }
        if foreign_stack_bounds.is_some() {
            state.refresh_stack_limit();
        }
        if !state.owned_values().tracking() {
            state.owned_values_mut().set_tracking(true);
        }
        Some(
            state
                .foreign_callback_cleanup_base
                .replace(state.cleanup_stack.len()),
        )
    });
    let Some(previous) = previous else {
        drop(foreign_stack_bounds);
        return;
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
    drop(foreign_stack_bounds);
    super::with_direct_task_runtime_state(|state| {
        state.foreign_callback_cleanup_base = previous;
        state.refresh_stack_limit();
        if let Err(payload) = outcome {
            state.foreign_callback_failure = Some(payload);
            MAY_HAVE_CALLBACK_FAILURE.set(true);
        }
    });
}

/// Calls a callback whose uses of its string arguments only inspect them.
/// Each validated string owns its bytes in a Rust stack frame until the
/// callback returns or unwinds. The ordinary Aura release of a parameter
/// leaves that frame in charge of the value.
///
/// # Safety
///
/// As for [`aura_direct_ffi_callback_guard`]. Every set bit names a C
/// string argument and the compiler proved that no such value escapes.
/// The function name is static UTF-8 data.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C" fn aura_direct_ffi_callback_local_text_guard(
    body: unsafe extern "C-unwind" fn(*mut u64),
    frame: *mut u64,
    text_mask: u64,
    function: *const u8,
    function_length: usize,
) {
    unsafe fn call_with_text(
        body: unsafe extern "C-unwind" fn(*mut u64),
        frame: *mut u64,
        mask: std::num::NonZeroU64,
        function: *const u8,
        function_length: usize,
    ) {
        let index = mask.trailing_zeros() as usize;
        // SAFETY: generated code supplied a frame word for every selected argument.
        let slot = unsafe { frame.add(index) };
        let text = match unsafe { crate::ffi::checked_callback_text(*slot as usize) } {
            Ok(text) => text,
            Err(error) => foreign_call_failure(
                unsafe { metadata_text(function, function_length) },
                "AU4005",
                error,
            ),
        };
        let mut local =
            OpaqueValue::with_view(Value::String(text.into()), None, super::EMPTY_VALUE_VIEW);
        *local.ref_count.get_mut() = super::FROZEN_REFERENCES;
        *local.shared_lock.get_mut() = std::ptr::from_ref(&super::FROZEN_VALUE_LOCK).cast_mut();
        // SAFETY: the remaining arguments and callback run inside this frame.
        // The callback only reads this pointer. Both return and unwind drop the value.
        unsafe {
            *slot = std::ptr::from_mut(&mut local) as u64;
            if let Some(remaining) = std::num::NonZeroU64::new(mask.get() & (mask.get() - 1)) {
                call_with_text(body, frame, remaining, function, function_length);
            } else {
                body(frame);
            }
        }
    }

    with_callback_guard(|| unsafe {
        if let Some(mask) = std::num::NonZeroU64::new(text_mask) {
            call_with_text(body, frame, mask, function, function_length)
        } else {
            body(frame)
        }
    });
}

/// A new `str` that holds a copy of the C text a callback received.
/// `function` names the Aura function the callback runs. The ordinary
/// parameter reference belongs to the task. The callback entry holds a
/// second reference and releases it after the guarded body returns or traps.
///
/// # Safety
///
/// `text` is null or points to NUL-terminated bytes, and the function
/// name is UTF-8 text in generated code's data.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_callback_text(
    text: *const c_char,
    function: *const u8,
    function_length: usize,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| unsafe {
        match crate::ffi::checked_callback_text(text as usize) {
            Ok(text) => {
                // Publish both references together. The task releases its
                // argument in the normal way, even if another call retains it.
                let mut boxed = OpaqueValue::new(Value::String(text.into()), None);
                *boxed.ref_count.get_mut() = 2;
                let pointer = allocate_box(boxed);
                register_direct_owned_value(pointer);
                pointer
            }
            Err(failure) => {
                foreign_call_failure(metadata_text(function, function_length), "AU4005", failure)
            }
        }
    })
}

/// Releases the callback entry's reference to its converted C text.
///
/// # Safety
///
/// `text` is null or came from that function and has not been freed.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_release_callback_text(text: *mut OpaqueValue) {
    if !text.is_null() {
        // SAFETY: the entry owns this reference outside the task's ledger.
        unsafe { release_untracked_value(text) }
    }
}

/// Raises the failure of a callback that ran under the foreign call that
/// has just returned, when there is one.
#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aura_direct_ffi_callback_check() {
    if MAY_HAVE_CALLBACK_FAILURE.get() {
        if let Some(payload) =
            super::with_direct_task_runtime_state(|state| state.foreign_callback_failure.take())
        {
            // A nested C call may have just returned to another Aura callback.
            // Unwind that callback's own exits before its C caller resumes.
            task_runtime_boundary(|| {
                match payload.downcast::<super::LightweightTaskFailureSignal>() {
                    Ok(signal) => runtime_diagnostic_error(signal.0),
                    Err(payload) => std::panic::resume_unwind(payload),
                }
            });
        }
    }
}

#[cfg(test)]
#[path = "native_foreign_calls_tests.rs"]
mod tests;

/// The C layout generated code describes as JSON in its data. A call site
/// passes the same bytes every time, so the layout is read once per
/// address.
///
/// # Safety
///
/// `descriptor` and `length` describe text generated code holds in its
/// data for the life of the program.
unsafe fn foreign_layout(descriptor: *const u8, length: usize) -> &'static CStruct {
    type Layouts = std::collections::HashMap<usize, (Box<str>, &'static CStruct)>;
    static LAYOUTS: std::sync::Mutex<Option<Layouts>> = std::sync::Mutex::new(None);
    // SAFETY: the caller's contract.
    let text = unsafe { metadata_text(descriptor, length) };
    let mut layouts = LAYOUTS.lock().unwrap_or_else(|poison| poison.into_inner());
    let layouts = layouts.get_or_insert_with(Default::default);
    if let Some((known, layout)) = layouts.get(&(descriptor as usize)) {
        if **known == *text {
            return layout;
        }
    }
    let Ok(layout) = serde_json::from_str::<CStruct>(text) else {
        runtime_error("a foreign call names a C layout that does not read");
    };
    if let Err(reason) = layout.validate_bounds() {
        runtime_error(reason);
    }
    let layout = layout.interned();
    layouts.insert(descriptor as usize, (text.into(), layout));
    layout
}

fn c_value_failure(symbol: &str, error: CValueError) -> ! {
    match error {
        CValueError::BadBool(byte) => foreign_call_failure(
            symbol,
            "AU4001",
            crate::ffi::FfiError::NonCanonicalBoolReturn(byte),
        ),
        other => runtime_diagnostic_error(Diagnostic::coded(other.code(), other.to_string())),
    }
}

/// Writes the class instance `value` at `slot` in the C layout that
/// `descriptor` describes. Generated code calls this for a class with an
/// array field or a union, whose value is not one word per scalar.
///
/// # Safety
///
/// `value` is a live value, `descriptor` is as [`foreign_layout`] needs,
/// and `slot` has room for one struct of the layout.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_pack_struct(
    value: *mut OpaqueValue,
    descriptor: *const u8,
    descriptor_length: usize,
    slot: *mut u8,
) {
    task_runtime_boundary(|| unsafe {
        let layout = foreign_layout(descriptor, descriptor_length);
        let bytes = slice::from_raw_parts_mut(slot, layout.size());
        if let Err(error) = with_value(value, |value| c_values::pack(value, layout, bytes)) {
            c_value_failure(&layout.name, error)
        }
    })
}

/// A new instance for the C struct at `slot`, which the foreign function
/// `symbol` returned.
///
/// # Safety
///
/// `descriptor` is as [`foreign_layout`] needs, `slot` holds one struct of
/// the layout, and the symbol name is UTF-8 text in generated code's data.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_unpack_struct(
    descriptor: *const u8,
    descriptor_length: usize,
    slot: *const u8,
    symbol: *const u8,
    symbol_length: usize,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| unsafe {
        let layout = foreign_layout(descriptor, descriptor_length);
        let bytes = slice::from_raw_parts(slot, layout.size());
        match c_values::unpack(layout, bytes) {
            Ok(value) => boxed_value(value),
            Err(error) => c_value_failure(metadata_text(symbol, symbol_length), error),
        }
    })
}

/// Sets every field of the instance `value` from the C struct at `slot`,
/// which the foreign function `symbol` wrote through a pointer.
///
/// # Safety
///
/// As [`aura_direct_ffi_unpack_struct`], and `value` is a live value its
/// caller holds exclusively.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_unpack_struct_into(
    value: *mut OpaqueValue,
    descriptor: *const u8,
    descriptor_length: usize,
    slot: *const u8,
    symbol: *const u8,
    symbol_length: usize,
) {
    task_runtime_boundary(|| unsafe {
        let layout = foreign_layout(descriptor, descriptor_length);
        let bytes = slice::from_raw_parts(slot, layout.size());
        let read = match c_values::unpack(layout, bytes) {
            Ok(Value::Instance(read)) => read,
            Ok(_) => runtime_error("a C struct did not read back as an instance"),
            Err(error) => c_value_failure(metadata_text(symbol, symbol_length), error),
        };
        value_mut(value, |value| {
            let Value::Instance(instance) = value else {
                runtime_error(format!(
                    "FFI argument expected a class instance, but received `{}`",
                    value_type_name(value)
                ));
            };
            for (name, field) in read.fields.iter() {
                if let Err(error) = instance.fields.insert(name, field.into_owned()) {
                    runtime_error(error);
                }
            }
        })
    })
}

/// The task owns the buffer through its handle. A trap while packing another
/// parameter or writing back a C result still releases it with the task ledger.
#[repr(C, align(16))]
struct StructArrayHeader {
    bytes: usize,
    owner: *mut OpaqueValue,
}

fn structs_block(bytes: usize) -> std::alloc::Layout {
    bytes
        .checked_add(std::mem::size_of::<StructArrayHeader>())
        .and_then(|size| {
            std::alloc::Layout::from_size_align(size, std::mem::align_of::<StructArrayHeader>())
                .ok()
        })
        .unwrap_or_else(|| runtime_error("a C struct array is too large"))
}

unsafe extern "C" fn free_structs_block(block: *mut c_void) {
    // SAFETY: the owning handle's final release is the only deallocation.
    let header = unsafe { &*block.cast::<StructArrayHeader>() };
    unsafe { std::alloc::dealloc(block.cast(), structs_block(header.bytes)) };
}

/// Stack storage available to one private copy of a read-only struct array.
pub const FFI_STRUCT_SCRATCH_BYTES: usize = 128;

/// Copies records whose native layout and stride match C into private storage.
/// The result is null for an empty list, `scratch` for a small list, or a
/// heap array the caller frees with [`aura_direct_ffi_release_structs`].
///
/// # Safety
///
/// `data` holds `length` initialized bytes and does not overlap `scratch`,
/// which has room for [`FFI_STRUCT_SCRATCH_BYTES`] bytes.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_copy_structs(
    data: *const u8,
    length: usize,
    scratch: *mut u8,
) -> *mut u8 {
    if length == 0 {
        return std::ptr::null_mut();
    }
    let array = if length <= FFI_STRUCT_SCRATCH_BYTES {
        scratch
    } else {
        allocate_structs(length, free_structs_block)
    };
    // SAFETY: the destination has room for the complete source.
    unsafe { std::ptr::copy_nonoverlapping(data, array, length) };
    array
}

fn allocate_structs(length: usize, release: FfiHandleDrop) -> *mut u8 {
    // SAFETY: the checked layout contains the header and the whole array.
    unsafe {
        let header = std::alloc::alloc(structs_block(length)).cast::<StructArrayHeader>();
        if header.is_null() {
            runtime_error("a C struct array could not be allocated");
        }
        header.write(StructArrayHeader {
            bytes: length,
            owner: std::ptr::null_mut(),
        });
        let handle = FfiHandleValue::owned(String::new(), header.cast(), Some(release))
            .expect("the checked C array allocation is non-null");
        (*header).owner = boxed_value(Value::FfiHandle(handle));
        header.add(1).cast()
    }
}

/// A new C array of the layout `descriptor` describes that holds the
/// elements of the list `value`, or null for an empty list. The caller
/// frees it with [`aura_direct_ffi_release_structs`].
///
/// # Safety
///
/// `value` is a live value and `descriptor` is as [`foreign_layout`]
/// needs.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_pack_structs(
    value: *mut OpaqueValue,
    descriptor: *const u8,
    descriptor_length: usize,
) -> *mut u8 {
    task_runtime_boundary(|| unsafe {
        let layout = foreign_layout(descriptor, descriptor_length);
        let bytes = match with_value(value, |value| c_values::pack_elements(value, layout)) {
            Ok(bytes) => bytes,
            Err(error) => c_value_failure(&layout.name, error),
        };
        if bytes.is_empty() {
            return std::ptr::null_mut();
        }
        let array = allocate_structs(bytes.len(), free_structs_block);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), array, bytes.len());
        array
    })
}

/// Replaces the elements of the list `value` with those of `array`, which
/// the foreign function `symbol` wrote through a `mut ptr[Name]`.
///
/// # Safety
///
/// As [`aura_direct_ffi_pack_structs`]; `array` is what that function
/// returned for `value`, `value` is held exclusively, and the symbol name
/// is UTF-8 text in generated code's data.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_unpack_structs(
    value: *mut OpaqueValue,
    descriptor: *const u8,
    descriptor_length: usize,
    array: *const u8,
    symbol: *const u8,
    symbol_length: usize,
) {
    if array.is_null() {
        return;
    }
    task_runtime_boundary(|| unsafe {
        let layout = foreign_layout(descriptor, descriptor_length);
        let length = (*array.cast::<StructArrayHeader>().sub(1)).bytes;
        let read = match c_values::unpack_elements(layout, slice::from_raw_parts(array, length)) {
            Ok(Value::Vec(read)) => read,
            Ok(_) => runtime_error("a C struct array did not read back as a list"),
            Err(error) => c_value_failure(metadata_text(symbol, symbol_length), error),
        };
        value_mut(value, |value| {
            let Value::Vec(vector) = value else {
                runtime_error(format!(
                    "FFI argument expected a list for a mut pointer, but received `{}`",
                    value_type_name(value)
                ));
            };
            // A list of generated class instances can keep using its native
            // field offsets after C fills it. Decode the entire array first:
            // one invalid member must leave every element of the list alone.
            let mut elements = read.elements;
            let class = match &vector.elements {
                ListElements::Values(_) | ListElements::Boxes(_) => {
                    vector.elements.first().and_then(|first| match &*first {
                        Value::Instance(instance) => {
                            instance.fields.typed().map(TypedFields::layout)
                        }
                        _ => None,
                    })
                }
                _ => None,
            };
            if let Some(class) = class.filter(|class| class.class_name() == layout.name) {
                for element in elements.values_mut() {
                    if let Value::Instance(instance) = element {
                        let fields = std::mem::take(instance.fields.map_mut());
                        instance.fields = TypedFields::from_map(class, fields)
                            .map_or_else(InstanceFields::Map, InstanceFields::Typed);
                    }
                }
            }
            if matches!(vector.elements, ListElements::Boxes(_)) {
                // A borrowed element already had a stable box before C ran.
                // Let generated code borrow the replacements the same way.
                let _ = elements.lend(0);
            }
            vector.elements = elements;
        })
    })
}

/// Frees a C array [`aura_direct_ffi_pack_structs`] returned.
///
/// # Safety
///
/// `array` is null or a pointer that function returned, not yet freed.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_ffi_release_structs(array: *mut u8) {
    if array.is_null() {
        return;
    }
    // SAFETY: only a live allocation produced by allocate_structs reaches here.
    let owner = unsafe { (*array.cast::<StructArrayHeader>().sub(1)).owner };
    unsafe { super::aura_direct_release_value(owner) };
}
