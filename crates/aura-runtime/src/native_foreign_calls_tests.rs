use std::ffi::c_void;
use std::slice;

use aura_core::diag::Diagnostic;
use aura_core::types::Type;

use super::super::{
    aura_direct_release_value, aura_direct_string_literal, boxed_value,
    with_direct_task_runtime_scope, with_task_runtime_error_capture, with_value, OpaqueValue,
};
use super::{
    aura_direct_ffi_bad_bool, aura_direct_ffi_bytes_view, aura_direct_ffi_bytes_view_mut,
    aura_direct_ffi_handle, aura_direct_ffi_handle_address, aura_direct_ffi_none,
    aura_direct_ffi_null_handle, aura_direct_ffi_release_bytes, aura_direct_ffi_str_view,
    ForeignView,
};
use crate::integer::{IntegerKind, IntegerValue};
use crate::list_elements::ListElements;
use crate::runtime_value::{run_lightweight_root_task, SharedType, Value, VecValue};

fn empty_view() -> ForeignView {
    ForeignView {
        pointer: std::ptr::dangling(),
        length: usize::MAX,
    }
}

/// # Safety
///
/// `view` describes bytes that are still alive.
unsafe fn view_bytes<'a>(view: &ForeignView) -> &'a [u8] {
    if view.pointer.is_null() {
        assert_eq!(view.length, 0, "a null view lends no bytes");
        return &[];
    }
    unsafe { slice::from_raw_parts(view.pointer, view.length) }
}

fn string_value(text: &str) -> *mut OpaqueValue {
    aura_direct_string_literal(text.as_ptr(), text.len())
}

fn byte(value: u8) -> Value {
    Value::Int(
        IntegerValue::from_typed_unsigned(u128::from(value), IntegerKind::Uint8)
            .expect("every byte fits uint8"),
    )
}

fn list_value(elements: ListElements) -> *mut OpaqueValue {
    boxed_value(Value::Vec(VecValue {
        element_type: SharedType::from(Type::named("uint8")),
        elements,
        str_view: Default::default(),
    }))
}

fn unboxed_bytes(bytes: &[u8]) -> *mut OpaqueValue {
    list_value(ListElements::from_uint8(bytes.to_vec()))
}

fn boxed_bytes(bytes: &[u8]) -> *mut OpaqueValue {
    let values: Vec<Value> = bytes.iter().copied().map(byte).collect();
    list_value(ListElements::Values(values.into()))
}

/// The diagnostic `work` ends its task with, run the way generated code
/// runs: on a task, inside the runtime's failure boundary.
fn capture_diagnostic(work: impl FnOnce() + Send + 'static) -> Diagnostic {
    run_lightweight_root_task(move || {
        with_direct_task_runtime_scope(|| {
            with_task_runtime_error_capture(|| {
                work();
                Ok(Value::Unit)
            })
        })
    })
    .expect_err("the helper should end the task")
}

#[test]
fn a_str_view_lends_the_text_bytes_in_place() {
    unsafe {
        let text = string_value("héllo");
        let mut view = empty_view();
        aura_direct_ffi_str_view(text, &mut view);
        assert_eq!(view_bytes(&view), "héllo".as_bytes());
        let lent = with_value(text, |value| match value {
            Value::String(text) => text.as_bytes().as_ptr(),
            other => panic!("expected a string, found {other:?}"),
        });
        assert_eq!(view.pointer, lent, "the view must not copy the text");
        aura_direct_release_value(text);

        let empty = string_value("");
        let mut view = empty_view();
        aura_direct_ffi_str_view(empty, &mut view);
        assert!(view.pointer.is_null());
        assert_eq!(view.length, 0);
        aura_direct_release_value(empty);
    }
}

#[test]
fn a_str_view_rejects_other_values_and_a_missing_destination() {
    let message = capture_diagnostic(|| unsafe {
        let mut view = empty_view();
        aura_direct_ffi_str_view(unboxed_bytes(&[1]), &mut view);
    })
    .message;
    assert!(
        message.contains("FFI argument expected str view, but received"),
        "{message}"
    );

    let message = capture_diagnostic(|| unsafe {
        aura_direct_ffi_str_view(string_value("x"), std::ptr::null_mut());
    })
    .message;
    assert_eq!(message, "a foreign call view has no place to go");
}

#[test]
fn a_bytes_view_lends_unboxed_bytes_and_copies_boxed_ones() {
    unsafe {
        let list = unboxed_bytes(&[1, 2, 3]);
        let mut view = empty_view();
        let copy = aura_direct_ffi_bytes_view(list, &mut view);
        assert!(copy.is_null(), "unboxed bytes are lent, not copied");
        assert_eq!(view_bytes(&view), [1, 2, 3]);
        aura_direct_ffi_release_bytes(copy, view.length);
        aura_direct_release_value(list);

        let list = boxed_bytes(&[4, 5]);
        let mut view = empty_view();
        let copy = aura_direct_ffi_bytes_view(list, &mut view);
        assert!(!copy.is_null(), "boxed elements need a contiguous copy");
        assert_eq!(view.pointer, copy.cast_const());
        assert_eq!(view_bytes(&view), [4, 5]);
        aura_direct_ffi_release_bytes(copy, view.length);
        aura_direct_release_value(list);

        for empty in [unboxed_bytes(&[]), boxed_bytes(&[])] {
            let mut view = empty_view();
            let copy = aura_direct_ffi_bytes_view(empty, &mut view);
            assert!(copy.is_null());
            assert!(view.pointer.is_null());
            assert_eq!(view.length, 0);
            aura_direct_release_value(empty);
        }
    }
}

#[test]
fn a_bytes_view_rejects_values_that_are_not_byte_lists() {
    let message = capture_diagnostic(|| unsafe {
        let mut view = empty_view();
        aura_direct_ffi_bytes_view(string_value("x"), &mut view);
    })
    .message;
    assert!(
        message.contains("FFI argument expected list[uint8] view, but received"),
        "{message}"
    );

    let message = capture_diagnostic(|| unsafe {
        let mut view = empty_view();
        let list = list_value(ListElements::Values(vec![Value::Bool(true)].into()));
        aura_direct_ffi_bytes_view(list, &mut view);
    })
    .message;
    assert_eq!(
        message,
        "FFI argument expected list[uint8] view, but an element is not a uint8"
    );
}

#[test]
fn a_mutable_bytes_view_writes_through_to_the_list() {
    unsafe {
        for list in [unboxed_bytes(&[1, 2, 3]), boxed_bytes(&[1, 2, 3])] {
            let mut view = empty_view();
            aura_direct_ffi_bytes_view_mut(list, &mut view);
            assert_eq!(view.length, 3);
            slice::from_raw_parts_mut(view.pointer.cast_mut(), view.length)
                .copy_from_slice(&[7, 8, 9]);
            let bytes = with_value(list, |value| match value {
                Value::Vec(vector) => vector.elements.to_uint8(),
                other => panic!("expected a list, found {other:?}"),
            });
            assert_eq!(bytes, Some(vec![7, 8, 9]));
            aura_direct_release_value(list);
        }

        let empty = unboxed_bytes(&[]);
        let mut view = empty_view();
        aura_direct_ffi_bytes_view_mut(empty, &mut view);
        assert!(view.pointer.is_null());
        assert_eq!(view.length, 0);
        aura_direct_release_value(empty);
    }
}

#[test]
fn a_mutable_bytes_view_rejects_values_that_are_not_byte_lists() {
    let message = capture_diagnostic(|| unsafe {
        let mut view = empty_view();
        aura_direct_ffi_bytes_view_mut(string_value("x"), &mut view);
    })
    .message;
    assert!(
        message.contains("FFI argument expected mut list[uint8] view, but received"),
        "{message}"
    );

    let message = capture_diagnostic(|| unsafe {
        let mut view = empty_view();
        let list = list_value(ListElements::Values(vec![Value::Bool(true)].into()));
        aura_direct_ffi_bytes_view_mut(list, &mut view);
    })
    .message;
    assert_eq!(
        message,
        "FFI argument expected mut list[uint8] view, but an element is not a uint8"
    );
}

#[test]
fn a_handle_boxes_an_address_and_gives_it_back() {
    unsafe {
        let mut target = 0_u8;
        let address: *mut c_void = (&raw mut target).cast();
        let name = "Database";
        let handle = aura_direct_ffi_handle(address, name.as_ptr(), name.len());
        assert!(!handle.is_null());
        with_value(handle, |value| match value {
            Value::FfiHandle(handle) => assert_eq!(handle.type_name(), "Database"),
            other => panic!("expected a handle, found {other:?}"),
        });
        assert_eq!(aura_direct_ffi_handle_address(handle), address);
        aura_direct_release_value(handle);

        let null = aura_direct_ffi_handle(std::ptr::null_mut(), name.as_ptr(), name.len());
        assert!(null.is_null(), "a null address is no handle");

        let unnamed = aura_direct_ffi_handle(address, std::ptr::null(), 0);
        with_value(unnamed, |value| match value {
            Value::FfiHandle(handle) => assert_eq!(handle.type_name(), ""),
            other => panic!("expected a handle, found {other:?}"),
        });
        aura_direct_release_value(unnamed);
    }
}

#[test]
fn a_handle_address_rejects_values_that_are_not_handles() {
    let message = capture_diagnostic(|| unsafe {
        aura_direct_ffi_handle_address(string_value("x"));
    })
    .message;
    assert!(
        message.contains("FFI argument expected opaque handle, but received"),
        "{message}"
    );
}

#[test]
fn the_none_helper_builds_a_unit_payload() {
    unsafe {
        let none = aura_direct_ffi_none();
        with_value(none, |value| assert!(matches!(value, Value::Unit)));
        aura_direct_release_value(none);
    }
}

#[test]
fn results_a_declaration_rules_out_end_the_task_with_coded_diagnostics() {
    let symbol = "open_database";
    let diagnostic = capture_diagnostic(move || unsafe {
        aura_direct_ffi_null_handle(symbol.as_ptr(), symbol.len());
    });
    assert_eq!(diagnostic.code, "AU4005");
    assert_eq!(
        diagnostic.message,
        format!(
            "FFI call to `open_database` failed: {}",
            crate::ffi::FfiError::NullOpaqueHandleReturn
        )
    );

    let diagnostic = capture_diagnostic(move || unsafe {
        aura_direct_ffi_bad_bool(7, symbol.as_ptr(), symbol.len());
    });
    assert_eq!(diagnostic.code, "AU4001");
    assert_eq!(
        diagnostic.message,
        format!(
            "FFI call to `open_database` failed: {}",
            crate::ffi::FfiError::NonCanonicalBoolReturn(7)
        )
    );
}
