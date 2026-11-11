use std::ffi::{c_void, CStr};
use std::slice;

use aura_core::diag::Diagnostic;
use aura_core::types::Type;

use super::super::{
    aura_direct_release_value, aura_direct_retain_value, aura_direct_string_literal, boxed_value,
    with_direct_task_runtime_scope, with_task_runtime_error_capture, with_value, OpaqueValue,
};
use super::{
    aura_direct_ffi_bad_bool, aura_direct_ffi_bytes_view, aura_direct_ffi_bytes_view_mut,
    aura_direct_ffi_c_string, aura_direct_ffi_c_string_result, aura_direct_ffi_check_flags,
    aura_direct_ffi_elements, aura_direct_ffi_elements_mut, aura_direct_ffi_handle,
    aura_direct_ffi_handle_address, aura_direct_ffi_none, aura_direct_ffi_null_handle,
    aura_direct_ffi_nullable_handle_address, aura_direct_ffi_release_bytes,
    aura_direct_ffi_release_c_string, aura_direct_ffi_str_view, ForeignView,
};
use crate::integer::{IntegerKind, IntegerValue};
use crate::list_elements::{ListElements, ListForm};
use crate::runtime_value::{
    run_lightweight_root_task, shared_union_value, SharedType, Value, VecValue,
};

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
        let handle = aura_direct_ffi_handle(address, name.as_ptr(), name.len(), None);
        assert!(!handle.is_null());
        with_value(handle, |value| match value {
            Value::FfiHandle(handle) => assert_eq!(handle.type_name(), "Database"),
            other => panic!("expected a handle, found {other:?}"),
        });
        assert_eq!(aura_direct_ffi_handle_address(handle), address);
        aura_direct_release_value(handle);

        let null = aura_direct_ffi_handle(std::ptr::null_mut(), name.as_ptr(), name.len(), None);
        assert!(null.is_null(), "a null address is no handle");

        let unnamed = aura_direct_ffi_handle(address, std::ptr::null(), 0, None);
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

/// `payload`, a value of the type `member`, in the union of `member` and
/// `None`.
fn nullable_value(member: &str, payload: Value) -> *mut OpaqueValue {
    let active = match payload {
        Value::Unit => Type::Unit,
        _ => Type::named(member),
    };
    let union_type = Type::normalize_union(
        vec![Type::named(member), Type::Unit],
        "main",
        &Default::default(),
    )
    .expect("a union of two members");
    let Type::Union(union) = &union_type else {
        panic!("expected a union, found {union_type}");
    };
    let index = union
        .members
        .iter()
        .position(|candidate| *candidate == active)
        .expect("the active member is a member");
    boxed_value(shared_union_value(
        std::sync::Arc::new(union_type.clone()),
        index,
        payload,
    ))
}

#[test]
fn a_nullable_handle_address_is_null_for_none() {
    unsafe {
        let mut target = 0_u8;
        let address: *mut c_void = (&raw mut target).cast();
        let name = "Database";
        let handle = aura_direct_ffi_handle(address, name.as_ptr(), name.len(), None);
        assert_eq!(aura_direct_ffi_nullable_handle_address(handle), address);
        let payload = with_value(handle, Value::clone);
        aura_direct_release_value(handle);

        let some = nullable_value("Database", payload);
        assert_eq!(aura_direct_ffi_nullable_handle_address(some), address);
        aura_direct_release_value(some);

        let none = nullable_value("Database", Value::Unit);
        assert!(aura_direct_ffi_nullable_handle_address(none).is_null());
        aura_direct_release_value(none);
    }

    let message = capture_diagnostic(|| unsafe {
        aura_direct_ffi_nullable_handle_address(string_value("x"));
    })
    .message;
    assert!(
        message.contains("FFI argument expected opaque handle or None, but received"),
        "{message}"
    );
}

#[test]
fn a_c_string_argument_is_a_terminated_copy_and_none_is_null() {
    let symbol = "open";
    unsafe {
        let text = string_value("héllo");
        let copy = aura_direct_ffi_c_string(text, 0, symbol.as_ptr(), symbol.len());
        assert_eq!(CStr::from_ptr(copy).to_bytes(), "héllo".as_bytes());
        aura_direct_ffi_release_c_string(copy);
        aura_direct_release_value(text);

        let some = nullable_value("str", Value::String("ab".into()));
        let copy = aura_direct_ffi_c_string(some, 0, symbol.as_ptr(), symbol.len());
        assert_eq!(CStr::from_ptr(copy).to_bytes(), b"ab");
        aura_direct_ffi_release_c_string(copy);
        aura_direct_release_value(some);

        let none = nullable_value("str", Value::Unit);
        let copy = aura_direct_ffi_c_string(none, 0, symbol.as_ptr(), symbol.len());
        assert!(copy.is_null());
        aura_direct_ffi_release_c_string(copy);
        aura_direct_release_value(none);
    }

    let diagnostic = capture_diagnostic(move || unsafe {
        aura_direct_ffi_c_string(string_value("a\0b"), 2, symbol.as_ptr(), symbol.len());
    });
    assert_eq!(diagnostic.code, "AU4005");
    assert_eq!(
        diagnostic.message,
        "FFI call to `open` failed: FFI argument 3 is a cstr that contains a NUL byte"
    );

    let message = capture_diagnostic(move || unsafe {
        aura_direct_ffi_c_string(unboxed_bytes(&[1]), 0, symbol.as_ptr(), symbol.len());
    })
    .message;
    assert!(
        message.contains("FFI argument expected cstr, but received"),
        "{message}"
    );
}

#[test]
fn a_c_string_result_is_copied_into_a_new_str() {
    let symbol = "name";
    unsafe {
        let text =
            aura_direct_ffi_c_string_result(c"héllo".as_ptr(), 0, symbol.as_ptr(), symbol.len());
        with_value(text, |value| match value {
            Value::String(text) => assert_eq!(&**text, "héllo"),
            other => panic!("expected a string, found {other:?}"),
        });
        aura_direct_release_value(text);

        let none =
            aura_direct_ffi_c_string_result(std::ptr::null(), 1, symbol.as_ptr(), symbol.len());
        assert!(none.is_null(), "a nullable result reports null as no text");
    }

    let diagnostic = capture_diagnostic(move || unsafe {
        aura_direct_ffi_c_string_result(std::ptr::null(), 0, symbol.as_ptr(), symbol.len());
    });
    assert_eq!(diagnostic.code, "AU4005");
    assert_eq!(
        diagnostic.message,
        "FFI call to `name` failed: FFI function returned a null cstr"
    );

    for nullable in [0, 1] {
        let diagnostic = capture_diagnostic(move || unsafe {
            aura_direct_ffi_c_string_result(
                c"\xff".as_ptr(),
                nullable,
                symbol.as_ptr(),
                symbol.len(),
            );
        });
        assert_eq!(diagnostic.code, "AU4005");
        assert_eq!(
            diagnostic.message,
            "FFI call to `name` failed: FFI function returned a cstr that is not UTF-8"
        );
    }
}

fn float_values(values: &[f64]) -> ListElements {
    ListElements::Values(
        values
            .iter()
            .copied()
            .map(Value::Float)
            .collect::<Vec<_>>()
            .into(),
    )
}

fn scalar_floats(values: &[f64]) -> ListElements {
    float_values(values)
        .in_scalar_form(ListForm::Float64)
        .expect("floats fit the float64 form")
}

fn scalar_flags(values: &[bool]) -> ListElements {
    let values: Vec<Value> = values.iter().copied().map(Value::Bool).collect();
    ListElements::Values(values.into())
        .in_scalar_form(ListForm::Bool)
        .expect("bools fit the bool form")
}

fn floats_of(list: *mut OpaqueValue) -> Vec<f64> {
    unsafe {
        with_value(list, |value| match value {
            Value::Vec(vector) => vector
                .elements
                .iter()
                .map(|element| match &*element {
                    Value::Float(value) => *value,
                    other => panic!("expected a float, found {other:?}"),
                })
                .collect(),
            other => panic!("expected a list, found {other:?}"),
        })
    }
}

/// The list access codes generated code names scalar forms by.
const FLOAT64: i64 = 3;
const BOOL: i64 = 4;
const FLOAT32: i64 = 13;

#[test]
fn pointer_elements_lend_a_scalar_list_and_copy_any_other() {
    unsafe {
        let list = list_value(scalar_floats(&[1.5, 2.5]));
        let mut view = empty_view();
        let copy = aura_direct_ffi_elements(list, FLOAT64, &mut view);
        assert!(copy.is_null(), "a scalar list lends its own elements");
        assert_eq!(view.length, 2);
        assert_eq!(
            slice::from_raw_parts(view.pointer.cast::<f64>(), 2),
            [1.5, 2.5]
        );
        aura_direct_release_value(list);

        let list = list_value(float_values(&[4.0, 5.0, 6.0]));
        let mut view = empty_view();
        let copy = aura_direct_ffi_elements(list, FLOAT64, &mut view);
        assert!(!copy.is_null(), "boxed elements need a contiguous copy");
        assert_eq!(
            slice::from_raw_parts(view.pointer.cast::<f64>(), 3),
            [4.0, 5.0, 6.0]
        );
        aura_direct_release_value(copy);
        aura_direct_release_value(list);

        // A `float32` list that keeps doubles is narrowed in the copy.
        let list = list_value(float_values(&[0.5, 1.5]));
        let mut view = empty_view();
        let copy = aura_direct_ffi_elements(list, FLOAT32, &mut view);
        assert!(!copy.is_null());
        assert_eq!(
            slice::from_raw_parts(view.pointer.cast::<f32>(), 2),
            [0.5, 1.5]
        );
        aura_direct_release_value(copy);
        aura_direct_release_value(list);

        for empty in [
            list_value(scalar_floats(&[])),
            list_value(float_values(&[])),
        ] {
            let mut view = empty_view();
            let copy = aura_direct_ffi_elements(empty, FLOAT64, &mut view);
            assert!(copy.is_null());
            assert!(view.pointer.is_null());
            assert_eq!(view.length, 0);
            aura_direct_release_value(empty);
        }
    }
}

#[test]
fn mutable_pointer_elements_write_through_to_the_list() {
    unsafe {
        for list in [
            list_value(scalar_floats(&[1.0, 2.0])),
            list_value(float_values(&[1.0, 2.0])),
        ] {
            let mut view = empty_view();
            aura_direct_ffi_elements_mut(list, FLOAT64, &mut view);
            assert_eq!(view.length, 2);
            slice::from_raw_parts_mut(view.pointer.cast_mut().cast::<f64>(), 2)
                .copy_from_slice(&[7.5, 8.5]);
            assert_eq!(floats_of(list), [7.5, 8.5]);
            aura_direct_release_value(list);
        }

        let empty = list_value(float_values(&[]));
        let mut view = empty_view();
        aura_direct_ffi_elements_mut(empty, FLOAT64, &mut view);
        assert!(view.pointer.is_null());
        assert_eq!(view.length, 0);
        aura_direct_release_value(empty);
    }
}

#[test]
fn pointer_elements_reject_values_that_are_not_lists_of_the_scalar() {
    for (work, expected) in [
        (
            (|view: &mut ForeignView| unsafe {
                aura_direct_ffi_elements(string_value("x"), FLOAT64, view);
            }) as fn(&mut ForeignView),
            "FFI argument expected a list for a pointer, but received",
        ),
        (
            |view| unsafe {
                aura_direct_ffi_elements_mut(string_value("x"), FLOAT64, view);
            },
            "FFI argument expected a list for a mut pointer, but received",
        ),
        (
            |view| unsafe {
                let list = list_value(ListElements::Values(vec![Value::Bool(true)].into()));
                aura_direct_ffi_elements(list, FLOAT64, view);
            },
            "FFI argument expected a list for a pointer, but an element is another type",
        ),
        (
            |view| unsafe {
                let list = list_value(ListElements::Values(vec![Value::Bool(true)].into()));
                aura_direct_ffi_elements_mut(list, FLOAT64, view);
            },
            "FFI argument expected a list for a mut pointer, but an element is another type",
        ),
        (
            |view| unsafe {
                aura_direct_ffi_elements(list_value(float_values(&[1.0])), 5, view);
            },
            "list access code 5 names no scalar form",
        ),
        (
            |_| unsafe {
                let list = list_value(scalar_floats(&[1.0]));
                aura_direct_ffi_elements(list, FLOAT64, std::ptr::null_mut());
            },
            "a foreign call view has no place to go",
        ),
    ] {
        let message = capture_diagnostic(move || {
            let mut view = empty_view();
            work(&mut view);
        })
        .message;
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn every_scalar_list_form_has_an_access_code() {
    unsafe {
        for access in [2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13] {
            let empty = list_value(ListElements::Values(Vec::new().into()));
            let mut view = empty_view();
            assert!(aura_direct_ffi_elements(empty, access, &mut view).is_null());
            assert_eq!(view.length, 0);
            aura_direct_release_value(empty);
        }
    }
}

#[test]
fn a_flag_list_with_a_byte_that_is_not_a_bool_ends_the_task() {
    let symbol = "fill_flags";
    unsafe {
        let list = list_value(scalar_flags(&[true, false]));
        aura_direct_ffi_check_flags(list, symbol.as_ptr(), symbol.len());
        aura_direct_release_value(list);
        // A value that is no flag list has nothing to check.
        let text = string_value("x");
        aura_direct_ffi_check_flags(text, symbol.as_ptr(), symbol.len());
        aura_direct_release_value(text);
        let floats = list_value(scalar_floats(&[1.0]));
        aura_direct_ffi_check_flags(floats, symbol.as_ptr(), symbol.len());
        aura_direct_release_value(floats);
    }
    let diagnostic = capture_diagnostic(move || unsafe {
        let list = list_value(scalar_flags(&[true, false]));
        let mut view = empty_view();
        aura_direct_ffi_elements_mut(list, BOOL, &mut view);
        view.pointer.cast_mut().add(1).write(7);
        aura_direct_ffi_check_flags(list, symbol.as_ptr(), symbol.len());
    });
    assert_eq!(diagnostic.code, "AU4001");
    assert_eq!(
        diagnostic.message,
        format!(
            "FFI call to `fill_flags` failed: {}",
            crate::ffi::FfiError::NonCanonicalBoolReturn(7)
        )
    );
}

#[test]
fn a_callback_failure_waits_for_the_foreign_call_to_return() {
    use super::{aura_direct_ffi_callback_check, aura_direct_ffi_callback_guard};
    unsafe extern "C-unwind" fn doubles(frame: *mut u64) {
        unsafe { *frame.add(1) = *frame * 2 };
    }
    unsafe extern "C-unwind" fn fails(frame: *mut u64) {
        unsafe { *frame.add(1) = 9 };
        std::panic::panic_any(41_i32);
    }

    let mut frame = [21_u64, 0];
    unsafe { aura_direct_ffi_callback_guard(doubles, frame.as_mut_ptr()) };
    assert_eq!(frame, [21, 42]);
    // Nothing failed, so the check returns.
    aura_direct_ffi_callback_check();

    let mut frame = [1_u64, 0];
    unsafe { aura_direct_ffi_callback_guard(fails, frame.as_mut_ptr()) };
    assert_eq!(frame[1], 9);
    // A later callback under the same foreign call does not run.
    let mut later = [21_u64, 0];
    unsafe { aura_direct_ffi_callback_guard(doubles, later.as_mut_ptr()) };
    assert_eq!(later, [21, 0]);

    let raised = std::panic::catch_unwind(|| aura_direct_ffi_callback_check())
        .expect_err("the check raises the parked failure");
    assert_eq!(raised.downcast_ref::<i32>(), Some(&41));
    // The failure is raised once.
    aura_direct_ffi_callback_check();
    unsafe { aura_direct_ffi_callback_guard(doubles, later.as_mut_ptr()) };
    assert_eq!(later, [21, 42]);
}

static RELEASED: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

unsafe extern "C" fn note_release(address: *mut c_void) {
    RELEASED.lock().expect("release log").push(address as usize);
}

/// How many times the handle drop function ran for `address`.
fn releases_of(address: *mut c_void) -> usize {
    let released = RELEASED.lock().expect("release log");
    released
        .iter()
        .filter(|released| **released == address as usize)
        .count()
}

#[test]
fn a_handle_with_a_drop_function_releases_its_object_once() {
    use super::{aura_direct_ffi_give_up_handle, aura_direct_ffi_stored_handle};
    let name = "Database";
    let mut targets = [0_u8; 5];
    let address =
        |index: usize| -> *mut c_void { targets.as_ptr().wrapping_add(index) as *mut c_void };
    let open = |index: usize| unsafe {
        aura_direct_ffi_handle(
            address(index),
            name.as_ptr(),
            name.len(),
            Some(note_release),
        )
    };
    unsafe {
        // Letting go of the last copy of the handle releases the object.
        let dropped = open(0);
        aura_direct_retain_value(dropped);
        aura_direct_release_value(dropped);
        assert_eq!(releases_of(address(0)), 0, "one owner is left");
        aura_direct_release_value(dropped);
        assert_eq!(releases_of(address(0)), 1);

        // A handle a foreign function took is not the program's to release.
        let taken = open(1);
        aura_direct_ffi_give_up_handle(taken);
        assert!(aura_direct_ffi_handle_address(taken).is_null());
        aura_direct_release_value(taken);
        assert_eq!(releases_of(address(1)), 0);
        let text = string_value("x");
        aura_direct_ffi_give_up_handle(text);
        aura_direct_release_value(text);

        // A place whose address the function left alone keeps its handle.
        let held = open(2);
        let kept = aura_direct_ffi_stored_handle(
            held,
            address(2),
            name.as_ptr(),
            name.len(),
            Some(note_release),
        );
        aura_direct_release_value(held);
        assert_eq!(releases_of(address(2)), 0, "the kept handle still owns it");
        aura_direct_release_value(kept);
        assert_eq!(releases_of(address(2)), 1);

        // Another address is a new handle, and the old object is given up.
        let held = open(3);
        let stored = aura_direct_ffi_stored_handle(
            held,
            address(4),
            name.as_ptr(),
            name.len(),
            Some(note_release),
        );
        aura_direct_release_value(held);
        assert_eq!(aura_direct_ffi_handle_address(stored), address(4));
        aura_direct_release_value(stored);
        assert_eq!((releases_of(address(3)), releases_of(address(4))), (0, 1));

        // A null address over a handle, and any address over `None`.
        let held = open(3);
        let cleared = aura_direct_ffi_stored_handle(
            held,
            std::ptr::null_mut(),
            name.as_ptr(),
            name.len(),
            Some(note_release),
        );
        assert!(cleared.is_null());
        aura_direct_release_value(held);
        assert_eq!(releases_of(address(3)), 0);
        let none = aura_direct_ffi_none();
        let first =
            aura_direct_ffi_stored_handle(none, address(3), name.as_ptr(), name.len(), None);
        aura_direct_release_value(none);
        aura_direct_release_value(first);
        assert_eq!(releases_of(address(3)), 0, "a type with no drop function");
    }
    targets[0] = 1;
}
