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
    aura_direct_ffi_c_string, aura_direct_ffi_c_string_result, aura_direct_ffi_callback_text,
    aura_direct_ffi_check_flags, aura_direct_ffi_elements, aura_direct_ffi_elements_mut,
    aura_direct_ffi_handle, aura_direct_ffi_handle_address, aura_direct_ffi_none,
    aura_direct_ffi_null_handle, aura_direct_ffi_nullable_handle_address,
    aura_direct_ffi_pack_struct, aura_direct_ffi_pack_structs, aura_direct_ffi_release_bytes,
    aura_direct_ffi_release_c_string, aura_direct_ffi_release_callback_text,
    aura_direct_ffi_release_structs, aura_direct_ffi_str_view, aura_direct_ffi_unpack_struct,
    aura_direct_ffi_unpack_struct_into, aura_direct_ffi_unpack_structs, ForeignView,
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
fn temporary_c_string_and_bytes_view_copies_are_tracked_and_freed_on_trap() {
    let symbol = "open_two";
    with_direct_task_runtime_scope(|| unsafe {
        let text = string_value("hello");
        let boxed = boxed_bytes(&[1, 2, 3]);
        let before = super::super::with_direct_task_runtime_state(|state| {
            state.owned_values().entries().len()
        });
        let cstr = aura_direct_ffi_c_string(text, 0, symbol.as_ptr(), symbol.len());
        let mut view = empty_view();
        let bytes = aura_direct_ffi_bytes_view(boxed, &mut view);
        assert!(!cstr.is_null());
        assert!(!bytes.is_null());
        let during = super::super::with_direct_task_runtime_state(|state| {
            state.owned_values().entries().len()
        });
        assert_eq!(
            during,
            before + 2,
            "temporary CString and boxed BytesView copies must be tracked in the task ledger"
        );
        aura_direct_ffi_release_c_string(cstr);
        aura_direct_ffi_release_bytes(bytes, view.length);
        let after = super::super::with_direct_task_runtime_state(|state| {
            state.owned_values().entries().len()
        });
        assert_eq!(after, before);
        aura_direct_release_value(text);
        aura_direct_release_value(boxed);
    });

    // Mid-call trap after arg 1 allocates CString and BytesView copies must not
    // leak either allocation when the task scope unwinds.
    let diagnostic = capture_diagnostic(move || unsafe {
        let first = string_value("arg1");
        let boxed = boxed_bytes(&[9, 8, 7]);
        let _cstr = aura_direct_ffi_c_string(first, 0, symbol.as_ptr(), symbol.len());
        let mut view = empty_view();
        let _bytes = aura_direct_ffi_bytes_view(boxed, &mut view);
        let bad = string_value("bad\0arg2");
        let _ = aura_direct_ffi_c_string(bad, 1, symbol.as_ptr(), symbol.len());
    });
    assert_eq!(diagnostic.code, "AU4005");
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
fn sanitize_flags_normalizes_all_non_canonical_bytes_before_reporting() {
    use super::aura_direct_ffi_sanitize_flags;
    unsafe {
        let first = list_value(scalar_flags(&[false, false]));
        let second = list_value(scalar_flags(&[false, false]));
        let mut first_view = empty_view();
        let mut second_view = empty_view();
        aura_direct_ffi_elements_mut(first, BOOL, &mut first_view);
        aura_direct_ffi_elements_mut(second, BOOL, &mut second_view);
        first_view.pointer.cast_mut().add(0).write(2);
        second_view.pointer.cast_mut().add(1).write(255);

        assert_eq!(aura_direct_ffi_sanitize_flags(first), 2);
        assert_eq!(aura_direct_ffi_sanitize_flags(second), 255);
        // Subsequent checks see canonical 0/1 bytes in both lists.
        assert_eq!(aura_direct_ffi_sanitize_flags(first), 0);
        assert_eq!(aura_direct_ffi_sanitize_flags(second), 0);
        assert_eq!(with_value(first, |v| v.render()), "[true, false]");
        assert_eq!(with_value(second, |v| v.render()), "[false, true]");

        let text = string_value("x");
        assert_eq!(aura_direct_ffi_sanitize_flags(text), 0);
        aura_direct_release_value(text);
        aura_direct_release_value(first);
        aura_direct_release_value(second);
    }
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

#[test]
fn a_runtime_trap_returns_through_each_c_callback_guard_before_exiting_the_task() {
    use super::{aura_direct_ffi_callback_check, aura_direct_ffi_callback_guard};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    unsafe extern "C-unwind" fn fail(_: *mut u64) {
        super::task_runtime_boundary(|| super::runtime_error("callback trapped"));
    }
    unsafe extern "C-unwind" fn nested(frame: *mut u64) {
        unsafe { aura_direct_ffi_callback_guard(fail, frame) };
        // The inner foreign call returned and can finish its own cleanup.
        unsafe { frame.write(1) };
        aura_direct_ffi_callback_check();
    }

    let returned = Arc::new(AtomicBool::new(false));
    let from_c = Arc::clone(&returned);
    let diagnostic = capture_diagnostic(move || {
        let mut frame = 0;
        unsafe { aura_direct_ffi_callback_guard(nested, &mut frame) };
        assert_eq!(frame, 1);
        from_c.store(true, Ordering::Relaxed);
        aura_direct_ffi_callback_check();
    });
    assert!(returned.load(Ordering::Relaxed));
    assert_eq!(diagnostic.message, "callback trapped");
}

#[test]
fn foreign_thread_callback_enables_stack_guard_and_releases_owned_handles() {
    use super::aura_direct_ffi_callback_guard;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static OBSERVED_STACK_LIMIT: AtomicUsize = AtomicUsize::new(0);
    static TARGET_ADDR: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C-unwind" fn callback_on_foreign_thread(_frame: *mut u64) {
        let limit =
            super::super::with_direct_task_runtime_state(|state| state.frames().stack_limit());
        OBSERVED_STACK_LIMIT.store(limit, Ordering::SeqCst);
        let name = "ForeignHandle";
        let ptr = TARGET_ADDR.load(Ordering::SeqCst) as *mut c_void;
        let _leaked_into_ledger =
            unsafe { aura_direct_ffi_handle(ptr, name.as_ptr(), name.len(), Some(note_release)) };
        // Simulate a trap mid-callback on the foreign thread.
        super::task_runtime_boundary(|| super::runtime_error("foreign callback trapped"));
    }

    let target = 0u8;
    let target_ptr = &target as *const u8 as *mut c_void;
    OBSERVED_STACK_LIMIT.store(0, Ordering::SeqCst);
    TARGET_ADDR.store(target_ptr as usize, Ordering::SeqCst);

    let thread = std::thread::spawn(|| {
        let mut frame = 0u64;
        unsafe { aura_direct_ffi_callback_guard(callback_on_foreign_thread, &mut frame) };
    });
    thread.join().expect("foreign thread should finish cleanly");

    assert_ne!(
        OBSERVED_STACK_LIMIT.load(Ordering::SeqCst),
        0,
        "foreign-thread callback must install a non-zero stack guard limit"
    );
    assert_eq!(
        releases_of(target_ptr),
        1,
        "owned handle created in a foreign-thread callback must be released on thread exit"
    );
}

#[test]
fn a_scoped_callback_with_no_text_preserves_its_scalar_arguments() {
    unsafe extern "C-unwind" fn increment(frame: *mut u64) {
        unsafe { *frame += 1 };
    }

    let mut word = 13;
    unsafe {
        super::aura_direct_ffi_callback_local_text_guard(
            increment,
            &mut word,
            0,
            b"increment".as_ptr(),
            9,
        );
    }
    super::aura_direct_ffi_callback_check();
    assert_eq!(word, 14);
}

#[test]
fn scoped_callback_strings_copy_their_input_and_leave_the_scalar_slots_in_place() {
    unsafe extern "C-unwind" fn inspect(frame: *mut u64) {
        let words = unsafe { slice::from_raw_parts_mut(frame, 5) };
        let first = words[0] as *mut OpaqueValue;
        let second = words[2] as *mut OpaqueValue;
        let mut view = ForeignView {
            pointer: std::ptr::null(),
            length: 0,
        };
        unsafe { aura_direct_ffi_str_view(first, &mut view) };
        assert_ne!(view.pointer as u64, words[4]);
        assert_eq!(view.length, 160);
        assert_eq!(rendered(second), "café");
        words[3] = words[1] + view.length as u64;
        // Compiled Aura code can release an argument when it finishes with it.
        unsafe {
            aura_direct_release_value(first);
            aura_direct_release_value(second);
        }
    }

    let text = std::ffi::CString::new("ab".repeat(80)).unwrap();
    let first = text.as_ptr() as u64;
    let mut frame = [first, 7, c"café".as_ptr() as u64, 0, first];
    unsafe {
        super::aura_direct_ffi_callback_local_text_guard(
            inspect,
            frame.as_mut_ptr(),
            0b101,
            b"inspect".as_ptr(),
            7,
        )
    };
    super::aura_direct_ffi_callback_check();
    assert_eq!(frame[3], 167);
}

#[test]
fn a_text_callback_can_borrow_its_argument_for_codecs_and_return_an_owned_digest() {
    use super::super::{
        aura_direct_arg_buffer_new, aura_direct_arg_buffer_store, aura_host_bytes_base64_decode,
        aura_host_bytes_base64_encode, aura_host_bytes_hex_decode, aura_host_bytes_hex_encode,
        aura_host_bytes_sha256, aura_host_bytes_sha256_string, aura_host_str_from_bytes,
        aura_host_str_to_bytes, value_ref,
    };
    use crate::runtime_value::result_ok;

    fn call(
        builtin: extern "C-unwind" fn(*mut i64, i64) -> *mut OpaqueValue,
        argument: *mut OpaqueValue,
    ) -> *mut OpaqueValue {
        let args = aura_direct_arg_buffer_new(1);
        aura_direct_arg_buffer_store(args, 0, argument as i64);
        builtin(args, 1)
    }

    unsafe extern "C-unwind" fn encode(frame: *mut u64) {
        let words = unsafe { slice::from_raw_parts_mut(frame, 2) };
        let text = words[0] as *mut OpaqueValue;
        let bytes = call(aura_host_str_to_bytes, text);
        let from_bytes = call(aura_host_str_from_bytes, bytes);
        assert_eq!(
            unsafe { value_ref(from_bytes) },
            result_ok(Value::string("café"))
        );

        let hex = call(aura_host_bytes_hex_encode, bytes);
        let base64 = call(aura_host_bytes_base64_encode, bytes);
        assert_eq!(rendered(hex), "636166c3a9");
        assert_eq!(rendered(base64), "Y2Fmw6k=");
        let from_hex = call(aura_host_bytes_hex_decode, hex);
        let from_base64 = call(aura_host_bytes_base64_decode, base64);
        let expected = result_ok(unsafe { value_ref(bytes) });
        assert_eq!(unsafe { value_ref(from_hex) }, expected);
        assert_eq!(unsafe { value_ref(from_base64) }, expected);

        let text_digest = call(aura_host_bytes_sha256_string, text);
        let byte_digest = call(aura_host_bytes_sha256, bytes);
        assert_eq!(unsafe { value_ref(text_digest) }, unsafe {
            value_ref(byte_digest)
        });
        words[1] = call(aura_host_bytes_hex_encode, byte_digest) as u64;
        for value in [
            bytes,
            from_bytes,
            hex,
            base64,
            from_hex,
            from_base64,
            text_digest,
            byte_digest,
            text,
        ] {
            unsafe { aura_direct_release_value(value) };
        }
    }

    let mut frame = [c"café".as_ptr() as u64, 0];
    unsafe {
        super::aura_direct_ffi_callback_local_text_guard(
            encode,
            frame.as_mut_ptr(),
            1,
            b"encode".as_ptr(),
            6,
        );
    }
    super::aura_direct_ffi_callback_check();
    let digest = frame[1] as *mut OpaqueValue;
    assert_eq!(
        rendered(digest),
        "850f7dc43910ff890f8879c0ed26fe697c93a067ad93a7d50f466a7028a9bf4e"
    );
    unsafe { aura_direct_release_value(digest) };
}

#[test]
fn nested_scoped_callbacks_keep_the_outer_string_until_the_body_returns() {
    unsafe extern "C-unwind" fn compare(frame: *mut u64) {
        let words = unsafe { slice::from_raw_parts_mut(frame, 3) };
        assert_ne!(words[0], words[1]);
        assert_eq!(rendered(words[0] as *mut OpaqueValue), "nested");
        assert_eq!(rendered(words[1] as *mut OpaqueValue), "outer");
        words[2] = 1;
    }
    unsafe extern "C-unwind" fn outer(frame: *mut u64) {
        let words = unsafe { slice::from_raw_parts_mut(frame, 2) };
        let mut nested = [c"nested".as_ptr() as u64, words[0], 0];
        unsafe {
            super::aura_direct_ffi_callback_local_text_guard(
                compare,
                nested.as_mut_ptr(),
                1,
                b"compare".as_ptr(),
                7,
            )
        };
        super::aura_direct_ffi_callback_check();
        assert_eq!(rendered(words[0] as *mut OpaqueValue), "outer");
        words[1] = nested[2];
    }
    for _ in 0..2 {
        let mut frame = [c"outer".as_ptr() as u64, 0];
        unsafe {
            super::aura_direct_ffi_callback_local_text_guard(
                outer,
                frame.as_mut_ptr(),
                1,
                b"outer".as_ptr(),
                5,
            )
        };
        super::aura_direct_ffi_callback_check();
        assert_eq!(frame[1], 1);
    }
}

#[test]
fn a_callback_copies_the_c_text_it_receives() {
    let function = "show";
    unsafe {
        let text =
            aura_direct_ffi_callback_text(c"caf\u{e9}".as_ptr(), function.as_ptr(), function.len());
        assert_eq!(rendered(text), "caf\u{e9}");
        with_value(text, |value| {
            let Value::String(text) = value else {
                panic!("expected callback text")
            };
            assert_eq!(text.capacity(), crate::text::INLINE_CAPACITY);
        });
        let retained = aura_direct_retain_value(text);
        aura_direct_release_value(text);
        aura_direct_ffi_release_callback_text(text);
        // A value retained by the Aura function outlives the callback.
        assert_eq!(rendered(retained), "caf\u{e9}");
        aura_direct_release_value(retained);
        aura_direct_ffi_release_callback_text(std::ptr::null_mut());
    }

    type Work = fn();
    let failures: [(Work, &str); 2] = [
        (
            || unsafe {
                aura_direct_ffi_callback_text(std::ptr::null(), "show".as_ptr(), 4);
            },
            "the C caller passed a null pointer as a cstr",
        ),
        (
            || unsafe {
                aura_direct_ffi_callback_text(c"\xff".as_ptr(), "show".as_ptr(), 4);
            },
            "the C caller passed a cstr that is not valid UTF-8",
        ),
    ];
    for (work, expected) in failures {
        let diagnostic = capture_diagnostic(work);
        assert_eq!(diagnostic.code, "AU4005", "{}", diagnostic.message);
        assert!(
            diagnostic.message.contains(expected) && diagnostic.message.contains("`show`"),
            "{}",
            diagnostic.message
        );
    }
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
    let targets = [0_u8; 5];
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

        // A handle a foreign function took is not the program's to release,
        // and reading its address or giving it up a second time traps.
        let taken = open(1);
        let taken_addr = taken as usize;
        aura_direct_ffi_give_up_handle(taken);
        let diagnostic = capture_diagnostic(move || {
            let _ = aura_direct_ffi_handle_address(taken_addr as *mut OpaqueValue);
        });
        assert!(
            diagnostic.message.contains("already been released"),
            "{}",
            diagnostic.message
        );
        let second = capture_diagnostic(move || {
            aura_direct_ffi_give_up_handle(taken_addr as *mut OpaqueValue);
        });
        assert!(
            second.message.contains("already been released"),
            "{}",
            second.message
        );
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
}

#[test]
fn an_array_lends_its_elements_as_a_pointer() {
    use crate::runtime_value::{ArrayStorage, ArrayValue};
    const INT64: i64 = 2;
    const INT32: i64 = 8;
    let array_of = |pointer: *mut OpaqueValue| unsafe {
        with_value(pointer, |value| match value {
            Value::Array(array) => array.try_clone().unwrap(),
            other => panic!("expected an Array, found {other:?}"),
        })
    };
    unsafe {
        let grid = ArrayValue::new(
            vec![2, 3].into(),
            ArrayStorage::Float64(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0].into()),
        )
        .unwrap();
        let boxed = boxed_value(Value::Array(grid.try_clone().unwrap()));
        let mut view = empty_view();
        let copy = aura_direct_ffi_elements(boxed, FLOAT64, &mut view);
        assert!(copy.is_null(), "a row-major Array lends its own elements");
        assert_eq!(view.length, 6);
        assert_eq!(
            slice::from_raw_parts(view.pointer.cast::<f64>(), 6),
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
        );

        // A strided view is packed for a read.
        let turned = boxed_value(Value::Array(grid.transpose().unwrap()));
        let mut view = empty_view();
        let copy = aura_direct_ffi_elements(turned, FLOAT64, &mut view);
        assert!(!copy.is_null(), "a view needs a packed copy");
        assert_eq!(
            slice::from_raw_parts(view.pointer.cast::<f64>(), 6),
            [1.0, 4.0, 2.0, 5.0, 3.0, 6.0]
        );
        aura_direct_release_value(copy);

        // A write goes to this Array alone: the buffer `grid` shares is
        // copied first, and a view becomes one row-major block.
        let mut view = empty_view();
        aura_direct_ffi_elements_mut(boxed, FLOAT64, &mut view);
        assert_eq!(view.length, 6);
        slice::from_raw_parts_mut(view.pointer.cast_mut().cast::<f64>(), 6)[0] = 9.0;
        assert_eq!(array_of(boxed).get(&[0, 0]).unwrap(), Value::Float(9.0));
        assert_eq!(grid.get(&[0, 0]).unwrap(), Value::Float(1.0));
        let mut view = empty_view();
        aura_direct_ffi_elements_mut(turned, FLOAT64, &mut view);
        slice::from_raw_parts_mut(view.pointer.cast_mut().cast::<f64>(), 6)[1] = 8.0;
        assert_eq!(array_of(turned).get(&[0, 1]).unwrap(), Value::Float(8.0));
        assert_eq!(grid.get(&[1, 0]).unwrap(), Value::Float(4.0));
        aura_direct_release_value(turned);
        aura_direct_release_value(boxed);

        for (storage, access) in [
            (ArrayStorage::Int32(vec![1, 2].into()), INT32),
            (ArrayStorage::Int64(vec![1, 2].into()), INT64),
            (ArrayStorage::Float32(vec![1.0, 2.0].into()), FLOAT32),
        ] {
            let boxed = boxed_value(Value::Array(
                ArrayValue::new(vec![2].into(), storage).unwrap(),
            ));
            let mut view = empty_view();
            assert!(aura_direct_ffi_elements(boxed, access, &mut view).is_null());
            assert_eq!(view.length, 2);
            assert!(!view.pointer.is_null());
            let mut view = empty_view();
            aura_direct_ffi_elements_mut(boxed, access, &mut view);
            assert_eq!(view.length, 2);
            aura_direct_release_value(boxed);
        }
    }

    for mutable in [false, true] {
        let message = capture_diagnostic(move || unsafe {
            let flags = boxed_value(Value::Array(
                ArrayValue::new(vec![1].into(), ArrayStorage::Bool(vec![true].into())).unwrap(),
            ));
            let mut view = empty_view();
            match mutable {
                true => aura_direct_ffi_elements_mut(flags, FLOAT64, &mut view),
                false => {
                    aura_direct_ffi_elements(flags, FLOAT64, &mut view);
                }
            }
        })
        .message;
        assert!(
            message.contains(
                "FFI argument expected an Array of another element type, but received `Array[bool]`"
            ),
            "{message}"
        );
    }
}

/// The layout of `struct { bool on; uint8_t bytes[3]; }`, as generated
/// code describes it, and an instance of it.
fn flagged_layout() -> String {
    use crate::c_layout::{CField, CFieldKind, CStruct};
    use crate::ffi::FfiScalar;
    let layout = CStruct {
        name: "Flagged".to_string(),
        fields: vec![
            CField {
                name: "on".to_string(),
                kind: CFieldKind::Scalar(FfiScalar::Bool),
            },
            CField {
                name: "bytes".to_string(),
                kind: CFieldKind::Array(Box::new(CFieldKind::Scalar(FfiScalar::U8)), 3),
            },
        ],
        layout_controls: Default::default(),
        union: false,
    };
    serde_json::to_string(&layout).expect("a layout serializes")
}

fn flagged(on: bool, bytes: &[u8]) -> Value {
    use crate::runtime_value::{InstanceValue, RuntimeName};
    Value::Instance(InstanceValue {
        class_name: RuntimeName::from("Flagged".to_string()),
        fields: [
            ("on".to_string(), Value::Bool(on)),
            (
                "bytes".to_string(),
                Value::Vec(VecValue::new(
                    Type::named("uint8"),
                    ListElements::from_uint8(bytes.to_vec()),
                )),
            ),
        ]
        .into(),
    })
}

fn rendered(value: *mut OpaqueValue) -> String {
    unsafe { with_value(value, |value| value.render()) }
}

#[test]
fn matching_record_layouts_copy_to_private_stack_or_heap_storage() {
    use super::{aura_direct_ffi_copy_structs, FFI_STRUCT_SCRATCH_BYTES};

    let input = [47_u8; FFI_STRUCT_SCRATCH_BYTES + 1];
    let mut scratch = [0_u8; FFI_STRUCT_SCRATCH_BYTES];
    for length in [0, 1, FFI_STRUCT_SCRATCH_BYTES, input.len()] {
        unsafe {
            let copy = aura_direct_ffi_copy_structs(input.as_ptr(), length, scratch.as_mut_ptr());
            if length == 0 {
                assert!(copy.is_null());
                continue;
            }
            assert_eq!(copy == scratch.as_mut_ptr(), length <= scratch.len());
            assert_eq!(slice::from_raw_parts(copy, length), &input[..length]);
            copy.write(19);
            assert_eq!(input[0], 47);
            if copy != scratch.as_mut_ptr() {
                aura_direct_ffi_release_structs(copy);
            }
        }
    }
}

#[test]
fn a_struct_with_an_array_field_is_packed_and_unpacked_by_the_runtime() {
    let descriptor = flagged_layout();
    let symbol = "probe";
    unsafe {
        let value = boxed_value(flagged(true, &[7, 8, 9]));
        let mut slot = [0_u8; 8];
        aura_direct_ffi_pack_struct(
            value,
            descriptor.as_ptr(),
            descriptor.len(),
            slot.as_mut_ptr(),
        );
        assert_eq!(&slot[..4], &[1, 7, 8, 9]);
        // The same descriptor is read once and found again.
        aura_direct_ffi_pack_struct(
            value,
            descriptor.as_ptr(),
            descriptor.len(),
            slot.as_mut_ptr(),
        );

        let back = aura_direct_ffi_unpack_struct(
            descriptor.as_ptr(),
            descriptor.len(),
            slot.as_ptr(),
            symbol.as_ptr(),
            symbol.len(),
        );
        assert_eq!(rendered(back), rendered(value));
        aura_direct_release_value(back);

        // What C wrote through a pointer lands in the instance passed.
        let written = [0_u8, 1, 2, 3];
        aura_direct_ffi_unpack_struct_into(
            value,
            descriptor.as_ptr(),
            descriptor.len(),
            written.as_ptr(),
            symbol.as_ptr(),
            symbol.len(),
        );
        assert_eq!(rendered(value), flagged(false, &[1, 2, 3]).render());
        aura_direct_release_value(value);
    }

    type Work = fn(&str);
    let failures: [(Work, &str, &str); 6] = [
        (
            |descriptor| unsafe {
                let value = boxed_value(flagged(true, &[7]));
                let mut slot = [0_u8; 8];
                aura_direct_ffi_pack_struct(
                    value,
                    descriptor.as_ptr(),
                    descriptor.len(),
                    slot.as_mut_ptr(),
                );
            },
            "AU4005",
            "C array field `bytes` holds 3 element(s), but the list has 1",
        ),
        (
            |descriptor| unsafe {
                let slot = [2_u8, 0, 0, 0];
                aura_direct_ffi_unpack_struct(
                    descriptor.as_ptr(),
                    descriptor.len(),
                    slot.as_ptr(),
                    "probe".as_ptr(),
                    5,
                );
            },
            "AU4001",
            "FFI call to `probe` failed",
        ),
        (
            |descriptor| unsafe {
                let value = boxed_value(flagged(true, &[7, 8, 9]));
                let slot = [3_u8, 0, 0, 0];
                aura_direct_ffi_unpack_struct_into(
                    value,
                    descriptor.as_ptr(),
                    descriptor.len(),
                    slot.as_ptr(),
                    "probe".as_ptr(),
                    5,
                );
            },
            "AU4001",
            "FFI call to `probe` failed",
        ),
        (
            |descriptor| unsafe {
                let slot = [0_u8; 4];
                aura_direct_ffi_unpack_struct_into(
                    string_value("x"),
                    descriptor.as_ptr(),
                    descriptor.len(),
                    slot.as_ptr(),
                    "probe".as_ptr(),
                    5,
                );
            },
            "",
            "FFI argument expected a class instance, but received `str`",
        ),
        (
            |_| unsafe {
                let text = "not a layout";
                let mut slot = [0_u8; 8];
                aura_direct_ffi_pack_struct(
                    string_value("x"),
                    text.as_ptr(),
                    text.len(),
                    slot.as_mut_ptr(),
                );
            },
            "",
            "a foreign call names a C layout that does not read",
        ),
        (
            |descriptor| unsafe {
                aura_direct_ffi_pack_structs(
                    string_value("x"),
                    descriptor.as_ptr(),
                    descriptor.len(),
                );
            },
            "AU4005",
            "FFI value for C type `list[Flagged]` has incompatible runtime shape",
        ),
    ];
    for (work, code, expected) in failures {
        let descriptor = descriptor.clone();
        let diagnostic = capture_diagnostic(move || work(&descriptor));
        if !code.is_empty() {
            assert_eq!(diagnostic.code, code, "{}", diagnostic.message);
        }
        assert!(
            diagnostic.message.contains(expected),
            "{}",
            diagnostic.message
        );
    }
}

#[test]
fn an_invalid_c_writeback_releases_its_array_with_the_failed_task() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static FREED: AtomicBool = AtomicBool::new(false);
    unsafe extern "C" fn release(block: *mut c_void) {
        FREED.store(true, Ordering::Relaxed);
        unsafe { super::free_structs_block(block) };
    }
    unsafe extern "C-unwind" fn fail(_args: *const i64, _count: usize) -> *mut OpaqueValue {
        let descriptor = flagged_layout();
        let values = boxed_value(Value::Vec(VecValue::new(
            Type::named("Flagged"),
            ListElements::from(vec![flagged(true, &[1, 2, 3])]),
        )));
        let array = super::allocate_structs(4, release);
        unsafe {
            array.copy_from_nonoverlapping([9_u8, 1, 2, 3].as_ptr(), 4);
            aura_direct_ffi_unpack_structs(
                values,
                descriptor.as_ptr(),
                descriptor.len(),
                array,
                b"probe".as_ptr(),
                5,
            );
        }
        panic!("the corrupt boolean should trap")
    }
    let diagnostic =
        super::super::run_direct_root_task(fail).expect_err("the C writeback should trap");
    assert_eq!(diagnostic.code, "AU4001");
    assert!(
        FREED.load(Ordering::Relaxed),
        "the failed task must release the C array"
    );
}

#[test]
fn a_mutable_struct_array_keeps_native_field_access_after_c_writes_it() {
    use crate::native_runtime::{register_class_layout, TypedFields, FIRST_CLASS_ACCESS};
    use crate::runtime_value::InstanceFields;
    use aura_core::layout::{encode_descriptor, ClassDefinition, LayoutEnv};

    let class = Type::named("Flagged");
    let env = LayoutEnv {
        classes: [(
            "Flagged".to_owned(),
            ClassDefinition {
                type_params: Vec::new(),
                fields: vec![
                    ("on".to_owned(), Type::named("bool")),
                    (
                        "bytes".to_owned(),
                        Type::Named("list".into(), vec![Type::named("uint8")]),
                    ),
                ],
            },
        )]
        .into(),
        ..Default::default()
    };
    let native = encode_descriptor(&class, &env).expect("a native class layout");
    let native = register_class_layout(&native, FIRST_CLASS_ACCESS + 0x1100_0010).unwrap();
    let Value::Instance(mut input) = flagged(true, &[1, 2, 3]) else {
        unreachable!()
    };
    let fields = std::mem::take(input.fields.map_mut());
    input.fields = InstanceFields::Typed(TypedFields::from_map(native, fields).unwrap());
    let mut elements = ListElements::from(vec![Value::Instance(input)]);
    elements.lend(0).expect("the caller borrows this element");
    let values = boxed_value(Value::Vec(VecValue::new(class, elements)));
    let descriptor = flagged_layout();
    let symbol = b"set";
    unsafe {
        let array = aura_direct_ffi_pack_structs(values, descriptor.as_ptr(), descriptor.len());
        array.copy_from_nonoverlapping([0_u8, 4, 5, 6].as_ptr(), 4);
        aura_direct_ffi_unpack_structs(
            values,
            descriptor.as_ptr(),
            descriptor.len(),
            array,
            symbol.as_ptr(),
            symbol.len(),
        );
        with_value(values, |value| {
            let Value::Vec(list) = value else {
                unreachable!()
            };
            assert!(matches!(list.elements, ListElements::Boxes(_)));
            let first = list.elements.first().unwrap();
            let Value::Instance(item) = &*first else {
                unreachable!()
            };
            assert_eq!(
                item.fields.typed().unwrap().layout().access(),
                native.access()
            );
            assert_eq!(*first, flagged(false, &[4, 5, 6]));
        });
        aura_direct_ffi_release_structs(array);
        aura_direct_release_value(values);
    }
}

#[test]
fn a_list_of_structs_crosses_as_a_c_array_the_runtime_owns() {
    let descriptor = flagged_layout();
    let symbol = "probe";
    let list = |elements: Vec<Value>| {
        boxed_value(Value::Vec(VecValue::new(
            Type::named("Flagged"),
            ListElements::from(elements),
        )))
    };
    unsafe {
        let values = list(vec![flagged(true, &[1, 2, 3]), flagged(false, &[4, 5, 6])]);
        let array = aura_direct_ffi_pack_structs(values, descriptor.as_ptr(), descriptor.len());
        assert_eq!(slice::from_raw_parts(array, 8), &[1, 1, 2, 3, 0, 4, 5, 6]);
        // C rewrites the array, and the list takes what it left.
        array.copy_from_nonoverlapping([0_u8, 9, 9, 9, 1, 8, 8, 8].as_ptr(), 8);
        aura_direct_ffi_unpack_structs(
            values,
            descriptor.as_ptr(),
            descriptor.len(),
            array,
            symbol.as_ptr(),
            symbol.len(),
        );
        let expected = list(vec![flagged(false, &[9, 9, 9]), flagged(true, &[8, 8, 8])]);
        assert_eq!(rendered(values), rendered(expected));
        aura_direct_ffi_release_structs(array);
        aura_direct_release_value(expected);
        aura_direct_release_value(values);

        // An empty list is a null pointer, which needs no release.
        let empty = list(Vec::new());
        let array = aura_direct_ffi_pack_structs(empty, descriptor.as_ptr(), descriptor.len());
        assert!(array.is_null());
        aura_direct_ffi_unpack_structs(
            empty,
            descriptor.as_ptr(),
            descriptor.len(),
            array,
            symbol.as_ptr(),
            symbol.len(),
        );
        aura_direct_ffi_release_structs(array);
        aura_direct_release_value(empty);
    }

    let bad_bool = descriptor.clone();
    let diagnostic = capture_diagnostic(move || unsafe {
        let values = boxed_value(Value::Vec(VecValue::new(
            Type::named("Flagged"),
            ListElements::from(vec![flagged(true, &[1, 2, 3])]),
        )));
        let array = aura_direct_ffi_pack_structs(values, bad_bool.as_ptr(), bad_bool.len());
        array.write(9);
        aura_direct_ffi_unpack_structs(
            values,
            bad_bool.as_ptr(),
            bad_bool.len(),
            array,
            "probe".as_ptr(),
            5,
        );
    });
    assert_eq!(diagnostic.code, "AU4001");
    let not_a_list = descriptor.clone();
    let diagnostic = capture_diagnostic(move || unsafe {
        let values = boxed_value(Value::Vec(VecValue::new(
            Type::named("Flagged"),
            ListElements::from(vec![flagged(true, &[1, 2, 3])]),
        )));
        let array = aura_direct_ffi_pack_structs(values, not_a_list.as_ptr(), not_a_list.len());
        aura_direct_ffi_unpack_structs(
            string_value("x"),
            not_a_list.as_ptr(),
            not_a_list.len(),
            array,
            "probe".as_ptr(),
            5,
        );
    });
    assert!(
        diagnostic
            .message
            .contains("FFI argument expected a list for a mut pointer, but received `str`"),
        "{}",
        diagnostic.message
    );
}

/// The descriptor generated code passes for a `fixed[uint8, 3]` value: a
/// layout whose one field is the array.
fn fixed_bytes_layout() -> String {
    use crate::c_layout::{CField, CFieldKind, CStruct};
    use crate::ffi::FfiScalar;
    let layout = CStruct {
        name: "fixed".to_string(),
        fields: vec![CField {
            name: "elements".to_string(),
            kind: CFieldKind::Array(Box::new(CFieldKind::Scalar(FfiScalar::U8)), 3),
        }],
        layout_controls: Default::default(),
        union: false,
    };
    serde_json::to_string(&layout).expect("a layout serializes")
}

#[test]
fn a_fixed_array_boxes_from_its_bytes_and_unboxes_back_into_a_slot() {
    use super::{aura_direct_fixed_box, aura_direct_fixed_unbox};

    let descriptor = fixed_bytes_layout();
    let bytes = [4_u8, 5, 6];
    unsafe {
        let value = aura_direct_fixed_box(descriptor.as_ptr(), descriptor.len(), bytes.as_ptr());
        assert_eq!(rendered(value), "[4, 5, 6]");
        let mut slot = [0_u8; 3];
        aura_direct_fixed_unbox(
            value,
            descriptor.as_ptr(),
            descriptor.len(),
            slot.as_mut_ptr(),
        );
        assert_eq!(slot, bytes);
        aura_direct_release_value(value);
    }

    // A value that is not a list of the element ends the task.
    let diagnostic = capture_diagnostic(move || unsafe {
        let descriptor = fixed_bytes_layout();
        let text = string_value("abc");
        let mut slot = [0_u8; 3];
        aura_direct_fixed_unbox(
            text,
            descriptor.as_ptr(),
            descriptor.len(),
            slot.as_mut_ptr(),
        );
    });
    assert_eq!(diagnostic.code, "AU4005", "{}", diagnostic.message);
}

#[test]
fn a_fixed_array_index_out_of_bounds_ends_the_task_at_its_span() {
    let diagnostic = capture_diagnostic(|| super::aura_direct_fixed_bounds(7, 3, 12, 5));
    assert_eq!(diagnostic.code, "AU4031");
    assert_eq!(
        diagnostic.message,
        "fixed array index `7` is out of bounds for length `3`"
    );
    let span = diagnostic.span.expect("the trap names its span");
    assert_eq!((span.line, span.column), (12, 5));
}

#[test]
fn wide_pointer_reads_and_writes_move_all_128_bits() {
    use super::{aura_direct_pointer_wide_read, aura_direct_pointer_wide_write};

    let mut cell = 0_u128;
    let address = &mut cell as *mut u128;
    unsafe {
        let value = boxed_value(Value::Int(
            IntegerValue::from_typed_signed(-2, IntegerKind::Int128).expect("an int128"),
        ));
        aura_direct_pointer_wide_write(address, value, 0);
        aura_direct_release_value(value);
        assert_eq!(cell, u128::MAX - 1);

        let signed = aura_direct_pointer_wide_read(address, 1, 0);
        assert_eq!(rendered(signed), "-2");
        aura_direct_release_value(signed);
        let unsigned = aura_direct_pointer_wide_read(address, 0, 1);
        assert_eq!(rendered(unsigned), (u128::MAX - 1).to_string());
        aura_direct_release_value(unsigned);

        let value = boxed_value(Value::Int(
            IntegerValue::from_typed_unsigned(u128::MAX, IntegerKind::Uint128).expect("a uint128"),
        ));
        aura_direct_pointer_wide_write(address, value, 1);
        aura_direct_release_value(value);
        assert_eq!(cell, u128::MAX);
    }

    // Only an integer can be written.
    let target = Box::into_raw(Box::new(0_u128)) as usize;
    let diagnostic = capture_diagnostic(move || unsafe {
        let flag = boxed_value(Value::Bool(true));
        aura_direct_pointer_wide_write(target as *mut u128, flag, 0);
    });
    assert_eq!(diagnostic.code, "AU4032");
    assert_eq!(diagnostic.message, "wide memory write needs an integer");
    drop(unsafe { Box::from_raw(target as *mut u128) });
}
