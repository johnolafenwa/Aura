#![cfg(test)]

use super::{
    boxed_value, compare_values, current_cancellation, decode_bytes, eval_binary_value,
    eval_unary_value, extract_duration_millis, inferred_collection_type, int32_overflow_message,
    normalize_vec_index, render_bool, render_float, render_runtime_diagnostic, runtime_span,
    value_mut, value_ref, value_type_name, with_cancellation_scope, OpaqueValue,
};
use crate::ast::{BinaryOp, UnaryOp};
use crate::diag::{Diagnostic, Span};
use crate::integer::{IntegerKind, IntegerRepresentation, IntegerValue};
use crate::runtime_value::{
    run_lightweight_root_task, spawn_lightweight_task, CancellationContext, ChannelValue,
    EnumVariantValue, FileValue, HttpListenerValue, HttpResponseValue, InstanceValue,
    LightweightTaskFailureSignal, MapValue, ModuleNamespaceValue, ProcessChildValue,
    ProcessCompletedValue, ProcessStdioConfig, ProcessSupervisorValue, RangeValue, SetValue,
    TaskCancelledSignal, TaskGroupValue, TaskValue, TaskWaitStatus, TcpListenerValue,
    TcpStreamValue, TlsListenerValue, TlsStreamValue, UdpDatagramValue, UdpSocketValue,
    UnixListenerValue, UnixStreamValue, Value, VecValue, WebSocketListenerValue, WebSocketValue,
};
use crate::sema::Type;
use rcgen::generate_simple_self_signed;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::panic;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

fn string_value(text: &str) -> *mut OpaqueValue {
    super::aurora_direct_string_literal(text.as_ptr(), text.len())
}

fn int_value(value: i64) -> *mut OpaqueValue {
    super::aurora_direct_box_i64(value)
}

fn float_value(value: f64) -> *mut OpaqueValue {
    super::aurora_direct_box_f64(value)
}

fn bool_value(value: bool) -> *mut OpaqueValue {
    super::aurora_direct_box_bool(i64::from(value))
}

fn duration_value(value: i64) -> *mut OpaqueValue {
    super::aurora_direct_duration_literal(value)
}

fn string_vec(values: &[&str]) -> *mut OpaqueValue {
    let vec = super::aurora_direct_vec_empty();
    for value in values {
        super::aurora_direct_vec_push_in_place(vec, string_value(value));
    }
    vec
}

fn int_vec(values: &[i64]) -> *mut OpaqueValue {
    let vec = super::aurora_direct_vec_empty();
    for value in values {
        let value = u8::try_from(*value).expect("test byte vectors only contain uint8 values");
        super::aurora_direct_vec_push_in_place(
            vec,
            boxed_value(Value::Int(
                IntegerValue::from_typed_unsigned(value as u128, IntegerKind::Uint8)
                    .expect("every byte fits the uint8 runtime kind"),
            )),
        );
    }
    vec
}

fn task_vec(tasks: &[TaskValue]) -> *mut OpaqueValue {
    let vec = super::aurora_direct_vec_empty();
    for task in tasks {
        expect_unit(super::aurora_direct_vec_push_in_place(
            vec,
            boxed_value(Value::Task(task.clone())),
        ));
    }
    vec
}

unsafe fn free_arg_buffer(buffer: *mut i64, count: usize) {
    let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(buffer, count));
    drop(boxed);
}

unsafe fn take_value(ptr: *mut OpaqueValue) -> Value {
    super::take_value(ptr)
}

unsafe fn retain_value(ptr: *mut OpaqueValue) -> *mut OpaqueValue {
    super::aurora_direct_retain_value(ptr)
}

unsafe fn release_value(ptr: *mut OpaqueValue) {
    super::aurora_direct_release_value(ptr)
}

fn expect_unit(ptr: *mut OpaqueValue) {
    match unsafe { take_value(ptr) } {
        Value::Unit => {}
        other => panic!("expected unit, found {:?}", other),
    }
}

fn expect_string(ptr: *mut OpaqueValue) -> String {
    match unsafe { take_value(ptr) } {
        Value::String(text) => text,
        other => panic!("expected string, found {:?}", other),
    }
}

fn expect_int(ptr: *mut OpaqueValue) -> i128 {
    match unsafe { take_value(ptr) } {
        Value::Int(value) => value.as_i128().expect("expected signed integer"),
        other => panic!("expected int, found {:?}", other),
    }
}

fn expect_task_result_ready_int(ptr: *mut OpaqueValue) -> i128 {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "TaskResult" && variant.variant_name == "Ready" =>
        {
            match variant
                .single_payload()
                .expect("expected task result payload")
            {
                Value::Int(value) => value.as_i128().expect("expected signed integer"),
                other => panic!("expected int payload, found {:?}", other),
            }
        }
        other => panic!("expected TaskResult.Ready(int), found {:?}", other),
    }
}

fn expect_task_result_error_message(ptr: *mut OpaqueValue) -> String {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "TaskResult" && variant.variant_name == "Error" =>
        {
            match variant
                .single_payload()
                .expect("expected task result payload")
            {
                Value::String(text) => text.clone(),
                other => panic!("expected string payload, found {:?}", other),
            }
        }
        other => panic!("expected TaskResult.Error(String), found {:?}", other),
    }
}

fn expect_float(ptr: *mut OpaqueValue) -> f64 {
    match unsafe { take_value(ptr) } {
        Value::Float(value) => value,
        other => panic!("expected float, found {:?}", other),
    }
}

fn expect_bool_boxed(ptr: *mut OpaqueValue) -> bool {
    match unsafe { take_value(ptr) } {
        Value::Bool(value) => value,
        other => panic!("expected bool, found {:?}", other),
    }
}

fn expect_vec_ints(ptr: *mut OpaqueValue) -> Vec<i128> {
    match unsafe { take_value(ptr) } {
        Value::Vec(values) => values
            .elements
            .into_iter()
            .map(|value| match value {
                Value::Int(value) => value.as_i128().expect("expected signed integer"),
                other => panic!("expected int element, found {:?}", other),
            })
            .collect(),
        other => panic!("expected vec, found {:?}", other),
    }
}

fn expect_vec_strings(ptr: *mut OpaqueValue) -> Vec<String> {
    match unsafe { take_value(ptr) } {
        Value::Vec(values) => values
            .elements
            .into_iter()
            .map(|value| match value {
                Value::String(text) => text.to_string(),
                other => panic!("expected string element, found {:?}", other),
            })
            .collect(),
        other => panic!("expected vec, found {:?}", other),
    }
}

fn expect_option_some_int(ptr: *mut OpaqueValue) -> i128 {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "Some" =>
        {
            match variant.single_payload().expect("expected option payload") {
                Value::Int(value) => value.as_i128().expect("expected signed integer"),
                other => panic!("expected int payload, found {:?}", other),
            }
        }
        other => panic!("expected Option.Some(int), found {:?}", other),
    }
}

fn expect_option_some_string(ptr: *mut OpaqueValue) -> String {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "Some" =>
        {
            match variant.single_payload().expect("expected option payload") {
                Value::String(text) => text.to_string(),
                other => panic!("expected string payload, found {:?}", other),
            }
        }
        other => panic!("expected Option.Some(String), found {:?}", other),
    }
}

fn assert_value_metadata(value: &Value, display_name: &str, type_name: &str) {
    assert_eq!(value_type_name(value), display_name);
    assert_eq!(
        inferred_collection_type(value),
        crate::sema::Type::named(type_name)
    );
}

fn assert_direct_type_match(value: Value, type_name: &str) {
    let ptr = boxed_value(value);
    assert_eq!(
        super::aurora_direct_value_type_matches(ptr, type_name.as_ptr(), type_name.len()),
        1
    );
    let _ = unsafe { take_value(ptr) };
}

fn close_via_direct(value: Value) {
    let ptr = boxed_value(value);
    expect_unit(super::aurora_direct_close_value(ptr, 0));
    unsafe { release_value(ptr) };
}

#[test]
fn direct_host_builtin_ffi_covers_success_and_diagnostic_boundaries() {
    let empty_args = super::aurora_direct_arg_buffer_new(0);
    let value =
        super::aurora_direct_host_builtin(b"sys::args".as_ptr(), "sys::args".len(), empty_args, 0);
    assert!(matches!(unsafe { take_value(value) }, Value::Vec(_)));

    let unknown = run_lightweight_root_task(|| {
        super::with_task_runtime_error_capture(|| {
            let empty_args = super::aurora_direct_arg_buffer_new(0);
            let _ = super::aurora_direct_host_builtin(
                b"missing::call".as_ptr(),
                "missing::call".len(),
                empty_args,
                0,
            );
            Ok(Value::Unit)
        })
    })
    .expect_err("unknown host builtins should fail the active task");
    assert!(unknown.message.contains("unknown host builtin"));

    let invalid_count = run_lightweight_root_task(|| {
        super::with_task_runtime_error_capture(|| {
            let _ = super::aurora_direct_host_builtin(
                b"sys::args".as_ptr(),
                "sys::args".len(),
                std::ptr::null_mut(),
                -1,
            );
            Ok(Value::Unit)
        })
    })
    .expect_err("invalid host builtin argument counts should fail the active task");
    assert!(invalid_count
        .message
        .contains("invalid host builtin argument count"));
}

fn capture_runtime_error_message(f: impl FnOnce() + panic::UnwindSafe) -> String {
    let payload = panic::catch_unwind(|| super::with_task_runtime_error_capture(f))
        .expect_err("runtime error should be captured as a panic");
    payload
        .downcast_ref::<crate::runtime_value::LightweightTaskFailureSignal>()
        .map(|signal| signal.0.message.clone())
        .unwrap_or_else(|| panic!("unexpected panic payload"))
}

fn expect_option_none(ptr: *mut OpaqueValue) {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "None" => {}
        other => panic!("expected Option.None, found {:?}", other),
    }
}

fn expect_variant_value(value: Value, enum_name: &str, variant_name: &str) -> Vec<Value> {
    match value {
        Value::EnumVariant(variant)
            if variant.enum_name == enum_name && variant.variant_name == variant_name =>
        {
            variant.payloads
        }
        other => panic!("expected {}.{}, found {:?}", enum_name, variant_name, other),
    }
}

fn expect_variant_ptr(ptr: *mut OpaqueValue, enum_name: &str, variant_name: &str) -> Vec<Value> {
    expect_variant_value(unsafe { take_value(ptr) }, enum_name, variant_name)
}

fn expect_queue_receive_item_int(ptr: *mut OpaqueValue) -> i128 {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "QueueReceive" && variant.variant_name == "Item" =>
        {
            match variant
                .single_payload()
                .expect("expected queue receive payload")
            {
                Value::Int(value) => value.as_i128().expect("expected signed integer"),
                other => panic!("expected int payload, found {:?}", other),
            }
        }
        other => panic!("expected QueueReceive.Item(int), found {:?}", other),
    }
}

fn expect_queue_receive_closed(ptr: *mut OpaqueValue) {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "QueueReceive" && variant.variant_name == "Closed" => {}
        other => panic!("expected QueueReceive.Closed, found {:?}", other),
    }
}

fn expect_result_ok_int(ptr: *mut OpaqueValue) -> i128 {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Ok" =>
        {
            match variant.single_payload().expect("expected result payload") {
                Value::Int(value) => value.as_i128().expect("expected signed integer"),
                other => panic!("expected int payload, found {:?}", other),
            }
        }
        other => panic!("expected Result.Ok(int), found {:?}", other),
    }
}

fn expect_result_ok_float(ptr: *mut OpaqueValue) -> f64 {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Ok" =>
        {
            match variant.single_payload().expect("expected result payload") {
                Value::Float(value) => *value,
                other => panic!("expected float payload, found {:?}", other),
            }
        }
        other => panic!("expected Result.Ok(float), found {:?}", other),
    }
}

fn expect_result_ok_unit(ptr: *mut OpaqueValue) {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Ok" =>
        {
            match variant.single_payload().expect("expected result payload") {
                Value::Unit => {}
                other => panic!("expected unit payload, found {:?}", other),
            }
        }
        other => panic!("expected Result.Ok(unit), found {:?}", other),
    }
}

fn expect_result_ok_string(ptr: *mut OpaqueValue) -> String {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Ok" =>
        {
            match variant.single_payload().expect("expected result payload") {
                Value::String(text) => text.to_string(),
                other => panic!("expected string payload, found {:?}", other),
            }
        }
        other => panic!("expected Result.Ok(String), found {:?}", other),
    }
}

fn expect_result_ok_payload(ptr: *mut OpaqueValue) -> Value {
    let mut payloads = expect_variant_ptr(ptr, "Result", "Ok");
    assert_eq!(payloads.len(), 1, "expected one Result.Ok payload");
    payloads.remove(0)
}

fn expect_result_err_payload(ptr: *mut OpaqueValue) -> Value {
    let mut payloads = expect_variant_ptr(ptr, "Result", "Err");
    assert_eq!(payloads.len(), 1, "expected one Result.Err payload");
    payloads.remove(0)
}

fn expect_option_some_payload(value: Value) -> Value {
    match value {
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "Some" =>
        {
            let mut payloads = variant.payloads;
            assert_eq!(payloads.len(), 1, "expected one Option.Some payload");
            payloads.remove(0)
        }
        other => panic!("expected Option.Some(...), found {:?}", other),
    }
}

fn expect_result_ok_vec_ints(ptr: *mut OpaqueValue) -> Vec<i128> {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Ok" =>
        {
            match variant.single_payload().expect("expected result payload") {
                Value::Vec(values) => values
                    .elements
                    .iter()
                    .map(|value| match value {
                        Value::Int(value) => value.as_i128().expect("expected signed integer"),
                        other => panic!("expected int element, found {:?}", other),
                    })
                    .collect(),
                other => panic!("expected vec payload, found {:?}", other),
            }
        }
        other => panic!("expected Result.Ok(Vec[int]), found {:?}", other),
    }
}

fn expect_result_ok_vec_strings(ptr: *mut OpaqueValue) -> Vec<String> {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Ok" =>
        {
            match variant.single_payload().expect("expected result payload") {
                Value::Vec(values) => values
                    .elements
                    .iter()
                    .map(|value| match value {
                        Value::String(text) => text.to_string(),
                        other => panic!("expected string element, found {:?}", other),
                    })
                    .collect(),
                other => panic!("expected vec payload, found {:?}", other),
            }
        }
        other => panic!("expected Result.Ok(Vec[String]), found {:?}", other),
    }
}

fn string_map(entries: &[(&str, &str)]) -> *mut OpaqueValue {
    let map = super::aurora_direct_map_empty();
    for (key, value) in entries {
        expect_option_none(super::aurora_direct_map_set_in_place(
            map,
            string_value(key),
            string_value(value),
        ));
    }
    map
}

fn expect_result_err_string(ptr: *mut OpaqueValue) -> String {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Err" =>
        {
            match variant.single_payload().expect("expected result payload") {
                Value::String(text) => text.to_string(),
                other => panic!("expected string payload, found {:?}", other),
            }
        }
        other => panic!("expected Result.Err(String), found {:?}", other),
    }
}

#[test]
fn render_bool_uses_aurora_boolean_strings() {
    assert_eq!(render_bool(0), "false");
    assert_eq!(render_bool(1), "true");
    assert_eq!(render_bool(99), "true");
}

#[test]
fn int32_overflow_message_mentions_value_and_type() {
    assert_eq!(
        int32_overflow_message(123),
        "integer value `123` does not fit in `int32`"
    );
}

#[test]
fn render_float_preserves_whole_number_fraction() {
    assert_eq!(render_float(42.0), "42.0");
    assert_eq!(render_float(3.5), "3.5");
}

#[test]
fn render_float_hides_float32_roundtrip_noise() {
    let float32_value = (834.5999755859375_f64 as f32) as f64;
    assert_eq!(render_float(float32_value), "834.6");
}

#[test]
fn render_float_covers_nonfinite_and_full_precision_values() {
    assert_eq!(render_float(f64::INFINITY), "inf");
    let precise = std::f64::consts::PI;
    assert_eq!(render_float(precise), precise.to_string());
}

#[test]
fn native_runtime_operator_helpers_cover_comparison_binary_and_unary_error_edges() {
    assert_eq!(
        super::compare_values(
            Value::Int(IntegerValue::from_signed(1)),
            Value::Int(IntegerValue::from_signed(2)),
            BinaryOp::Less,
        )
        .expect("int comparisons should succeed"),
        Value::Bool(true)
    );
    assert_eq!(
        super::compare_values(Value::Float(2.5), Value::Float(2.5), BinaryOp::GreaterEq,)
            .expect("float comparisons should succeed"),
        Value::Bool(true)
    );
    assert_eq!(
        super::compare_values(
            Value::String("ada".to_string()),
            Value::String("grace".to_string()),
            BinaryOp::Less,
        )
        .expect("string ordering should succeed"),
        Value::Bool(true)
    );
    assert_eq!(
        super::compare_values(Value::Unit, Value::Unit, BinaryOp::Eq)
            .expect("unit equality should succeed"),
        Value::Bool(true)
    );
    assert!(super::compare_values(
        Value::Int(IntegerValue::from_signed(1)),
        Value::Int(IntegerValue::from_signed(2)),
        BinaryOp::Add,
    )
    .expect_err("non-comparison int ops should fail in compare_values")
    .message
    .contains("unsupported comparison operator"));
    assert!(
        super::compare_values(Value::Float(1.0), Value::Float(2.0), BinaryOp::Add,)
            .expect_err("non-comparison float ops should fail in compare_values")
            .message
            .contains("unsupported comparison operator")
    );
    assert!(super::compare_values(
        Value::String("a".to_string()),
        Value::String("b".to_string()),
        BinaryOp::Add,
    )
    .expect_err("non-comparison string ops should fail in compare_values")
    .message
    .contains("unsupported comparison operator"));
    assert!(super::compare_values(
        Value::Bool(true),
        Value::String("b".to_string()),
        BinaryOp::Less,
    )
    .expect_err("mismatched comparisons should fail")
    .message
    .contains("unsupported comparison"));

    assert_eq!(
        super::eval_binary_value(Value::Bool(true), Value::Bool(false), BinaryOp::And)
            .expect("bool and should succeed"),
        Value::Bool(false)
    );
    assert_eq!(
        super::eval_binary_value(Value::Bool(true), Value::Bool(false), BinaryOp::Or)
            .expect("bool or should succeed"),
        Value::Bool(true)
    );
    assert!(super::eval_binary_value(
        Value::Bool(true),
        Value::Int(IntegerValue::from_signed(1)),
        BinaryOp::And,
    )
    .expect_err("logical and should reject non-bool rhs")
    .message
    .contains("logical `and` expects bool operands"));
    assert!(super::eval_binary_value(
        Value::Int(IntegerValue::from_signed(1)),
        Value::Bool(false),
        BinaryOp::Or,
    )
    .expect_err("logical or should reject non-bool lhs")
    .message
    .contains("logical `or` expects bool operands"));
    assert_eq!(
        super::eval_binary_value(
            Value::String("aurora".to_string()),
            Value::String(" repo".to_string()),
            BinaryOp::Add,
        )
        .expect("string concat should succeed"),
        Value::String("aurora repo".to_string())
    );
    assert_eq!(
        super::eval_binary_value(Value::Float(9.0), Value::Float(4.0), BinaryOp::Div,)
            .expect("float division should succeed"),
        Value::Float(2.25)
    );
    assert!(
        super::eval_binary_value(Value::Float(9.0), Value::Float(0.0), BinaryOp::Div,)
            .expect_err("float division by zero should fail")
            .message
            .contains("division by zero")
    );
    assert_eq!(
        super::eval_binary_value(Value::Float(9.0), Value::Float(4.0), BinaryOp::Mod,)
            .expect("float modulo should succeed"),
        Value::Float(1.0)
    );
    assert_eq!(
        super::eval_binary_value(
            Value::Int(IntegerValue::from_signed(-7)),
            Value::Int(IntegerValue::from_signed(3)),
            BinaryOp::FloorDiv,
        )
        .expect("integer floor division should round toward negative infinity"),
        Value::Int(IntegerValue::from_signed(-3))
    );
    assert_eq!(
        super::eval_binary_value(Value::Float(7.5), Value::Float(-2.0), BinaryOp::FloorDiv,)
            .expect("float floor division should round toward negative infinity"),
        Value::Float(-4.0)
    );
    assert_eq!(
        super::eval_binary_value(
            Value::Int(IntegerValue::from_signed(1)),
            Value::Int(IntegerValue::from_signed(0)),
            BinaryOp::FloorDiv,
        )
        .expect_err("integer floor division by zero should fail")
        .message,
        "division by zero"
    );
    assert_eq!(
        super::eval_binary_value(Value::Float(1.0), Value::Float(0.0), BinaryOp::FloorDiv)
            .expect_err("float floor division by zero should fail")
            .message,
        "division by zero"
    );
    assert_eq!(
        super::eval_binary_value(Value::Float(1.0), Value::Float(-0.0), BinaryOp::FloorDiv)
            .expect_err("float floor division by negative zero should fail")
            .message,
        "division by zero"
    );
    assert_eq!(
        super::eval_binary_value(Value::Float(1.0), Value::Float(-0.0), BinaryOp::Mod)
            .expect_err("float remainder by negative zero should fail")
            .message,
        "division by zero"
    );
    assert!(super::eval_binary_value(
        Value::Int(IntegerValue::from_literal(u128::MAX)),
        Value::Int(IntegerValue::from_signed(1)),
        BinaryOp::Add,
    )
    .expect_err("checked int add should report overflow")
    .message
    .contains("integer overflow"));
    assert!(super::eval_binary_value(
        Value::Int(IntegerValue::from_signed(1)),
        Value::Int(IntegerValue::from_signed(0)),
        BinaryOp::Div,
    )
    .expect_err("int division by zero should fail")
    .message
    .contains("division by zero"));
    assert!(super::eval_binary_value(
        Value::Int(IntegerValue::from_signed(1)),
        Value::Int(IntegerValue::from_signed(0)),
        BinaryOp::Mod,
    )
    .expect_err("int modulo by zero should fail")
    .message
    .contains("division by zero"));
    assert!(super::eval_binary_value(
        Value::Bool(true),
        Value::String("x".to_string()),
        BinaryOp::Add,
    )
    .expect_err("unsupported add operands should fail")
    .message
    .contains("unsupported `+` operands"));
    assert!(super::eval_binary_value(
        Value::Bool(true),
        Value::String("x".to_string()),
        BinaryOp::Sub,
    )
    .expect_err("unsupported sub operands should fail")
    .message
    .contains("unsupported `-` operands"));
    assert!(super::eval_binary_value(
        Value::Bool(true),
        Value::String("x".to_string()),
        BinaryOp::Mul,
    )
    .expect_err("unsupported mul operands should fail")
    .message
    .contains("unsupported `*` operands"));
    assert!(super::eval_binary_value(
        Value::Bool(true),
        Value::String("x".to_string()),
        BinaryOp::Div,
    )
    .expect_err("unsupported div operands should fail")
    .message
    .contains("unsupported `/` operands"));
    assert!(super::eval_binary_value(
        Value::Bool(true),
        Value::String("x".to_string()),
        BinaryOp::Mod,
    )
    .expect_err("unsupported mod operands should fail")
    .message
    .contains("unsupported `%` operands"));

    assert_eq!(
        super::eval_unary_value(Value::Bool(false), UnaryOp::Not).expect("bool not should succeed"),
        Value::Bool(true)
    );
    assert_eq!(
        super::eval_unary_value(Value::Float(3.5), UnaryOp::Neg)
            .expect("float negation should succeed"),
        Value::Float(-3.5)
    );
    assert!(super::eval_unary_value(
        Value::Int(IntegerValue::from_literal((1_u128 << 127) + 1)),
        UnaryOp::Neg,
    )
    .expect_err("minimum signed integer negation should overflow")
    .message
    .contains("integer overflow"));
    assert!(
        super::eval_unary_value(Value::String("x".to_string()), UnaryOp::Not)
            .expect_err("logical not should reject non-bools")
            .message
            .contains("expects `bool`")
    );
    assert!(
        super::eval_unary_value(Value::String("x".to_string()), UnaryOp::Neg)
            .expect_err("unary minus should reject non-numerics")
            .message
            .contains("expects a numeric value")
    );
}

#[test]
fn native_runtime_timeout_and_option_decoders_cover_error_edges() {
    assert_eq!(super::extract_duration_millis(Value::Duration(42)), 42);
    let message = capture_runtime_error_message(|| {
        let _ = super::extract_duration_millis(Value::Int(IntegerValue::from_literal(
            (i128::MAX as u128) + 1,
        )));
    });
    assert!(message.contains("outside signed timer range"));
    let message = capture_runtime_error_message(|| {
        let _ = super::extract_duration_millis(Value::String("soon".to_string()));
    });
    assert!(message.contains("expected `Duration`"));

    let invalid_utf8 = [0xff_u8];
    let message = capture_runtime_error_message(|| {
        let _ = super::decode_bytes(invalid_utf8.as_ptr(), invalid_utf8.len());
    });
    assert!(message.contains("invalid UTF-8"));

    let mut null_payloads = vec![0_i64].into_boxed_slice();
    let null_payloads_ptr = null_payloads.as_mut_ptr();
    let null_payloads_len = null_payloads.len();
    std::mem::forget(null_payloads);
    let message = capture_runtime_error_message(|| unsafe {
        let _ = super::consume_opaque_buffer(null_payloads_ptr, null_payloads_len);
    });
    assert!(message.contains("null enum payload handle"));

    let cleanup_value = int_value(9);
    let mut cleanup_args = vec![cleanup_value as i64].into_boxed_slice();
    let cleanup_args_ptr = cleanup_args.as_mut_ptr();
    let cleanup_args_len = cleanup_args.len();
    std::mem::forget(cleanup_args);
    unsafe {
        super::release_direct_cleanup_args(cleanup_args_ptr, cleanup_args_len);
    }
    unsafe {
        super::release_direct_cleanup_args(std::ptr::null_mut(), 1);
    }
    let mut zero_cleanup_args = vec![0_i64].into_boxed_slice();
    let zero_cleanup_args_ptr = zero_cleanup_args.as_mut_ptr();
    let zero_cleanup_args_len = zero_cleanup_args.len();
    std::mem::forget(zero_cleanup_args);
    unsafe {
        super::release_direct_cleanup_args(zero_cleanup_args_ptr, zero_cleanup_args_len);
    }

    assert_eq!(
        super::optional_timeout_from_ptr(std::ptr::null_mut(), "timeout"),
        None
    );
    assert_eq!(
        super::process_optional_timeout_from_ptr(std::ptr::null_mut(), "timeout"),
        None
    );

    let unit = boxed_value(Value::Unit);
    assert_eq!(super::optional_timeout_from_ptr(unit, "timeout"), None);
    assert_eq!(
        super::process_optional_timeout_from_ptr(unit, "timeout"),
        None
    );
    unsafe { release_value(unit) };

    let duration = boxed_value(Value::Duration(25));
    assert_eq!(
        super::optional_timeout_from_ptr(duration, "timeout"),
        Some(StdDuration::from_millis(25))
    );
    assert_eq!(
        super::process_optional_timeout_from_ptr(duration, "timeout"),
        Some(StdDuration::from_millis(25))
    );
    unsafe { release_value(duration) };

    let negative_timeout = boxed_value(Value::Duration(-1));
    let message = capture_runtime_error_message(|| {
        let _ = super::optional_timeout_from_ptr(negative_timeout, "timeout");
    });
    assert!(message.contains("duration must be non-negative"));
    unsafe { release_value(negative_timeout) };

    let open_ended_timeout = boxed_value(Value::Duration(-1));
    assert_eq!(
        super::process_optional_timeout_from_ptr(open_ended_timeout, "timeout"),
        None
    );
    unsafe { release_value(open_ended_timeout) };

    let huge_timeout = boxed_value(Value::Duration(i128::MAX));
    let message = capture_runtime_error_message(|| {
        let _ = super::process_optional_timeout_from_ptr(huge_timeout, "timeout");
    });
    assert!(message.contains("duration must be non-negative"));
    unsafe { release_value(huge_timeout) };

    let wrong_timeout = boxed_value(Value::String("soon".to_string()));
    let message = capture_runtime_error_message(|| {
        let _ = super::optional_timeout_from_ptr(wrong_timeout, "timeout");
    });
    assert!(message.contains("expects `Duration`"));
    unsafe { release_value(wrong_timeout) };

    let wrong_duration = boxed_value(Value::String("soon".to_string()));
    let message = capture_runtime_error_message(|| {
        let _ = super::duration_from_ptr(wrong_duration, "sleep");
    });
    assert!(message.contains("expects `Duration`"));
    unsafe { release_value(wrong_duration) };

    let invalid_restarts = int_value(-2);
    let message = capture_runtime_error_message(|| {
        let _ = super::supervisor_max_restarts_from_ptr(invalid_restarts, "supervisor");
    });
    assert!(message.contains("max_restarts"));
    unsafe { release_value(invalid_restarts) };

    assert_eq!(
        super::expect_command_vec(
            &Value::Vec(VecValue {
                element_type: crate::sema::Type::named("Unknown"),
                elements: vec![Value::String("echo".to_string())],
            }),
            "command",
        ),
        vec!["echo".to_string()]
    );
    let message = capture_runtime_error_message(|| {
        let _ = super::expect_command_vec(
            &Value::Vec(VecValue {
                element_type: crate::sema::Type::named("int32"),
                elements: vec![Value::Int(IntegerValue::from_signed(1))],
            }),
            "command",
        );
    });
    assert!(message.contains("expects `Vec[String]`"));

    assert_eq!(
        super::expect_optional_string_value(&Value::Unit, "stderr"),
        None
    );
    assert_eq!(
        super::expect_optional_string_value(
            &Value::EnumVariant(EnumVariantValue {
                enum_name: "Option".to_string(),
                variant_name: "None".to_string(),
                payloads: vec![],
            }),
            "stderr",
        ),
        None
    );
    assert_eq!(
        super::expect_optional_string_value(
            &Value::EnumVariant(EnumVariantValue {
                enum_name: "Option".to_string(),
                variant_name: "Some".to_string(),
                payloads: vec![Value::String("log".to_string())],
            }),
            "stderr",
        ),
        Some("log".to_string())
    );
    let message = capture_runtime_error_message(|| {
        let _ = super::expect_optional_string_value(
            &Value::EnumVariant(EnumVariantValue {
                enum_name: "Option".to_string(),
                variant_name: "Some".to_string(),
                payloads: vec![],
            }),
            "stderr",
        );
    });
    assert!(message.contains("malformed option payload"));
    let message = capture_runtime_error_message(|| {
        let _ = super::expect_optional_string_value(&Value::Bool(true), "stderr");
    });
    assert!(message.contains("expects `Option[String]`"));
}

#[test]
fn runtime_init_is_callable() {
    super::aurora_direct_runtime_init(
        b"/virtual/test.au".as_ptr(),
        b"/virtual/test.au".len(),
        b"def main() -> int32:\n    return 0\n".as_ptr(),
        b"def main() -> int32:\n    return 0\n".len(),
    );
}

#[test]
fn native_runtime_ref_count_helpers_reject_zero_and_overflow() {
    let released = AtomicUsize::new(1);
    assert!(super::release_ref_count(&released).expect("final release should succeed"));
    assert_eq!(released.load(Ordering::Relaxed), 0);

    let released_retain = AtomicUsize::new(0);
    let retain_after_release_error = super::retain_ref_count(&released_retain)
        .expect_err("retain after release should be rejected");
    assert!(retain_after_release_error.contains("already-released"));

    let overflow = AtomicUsize::new(usize::MAX);
    let overflow_error =
        super::retain_ref_count(&overflow).expect_err("overflow should be rejected");
    assert!(overflow_error.contains("overflow"));
    assert_eq!(overflow.load(Ordering::Relaxed), usize::MAX);

    let zero = AtomicUsize::new(0);
    let underflow_error =
        super::release_ref_count(&zero).expect_err("underflow should be rejected");
    assert!(underflow_error.contains("already-released"));
    assert_eq!(zero.load(Ordering::Relaxed), 0);

    let shared = AtomicUsize::new(2);
    assert!(!super::release_ref_count(&shared).expect("shared release should succeed"));
    assert_eq!(shared.load(Ordering::Relaxed), 1);
    super::retain_ref_count(&shared).expect("retain should succeed");
    assert_eq!(shared.load(Ordering::Relaxed), 2);
}

#[cfg(unix)]
#[test]
fn with_sigpipe_blocked_restores_the_previous_signal_mask_after_broken_pipe() {
    unsafe fn current_sigpipe_blocked() -> bool {
        let mut current: libc::sigset_t = std::mem::zeroed();
        let rc = libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), &mut current);
        assert_eq!(rc, 0, "should read current signal mask");
        libc::sigismember(&current, libc::SIGPIPE) == 1
    }

    let before = unsafe { current_sigpipe_blocked() };
    let error = super::with_sigpipe_blocked(|| {
        Err::<(), _>(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "simulated broken pipe",
        ))
    })
    .expect_err("broken pipe should propagate through helper");
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    let after = unsafe { current_sigpipe_blocked() };
    assert_eq!(
        after, before,
        "SIGPIPE mask should be restored after helper returns"
    );
}

#[test]
fn direct_print_helpers_are_callable() {
    super::aurora_direct_print_i64(7);
    super::aurora_direct_print_f64(7.0);
    super::aurora_direct_print_bool(0);
    super::aurora_direct_print_bool(1);
    super::aurora_direct_print_value(string_value(""));
    expect_result_ok_unit(super::aurora_direct_io_write(string_value("")));
    expect_result_ok_unit(super::aurora_direct_io_flush());
}

#[test]
fn direct_print_u64_renders_the_full_unsigned_range() {
    const HELPER_ENV: &str = "AURORA_DIRECT_RUNTIME_PRINT_U64_HELPER";
    if std::env::var_os(HELPER_ENV).is_some() {
        super::aurora_direct_print_u64(u64::MAX);
        return;
    }

    let output = Command::new(std::env::current_exe().expect("test binary should exist"))
        .arg("--exact")
        .arg("native_runtime::tests::direct_print_u64_renders_the_full_unsigned_range")
        .arg("--nocapture")
        .env(HELPER_ENV, "1")
        .output()
        .expect("child test process should run");

    assert!(
        output.status.success(),
        "uint64 print helper should succeed"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("18446744073709551615\n"),
        "uint64 print helper should render u64::MAX as unsigned decimal"
    );
}

#[test]
fn direct_uint64_boxing_helpers_preserve_the_full_range() {
    for value in [0, (i64::MAX as u64) + 1, u64::MAX] {
        let boxed = super::aurora_direct_box_u64(value);
        match unsafe { value_ref(boxed) } {
            Value::Int(actual) => {
                assert_eq!(
                    actual.representation(),
                    IntegerRepresentation::Unsigned(u128::from(value))
                );
                assert_eq!(actual.runtime_type_name(), Some("uint64"));
            }
            other => panic!("expected canonical unsigned integer, found {:?}", other),
        }
        assert_eq!(super::aurora_direct_unbox_u64(boxed), value);
        unsafe {
            release_value(boxed);
        }
    }
}

#[test]
fn direct_runtime_type_tags_preserve_generic_identity_through_clone() {
    let value = boxed_value(Value::EnumVariant(EnumVariantValue {
        enum_name: "Option".to_string(),
        variant_name: "Some".to_string(),
        payloads: vec![Value::Int(IntegerValue::from_i32(7))],
    }));
    super::aurora_direct_tag_value_type(value, b"Option[int32]".as_ptr(), "Option[int32]".len());

    for candidate in [value, super::aurora_direct_clone_value(value)] {
        assert_eq!(
            super::aurora_direct_value_type_matches(
                candidate,
                b"Option[int32]".as_ptr(),
                "Option[int32]".len(),
            ),
            1
        );
        assert_eq!(
            super::aurora_direct_value_type_matches(
                candidate,
                b"Option[int64]".as_ptr(),
                "Option[int64]".len(),
            ),
            0
        );
        assert_eq!(
            super::aurora_direct_value_type_matches(candidate, b"Option".as_ptr(), "Option".len(),),
            1
        );
        assert_eq!(
            super::aurora_direct_value_type_matches(
                candidate,
                b"Option[?T]".as_ptr(),
                "Option[?T]".len(),
            ),
            1
        );
        assert_eq!(
            super::aurora_direct_value_type_matches(
                candidate,
                b"Option[Vec[?T]]".as_ptr(),
                "Option[Vec[?T]]".len(),
            ),
            0
        );
        unsafe {
            release_value(candidate);
        }
    }

    let nested = boxed_value(Value::EnumVariant(EnumVariantValue {
        enum_name: "Option".to_string(),
        variant_name: "Some".to_string(),
        payloads: vec![Value::Vec(VecValue {
            element_type: Type::named("int64"),
            elements: vec![Value::Int(
                IntegerValue::from_typed_signed(9, IntegerKind::Int64).expect("9 fits int64"),
            )],
        })],
    }));
    super::aurora_direct_tag_value_type(
        nested,
        b"Option[Vec[int64]]".as_ptr(),
        "Option[Vec[int64]]".len(),
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(nested, b"Option[?T]".as_ptr(), "Option[?T]".len(),),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            nested,
            b"Option[Vec[?T]]".as_ptr(),
            "Option[Vec[?T]]".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            nested,
            b"Option[Vec[int32]]".as_ptr(),
            "Option[Vec[int32]]".len(),
        ),
        0
    );
    unsafe {
        release_value(nested);
    }

    let mixed_map = boxed_value(Value::Map(MapValue {
        key_type: Type::named("Unknown"),
        value_type: Type::named("Unknown"),
        entries: Vec::new(),
    }));
    super::aurora_direct_tag_value_type(
        mixed_map,
        b"Map[int32, int64]".as_ptr(),
        "Map[int32, int64]".len(),
    );
    match unsafe { value_ref(mixed_map) } {
        Value::Map(map) => {
            assert_eq!(map.key_type, Type::named("int32"));
            assert_eq!(map.value_type, Type::named("int64"));
        }
        other => panic!("expected tagged map, found {other:?}"),
    }
    assert_eq!(
        super::aurora_direct_value_type_matches(
            mixed_map,
            b"Map[?K, ?V]".as_ptr(),
            "Map[?K, ?V]".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            mixed_map,
            b"Map[?T, ?T]".as_ptr(),
            "Map[?T, ?T]".len(),
        ),
        0
    );
    unsafe {
        release_value(mixed_map);
    }

    let vector = boxed_value(Value::Vec(VecValue {
        element_type: Type::named("Unknown"),
        elements: Vec::new(),
    }));
    super::aurora_direct_tag_value_type(vector, b"Vec[int32]".as_ptr(), "Vec[int32]".len());
    assert_eq!(super::aurora_direct_value_has_runtime_type(vector), 1);
    match unsafe { value_ref(vector) } {
        Value::Vec(vector) => assert_eq!(vector.element_type, Type::named("int32")),
        other => panic!("expected tagged vector, found {other:?}"),
    }
    assert_eq!(
        super::aurora_direct_value_type_matches(vector, b"Vec[?T]".as_ptr(), "Vec[?T]".len(),),
        1
    );
    unsafe {
        release_value(vector);
    }

    let set = boxed_value(Value::Set(SetValue {
        element_type: Type::named("Unknown"),
        elements: Vec::new(),
    }));
    super::aurora_direct_tag_value_type(set, b"Set[int64]".as_ptr(), "Set[int64]".len());
    match unsafe { value_ref(set) } {
        Value::Set(set) => assert_eq!(set.element_type, Type::named("int64")),
        other => panic!("expected tagged set, found {other:?}"),
    }
    assert_eq!(
        super::aurora_direct_value_type_matches(set, b"Set[?T]".as_ptr(), "Set[?T]".len(),),
        1
    );
    unsafe {
        release_value(set);
    }

    let instance = boxed_value(Value::Instance(InstanceValue {
        class_name: "Marker".to_string(),
        fields: BTreeMap::new(),
    }));
    super::aurora_direct_tag_value_type(instance, b"Marker[int64]".as_ptr(), "Marker[int64]".len());
    assert_eq!(
        super::aurora_direct_value_type_matches(
            instance,
            b"Marker[?T]".as_ptr(),
            "Marker[?T]".len(),
        ),
        1
    );
    let cloned_instance = super::aurora_direct_clone_value(instance);
    assert_eq!(
        super::aurora_direct_value_type_matches(
            cloned_instance,
            b"Marker[int64]".as_ptr(),
            "Marker[int64]".len(),
        ),
        1
    );
    unsafe {
        release_value(instance);
        release_value(cloned_instance);
    }

    let queue = boxed_value(Value::Channel(ChannelValue::new()));
    super::aurora_direct_tag_value_type(queue, b"Queue[int32]".as_ptr(), "Queue[int32]".len());
    assert_eq!(
        super::aurora_direct_value_type_matches(queue, b"Queue[?T]".as_ptr(), "Queue[?T]".len(),),
        1
    );
    unsafe {
        release_value(queue);
    }

    let task = boxed_value(Value::Task(TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Unit)
    }))));
    super::aurora_direct_tag_value_type(task, b"Task[int64]".as_ptr(), "Task[int64]".len());
    assert_eq!(
        super::aurora_direct_value_type_matches(task, b"Task[?T]".as_ptr(), "Task[?T]".len(),),
        1
    );
    unsafe {
        release_value(task);
    }

    let unit = boxed_value(Value::Unit);
    assert_eq!(super::aurora_direct_value_has_runtime_type(unit), 0);
    unsafe {
        release_value(unit);
    }

    let untagged = boxed_value(Value::EnumVariant(EnumVariantValue {
        enum_name: "Option".to_string(),
        variant_name: "Some".to_string(),
        payloads: vec![Value::Int(IntegerValue::from_i32(11))],
    }));
    assert_eq!(
        super::aurora_direct_value_type_matches(
            untagged,
            b"Option[?T]".as_ptr(),
            "Option[?T]".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            untagged,
            b"Option[Vec[?T]]".as_ptr(),
            "Option[Vec[?T]]".len(),
        ),
        0
    );
    unsafe {
        release_value(untagged);
    }
}

#[test]
fn direct_int64_unbox_helper_preserves_the_full_signed_range() {
    for value in [i64::MIN, -1, 0, i64::MAX] {
        let boxed = super::aurora_direct_box_i64(value);
        assert_eq!(super::aurora_direct_unbox_int64(boxed), value);
        unsafe {
            release_value(boxed);
        }
    }
}

#[test]
fn direct_integer_to_float_helper_rounds_without_consuming_the_integer() {
    let boxed = boxed_value(Value::Int(IntegerValue::from_literal(
        9_007_199_254_740_993,
    )));
    assert_eq!(
        super::aurora_direct_integer_to_float(boxed),
        9_007_199_254_740_992.0
    );
    assert_eq!(
        super::aurora_direct_integer_to_float(boxed),
        9_007_199_254_740_992.0
    );
    unsafe {
        release_value(boxed);
    }
}

#[test]
fn direct_unboxed_wide_cast_helpers_preserve_checked_numeric_semantics() {
    assert_eq!(
        super::aurora_direct_cast_integer_to_integer((-42_i64) as u64, 0, 0, 0, 0),
        (-42_i64) as u64
    );
    assert_eq!(
        super::aurora_direct_cast_integer_to_integer(42, 0, 1, 0, 0),
        42
    );
    assert_eq!(
        super::aurora_direct_cast_integer_to_integer(u64::MAX, 1, 2, 0, 0),
        u64::MAX
    );
    assert_eq!(
        super::aurora_direct_cast_integer_to_float(1_u64 << 53, 0, 1, 0, 0),
        (1_u64 << 53) as f64
    );
    assert_eq!(
        super::aurora_direct_cast_integer_to_float(1_u64 << 63, 1, 1, 0, 0),
        (1_u64 << 63) as f64
    );
    assert_eq!(
        super::aurora_direct_cast_integer_to_float(42, 0, 0, 0, 0),
        42.0_f32 as f64
    );
    assert_eq!(
        super::aurora_direct_cast_float_to_integer(4_294_967_296.75, 1, 0, 0),
        4_294_967_296
    );
    assert_eq!(
        super::aurora_direct_cast_float_to_integer(-42.75, 1, 0, 0),
        (-42_i64) as u64
    );
}

#[test]
fn wide_integer_overflow_messages_match_mir_diagnostics_exactly() {
    for (kind, op, left, right, expected) in [
        (
            0,
            0,
            i64::MAX as u64,
            1,
            "integer value `9223372036854775808` does not fit in `int64`",
        ),
        (
            0,
            1,
            i64::MIN as u64,
            1,
            "integer value `-9223372036854775809` does not fit in `int64`",
        ),
        (
            0,
            2,
            i64::MAX as u64,
            2,
            "integer value `18446744073709551614` does not fit in `int64`",
        ),
        (
            0,
            3,
            i64::MIN as u64,
            (-1_i64) as u64,
            "integer value `9223372036854775808` does not fit in `int64`",
        ),
        (
            1,
            0,
            u64::MAX,
            1,
            "integer value `18446744073709551616` does not fit in `uint64`",
        ),
        (1, 1, 0, 1, "integer value `-1` does not fit in `uint64`"),
        (
            1,
            2,
            u64::MAX,
            2,
            "integer value `36893488147419103230` does not fit in `uint64`",
        ),
    ] {
        assert_eq!(
            super::wide_integer_overflow_message(kind, op, left, right),
            expected
        );
    }
}

#[test]
fn direct_stdout_result_helpers_accept_empty_writes_and_flushes() {
    super::write_stdout_result("").expect("empty direct stdout writes should succeed");
    super::flush_stdout_result().expect("direct stdout flushes should succeed");
}

#[test]
fn native_runtime_process_capture_task_helper_covers_success_and_malformed_results() {
    assert_eq!(
        super::await_process_capture_task(None, "stdout"),
        Vec::<u8>::new()
    );

    let bytes_task = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Vec(VecValue {
            element_type: crate::sema::Type::named("uint8"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(65)),
                Value::Int(IntegerValue::from_literal(66)),
            ],
        }))
    }));
    assert_eq!(
        super::await_process_capture_task(Some(bytes_task), "stdout"),
        b"AB".to_vec()
    );

    let non_byte_integer = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Vec(VecValue {
            element_type: crate::sema::Type::named("uint8"),
            elements: vec![Value::Int(IntegerValue::from_signed(300))],
        }))
    }));
    let message = capture_runtime_error_message(|| {
        super::await_process_capture_task(Some(non_byte_integer), "stdout");
    });
    assert!(message.contains("process stdout capture returned a non-byte integer"));

    let wrong_payload = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Vec(VecValue {
            element_type: crate::sema::Type::named("uint8"),
            elements: vec![Value::String("bad".to_string())],
        }))
    }));
    let message = capture_runtime_error_message(|| {
        super::await_process_capture_task(Some(wrong_payload), "stderr");
    });
    assert!(message.contains("process stderr capture returned `bad` inside `Vec[uint8]"));

    let wrong_result_type = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Vec(VecValue {
            element_type: crate::sema::Type::named("String"),
            elements: vec![Value::String("bad".to_string())],
        }))
    }));
    let message = capture_runtime_error_message(|| {
        super::await_process_capture_task(Some(wrong_result_type), "stderr");
    });
    assert!(message.contains("process stderr capture returned `[bad]` instead of `Vec[uint8]"));

    let capture_error =
        TaskValue::from_handle(thread::spawn(|| Err(Diagnostic::new("pipe failed"))));
    let message = capture_runtime_error_message(|| {
        super::await_process_capture_task(Some(capture_error), "stdout");
    });
    assert!(message.contains("pipe failed"));

    let group = TaskGroupValue::new(&CancellationContext::default());
    let cancellation = group.child_cancellation();
    group.cancel();
    let message = with_cancellation_scope(cancellation, || {
        let cancelled_task = TaskValue::from_handle(thread::spawn(|| {
            thread::sleep(StdDuration::from_millis(50));
            Ok(Value::Vec(VecValue {
                element_type: crate::sema::Type::named("uint8"),
                elements: Vec::new(),
            }))
        }));
        capture_runtime_error_message(|| {
            super::await_process_capture_task(Some(cancelled_task), "stdout");
        })
    });
    assert!(message.contains("process stdout capture was cancelled unexpectedly"));
}

#[test]
fn native_runtime_process_error_and_wait_all_helpers_cover_remaining_paths() {
    assert!(expect_variant_value(
        super::process_error_from_io(io::Error::new(io::ErrorKind::TimedOut, "timed out")),
        "Error",
        "TimedOut",
    )
    .is_empty());
    assert!(expect_variant_value(
        super::process_error_from_io(io::Error::new(io::ErrorKind::Interrupted, "cancelled")),
        "Error",
        "Cancelled",
    )
    .is_empty());
    assert_eq!(
        expect_variant_value(
            super::process_error_from_io(io::Error::new(io::ErrorKind::Other, "io failure")),
            "Error",
            "Io",
        )
        .len(),
        1
    );

    let wait_all_payloads = expect_variant_ptr(
        super::aurora_direct_wait_all(super::aurora_direct_vec_empty()),
        "WaitAll",
        "Ready",
    );
    match wait_all_payloads.as_slice() {
        [Value::Vec(values)] => assert!(values.elements.is_empty()),
        other => panic!(
            "expected WaitAll.Ready empty vec payload, found {:?}",
            other
        ),
    }

    assert!(expect_variant_ptr(
        super::aurora_direct_wait_any(super::aurora_direct_vec_empty()),
        "WaitAny",
        "TimedOut",
    )
    .is_empty());
    assert!(expect_variant_ptr(
        super::aurora_direct_wait_any_timeout_value(
            super::aurora_direct_vec_empty(),
            duration_value(0),
        ),
        "WaitAny",
        "TimedOut",
    )
    .is_empty());

    let timed_wait_all_payloads = expect_variant_ptr(
        super::aurora_direct_wait_all_timeout_value(
            super::aurora_direct_vec_empty(),
            duration_value(0),
        ),
        "WaitAll",
        "Ready",
    );
    match timed_wait_all_payloads.as_slice() {
        [Value::Vec(values)] => assert!(values.elements.is_empty()),
        other => panic!(
            "expected WaitAll.Ready empty vec payload, found {:?}",
            other
        ),
    }

    let ready_task = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Int(IntegerValue::from_signed(70)))
    }));
    let ready_payloads = expect_variant_ptr(
        super::aurora_direct_wait_any(task_vec(&[ready_task.clone()])),
        "WaitAny",
        "Ready",
    );
    match ready_payloads.as_slice() {
        [Value::Int(index), Value::Int(value)] => {
            assert_eq!(index.as_i128(), Some(0));
            assert_eq!(value.as_i128(), Some(70));
        }
        other => panic!("expected WaitAny.Ready(0, 70), found {:?}", other),
    }

    let error_task =
        TaskValue::from_handle(thread::spawn(|| Err(Diagnostic::new("wait_any failed"))));
    let error_payloads = expect_variant_ptr(
        super::aurora_direct_wait_any(task_vec(&[error_task.clone()])),
        "WaitAny",
        "Error",
    );
    match error_payloads.as_slice() {
        [Value::Int(index), Value::String(message)] => {
            assert_eq!(index.as_i128(), Some(0));
            assert_eq!(message, "wait_any failed");
        }
        other => panic!("expected WaitAny.Error(0, message), found {:?}", other),
    }

    let first = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Int(IntegerValue::from_signed(1)))
    }));
    let second = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Int(IntegerValue::from_signed(2)))
    }));
    let all_ready_payloads = expect_variant_ptr(
        super::aurora_direct_wait_all(task_vec(&[first.clone(), second.clone()])),
        "WaitAll",
        "Ready",
    );
    match all_ready_payloads.as_slice() {
        [Value::Vec(values)] => {
            let ints = values
                .elements
                .iter()
                .map(|value| match value {
                    Value::Int(value) => value.as_i128().expect("expected signed integer"),
                    other => panic!("expected int wait_all value, found {:?}", other),
                })
                .collect::<Vec<_>>();
            assert_eq!(ints, vec![1, 2]);
        }
        other => panic!("expected WaitAll.Ready([1, 2]), found {:?}", other),
    }

    let wait_all_error_task =
        TaskValue::from_handle(thread::spawn(|| Err(Diagnostic::new("wait_all failed"))));
    let all_error_payloads = expect_variant_ptr(
        super::aurora_direct_wait_all(task_vec(&[first.clone(), wait_all_error_task.clone()])),
        "WaitAll",
        "Error",
    );
    match all_error_payloads.as_slice() {
        [Value::Int(index), Value::String(message)] => {
            assert_eq!(index.as_i128(), Some(1));
            assert_eq!(message, "wait_all failed");
        }
        other => panic!("expected WaitAll.Error(1, message), found {:?}", other),
    }

    let slow_task = TaskValue::from_handle(thread::spawn(|| {
        thread::sleep(StdDuration::from_millis(50));
        Ok(Value::Int(IntegerValue::from_signed(9)))
    }));
    assert!(expect_variant_ptr(
        super::aurora_direct_wait_any_timeout_value(
            task_vec(&[slow_task.clone()]),
            duration_value(0)
        ),
        "WaitAny",
        "TimedOut",
    )
    .is_empty());
    assert!(expect_variant_ptr(
        super::aurora_direct_wait_all_timeout_value(
            task_vec(&[slow_task.clone()]),
            duration_value(0)
        ),
        "WaitAll",
        "TimedOut",
    )
    .is_empty());
    assert_eq!(
        expect_task_result_ready_int(super::aurora_direct_task_join(boxed_value(Value::Task(
            slow_task
        )))),
        9
    );

    let no_start_command = expect_result_err_payload(super::aurora_direct_process_start(
        super::aurora_direct_vec_empty(),
        boxed_value(Value::Unit),
        super::aurora_direct_map_empty(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        bool_value(false),
    ));
    assert!(expect_variant_value(no_start_command, "Error", "NoCommand").is_empty());

    let no_run_command = expect_result_err_payload(super::aurora_direct_process_run(
        super::aurora_direct_vec_empty(),
        boxed_value(Value::Unit),
        super::aurora_direct_map_empty(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        boxed_value(Value::Unit),
        bool_value(false),
    ));
    assert!(expect_variant_value(no_run_command, "Error", "NoCommand").is_empty());
}

#[test]
fn native_runtime_direct_process_wrappers_cover_child_pipe_and_completed_paths() {
    assert!(
        expect_variant_ptr(super::aurora_direct_process_inherit(), "Stdio", "Inherit",).is_empty()
    );

    let completed = ProcessCompletedValue::new(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "process.ExitStatus".to_string(),
            variant_name: "Exited".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(0))],
        }),
        b"stdout".to_vec(),
        b"stderr".to_vec(),
    );
    let completed_ptr = boxed_value(Value::ProcessCompleted(completed));
    assert_eq!(
        super::aurora_direct_process_completed_success(completed_ptr),
        1
    );
    assert_eq!(
        expect_string(super::aurora_direct_process_completed_stdout(completed_ptr)),
        "stdout"
    );
    assert_eq!(
        expect_string(super::aurora_direct_process_completed_stderr(completed_ptr)),
        "stderr"
    );
    assert_eq!(
        expect_vec_ints(super::aurora_direct_process_completed_stdout_bytes(
            completed_ptr
        )),
        b"stdout"
            .iter()
            .map(|byte| i128::from(*byte))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        expect_vec_ints(super::aurora_direct_process_completed_stderr_bytes(
            completed_ptr
        )),
        b"stderr"
            .iter()
            .map(|byte| i128::from(*byte))
            .collect::<Vec<_>>()
    );
    expect_result_ok_unit(super::aurora_direct_process_completed_check(completed_ptr));
    let status_payload = expect_variant_ptr(
        super::aurora_direct_process_completed_status(completed_ptr),
        "process.ExitStatus",
        "Exited",
    );
    assert_eq!(status_payload.len(), 1);
    unsafe { release_value(completed_ptr) };

    let child = ProcessChildValue::spawn(
        vec![
            std::env::current_exe()
                .expect("current test binary should be available")
                .to_string_lossy()
                .into_owned(),
            "--help".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("process child should spawn");
    let child_ptr = boxed_value(Value::ProcessChild(child));
    let stdout_payload = expect_variant_ptr(
        super::aurora_direct_process_child_stdout(child_ptr),
        "Option",
        "Some",
    );
    let stdout_pipe = match stdout_payload.as_slice() {
        [Value::ProcessPipe(pipe)] => pipe.clone(),
        other => panic!("expected process stdout pipe, found {:?}", other),
    };
    expect_option_none(super::aurora_direct_process_child_stderr(child_ptr));

    let stdout_text = expect_result_ok_string(super::aurora_direct_process_pipe_read_all(
        boxed_value(Value::ProcessPipe(stdout_pipe.clone())),
    ));
    assert!(
        stdout_text.contains("Usage") || stdout_text.contains("USAGE"),
        "unexpected child help stdout: {stdout_text}"
    );

    let wait_payload = expect_variant_ptr(
        super::aurora_direct_process_child_wait_ok(child_ptr, std::ptr::null_mut()),
        "Result",
        "Ok",
    );
    assert!(matches!(
        wait_payload.as_slice(),
        [Value::EnumVariant(status)] if status.enum_name == "ExitStatus"
    ));
    expect_unit(super::aurora_direct_process_pipe_close(boxed_value(
        Value::ProcessPipe(stdout_pipe),
    )));
    expect_unit(super::aurora_direct_process_child_close(child_ptr));
    unsafe { release_value(child_ptr) };
}

#[test]
fn native_runtime_direct_process_wrappers_cover_streaming_and_signal_paths() {
    fn process_pipe_from_option(
        ptr: *mut OpaqueValue,
        label: &str,
    ) -> crate::runtime_value::ProcessPipeValue {
        let payloads = expect_variant_ptr(ptr, "Option", "Some");
        match payloads.as_slice() {
            [Value::ProcessPipe(pipe)] => pipe.clone(),
            other => panic!("expected {label} process pipe, found {:?}", other),
        }
    }

    fn string_from_option(value: Value, label: &str) -> String {
        match expect_option_some_payload(value) {
            Value::String(text) => text,
            other => panic!("expected {label} string payload, found {:?}", other),
        }
    }

    fn byte_values_from_option(value: Value, label: &str) -> Vec<i128> {
        match expect_option_some_payload(value) {
            Value::Vec(values) => values
                .elements
                .into_iter()
                .map(|value| match value {
                    Value::Int(byte) => byte.as_i128().expect("byte should be signed"),
                    other => panic!("expected {label} byte payload, found {:?}", other),
                })
                .collect(),
            other => panic!("expected {label} byte vector, found {:?}", other),
        }
    }

    let io_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf 'alpha\\nbeta'; printf 'err\\n' >&2".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Pipe,
        false,
    )
    .expect("process with stdout and stderr should spawn");
    let io_child_ptr = boxed_value(Value::ProcessChild(io_child));
    let stdout_pipe = process_pipe_from_option(
        super::aurora_direct_process_child_stdout(io_child_ptr),
        "stdout",
    );
    let stderr_pipe = process_pipe_from_option(
        super::aurora_direct_process_child_stderr(io_child_ptr),
        "stderr",
    );

    let first_line = string_from_option(
        expect_result_ok_payload(super::aurora_direct_process_pipe_read_line(
            boxed_value(Value::ProcessPipe(stdout_pipe.clone())),
            duration_value(5_000),
        )),
        "stdout line",
    );
    assert!(first_line.starts_with("alpha"));
    let byte_chunk = byte_values_from_option(
        expect_result_ok_payload(super::aurora_direct_process_pipe_read_bytes(
            boxed_value(Value::ProcessPipe(stdout_pipe.clone())),
            int_value(4),
            duration_value(5_000),
        )),
        "stdout byte chunk",
    );
    assert!(!byte_chunk.is_empty());
    assert_eq!(
        expect_result_ok_string(super::aurora_direct_process_pipe_read_all(boxed_value(
            Value::ProcessPipe(stderr_pipe.clone())
        ))),
        "err\n"
    );

    let maybe_status = expect_result_ok_payload(super::aurora_direct_process_child_wait_or_none(
        io_child_ptr,
        duration_value(5_000),
    ));
    match expect_option_some_payload(maybe_status) {
        Value::EnumVariant(status) if status.enum_name == "ExitStatus" => {}
        other => panic!("expected process exit status, found {:?}", other),
    }
    let waited_again = expect_variant_ptr(
        super::aurora_direct_process_child_wait(io_child_ptr, std::ptr::null_mut()),
        "Wait",
        "Exited",
    );
    assert_eq!(waited_again.len(), 1);
    expect_unit(super::aurora_direct_process_pipe_close(boxed_value(
        Value::ProcessPipe(stdout_pipe),
    )));
    expect_unit(super::aurora_direct_process_pipe_close(boxed_value(
        Value::ProcessPipe(stderr_pipe),
    )));
    expect_unit(super::aurora_direct_process_child_close(io_child_ptr));
    unsafe { release_value(io_child_ptr) };

    let cat_child = ProcessChildValue::spawn(
        vec!["/bin/sh".to_string(), "-c".to_string(), "cat".to_string()],
        None,
        Vec::new(),
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("cat process should spawn");
    let cat_child_ptr = boxed_value(Value::ProcessChild(cat_child));
    let cat_stdin = process_pipe_from_option(
        super::aurora_direct_process_child_stdin(cat_child_ptr),
        "stdin",
    );
    let cat_stdout = process_pipe_from_option(
        super::aurora_direct_process_child_stdout(cat_child_ptr),
        "stdout",
    );
    expect_result_ok_unit(super::aurora_direct_process_pipe_write_all(
        boxed_value(Value::ProcessPipe(cat_stdin.clone())),
        string_value("left"),
        duration_value(5_000),
    ));
    expect_result_ok_unit(super::aurora_direct_process_pipe_write_bytes(
        boxed_value(Value::ProcessPipe(cat_stdin.clone())),
        int_vec(&[114, 105, 103, 104, 116, 10]),
        duration_value(5_000),
    ));
    expect_result_ok_unit(super::aurora_direct_process_pipe_flush(boxed_value(
        Value::ProcessPipe(cat_stdin.clone()),
    )));
    expect_unit(super::aurora_direct_process_pipe_close(boxed_value(
        Value::ProcessPipe(cat_stdin),
    )));
    assert_eq!(
        expect_result_ok_string(super::aurora_direct_process_pipe_read_all(boxed_value(
            Value::ProcessPipe(cat_stdout.clone())
        ))),
        "leftright\n"
    );
    let cat_wait = expect_variant_ptr(
        super::aurora_direct_process_child_wait(cat_child_ptr, duration_value(5_000)),
        "Wait",
        "Exited",
    );
    assert_eq!(cat_wait.len(), 1);
    expect_unit(super::aurora_direct_process_pipe_close(boxed_value(
        Value::ProcessPipe(cat_stdout),
    )));
    expect_unit(super::aurora_direct_process_child_close(cat_child_ptr));
    unsafe { release_value(cat_child_ptr) };

    let signal_wrappers: [extern "C-unwind" fn(*mut OpaqueValue) -> *mut OpaqueValue; 2] = [
        super::aurora_direct_process_child_terminate,
        super::aurora_direct_process_child_kill,
    ];
    for signal_wrapper in signal_wrappers {
        let child = ProcessChildValue::spawn(
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "sleep 10".to_string(),
            ],
            None,
            Vec::new(),
            ProcessStdioConfig::Null,
            ProcessStdioConfig::Null,
            ProcessStdioConfig::Null,
            false,
        )
        .expect("sleep process should spawn");
        let child_ptr = boxed_value(Value::ProcessChild(child));
        expect_result_ok_unit(signal_wrapper(child_ptr));
        let wait_payloads = expect_variant_ptr(
            super::aurora_direct_process_child_wait(child_ptr, duration_value(5_000)),
            "Wait",
            "Exited",
        );
        assert_eq!(wait_payloads.len(), 1);
        expect_unit(super::aurora_direct_process_child_close(child_ptr));
        unsafe { release_value(child_ptr) };
    }
}

#[test]
fn native_runtime_direct_process_wrappers_cover_timeout_and_error_results() {
    fn process_pipe_from_option(
        ptr: *mut OpaqueValue,
        label: &str,
    ) -> crate::runtime_value::ProcessPipeValue {
        let payloads = expect_variant_ptr(ptr, "Option", "Some");
        match payloads.as_slice() {
            [Value::ProcessPipe(pipe)] => pipe.clone(),
            other => panic!("expected {label} process pipe, found {:?}", other),
        }
    }

    fn assert_process_io_error(value: Value) {
        assert_eq!(expect_variant_value(value, "Error", "Io").len(), 1);
    }

    let failed_completed = ProcessCompletedValue::new(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "process.ExitStatus".to_string(),
            variant_name: "Exited".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(7))],
        }),
        Vec::new(),
        Vec::new(),
    );
    let failed_completed_ptr = boxed_value(Value::ProcessCompleted(failed_completed));
    assert_eq!(
        super::aurora_direct_process_completed_success(failed_completed_ptr),
        0
    );
    assert_eq!(
        expect_variant_value(
            expect_result_err_payload(super::aurora_direct_process_completed_check(
                failed_completed_ptr
            )),
            "Error",
            "Other",
        )
        .len(),
        1
    );
    unsafe { release_value(failed_completed_ptr) };

    assert_eq!(
        expect_variant_value(
            expect_result_err_payload(super::aurora_direct_process_start(
                string_vec(&["__definitely_missing_aurora_process_start__"]),
                boxed_value(Value::Unit),
                super::aurora_direct_map_empty(),
                super::aurora_direct_process_null(),
                super::aurora_direct_process_null(),
                super::aurora_direct_process_null(),
                bool_value(false),
            )),
            "Error",
            "Spawn",
        )
        .len(),
        1
    );
    assert_eq!(
        expect_variant_value(
            expect_result_err_payload(super::aurora_direct_process_run(
                string_vec(&["__definitely_missing_aurora_process_run__"]),
                boxed_value(Value::Unit),
                super::aurora_direct_map_empty(),
                super::aurora_direct_process_null(),
                super::aurora_direct_process_null(),
                super::aurora_direct_process_null(),
                boxed_value(Value::Unit),
                bool_value(false),
            )),
            "Error",
            "Spawn",
        )
        .len(),
        1
    );

    let slow_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "sleep 1".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("slow child should spawn");
    let slow_child_ptr = boxed_value(Value::ProcessChild(slow_child));
    assert!(expect_variant_ptr(
        super::aurora_direct_process_child_wait(slow_child_ptr, duration_value(0)),
        "Wait",
        "TimedOut",
    )
    .is_empty());
    let wait_or_none = expect_result_ok_payload(super::aurora_direct_process_child_wait_or_none(
        slow_child_ptr,
        duration_value(0),
    ));
    assert!(matches!(
        wait_or_none,
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "None"
    ));
    assert!(expect_variant_value(
        expect_result_err_payload(super::aurora_direct_process_child_wait_ok(
            slow_child_ptr,
            duration_value(0)
        )),
        "Error",
        "TimedOut",
    )
    .is_empty());
    expect_unit(super::aurora_direct_process_child_close(slow_child_ptr));
    unsafe { release_value(slow_child_ptr) };

    let group = TaskGroupValue::new(&CancellationContext::default());
    let cancellation = group.child_cancellation();
    group.cancel();
    with_cancellation_scope(cancellation.clone(), || {
        let cancelled_run = expect_result_err_payload(super::aurora_direct_process_run(
            string_vec(&["/bin/sh", "-c", "sleep 1"]),
            boxed_value(Value::Unit),
            super::aurora_direct_map_empty(),
            super::aurora_direct_process_null(),
            super::aurora_direct_process_null(),
            super::aurora_direct_process_null(),
            duration_value(1_000),
            bool_value(false),
        ));
        assert!(expect_variant_value(cancelled_run, "Error", "Cancelled").is_empty());
    });

    let cancelled_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "sleep 1".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("cancelled wait child should spawn");
    let cancelled_child_ptr = boxed_value(Value::ProcessChild(cancelled_child));
    with_cancellation_scope(cancellation.clone(), || {
        assert!(expect_variant_ptr(
            super::aurora_direct_process_child_wait(cancelled_child_ptr, duration_value(1_000)),
            "Wait",
            "Cancelled",
        )
        .is_empty());
        assert!(expect_variant_value(
            expect_result_err_payload(super::aurora_direct_process_child_wait_or_none(
                cancelled_child_ptr,
                duration_value(1_000),
            )),
            "Error",
            "Cancelled",
        )
        .is_empty());
        assert!(expect_variant_value(
            expect_result_err_payload(super::aurora_direct_process_child_wait_ok(
                cancelled_child_ptr,
                duration_value(1_000),
            )),
            "Error",
            "Cancelled",
        )
        .is_empty());
        expect_unit(super::aurora_direct_process_child_close(
            cancelled_child_ptr,
        ));
    });
    unsafe { release_value(cancelled_child_ptr) };

    let pipe_child = ProcessChildValue::spawn(
        vec!["/bin/sh".to_string(), "-c".to_string(), "cat".to_string()],
        None,
        Vec::new(),
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("pipe child should spawn");
    let pipe_child_ptr = boxed_value(Value::ProcessChild(pipe_child));
    let stdin_pipe = process_pipe_from_option(
        super::aurora_direct_process_child_stdin(pipe_child_ptr),
        "stdin",
    );
    let stdout_pipe = process_pipe_from_option(
        super::aurora_direct_process_child_stdout(pipe_child_ptr),
        "stdout",
    );
    assert_process_io_error(expect_result_err_payload(
        super::aurora_direct_process_pipe_read_all(boxed_value(Value::ProcessPipe(
            stdin_pipe.clone(),
        ))),
    ));
    assert_process_io_error(expect_result_err_payload(
        super::aurora_direct_process_pipe_read_line(
            boxed_value(Value::ProcessPipe(stdin_pipe.clone())),
            duration_value(0),
        ),
    ));
    assert_process_io_error(expect_result_err_payload(
        super::aurora_direct_process_pipe_read_bytes(
            boxed_value(Value::ProcessPipe(stdin_pipe.clone())),
            int_value(4),
            duration_value(0),
        ),
    ));
    assert_process_io_error(expect_result_err_payload(
        super::aurora_direct_process_pipe_write_all(
            boxed_value(Value::ProcessPipe(stdout_pipe.clone())),
            string_value("payload"),
            duration_value(0),
        ),
    ));
    assert_process_io_error(expect_result_err_payload(
        super::aurora_direct_process_pipe_write_bytes(
            boxed_value(Value::ProcessPipe(stdout_pipe.clone())),
            int_vec(&[1, 2, 3]),
            duration_value(0),
        ),
    ));
    expect_unit(super::aurora_direct_process_pipe_close(boxed_value(
        Value::ProcessPipe(stdin_pipe),
    )));
    expect_unit(super::aurora_direct_process_pipe_close(boxed_value(
        Value::ProcessPipe(stdout_pipe),
    )));
    expect_unit(super::aurora_direct_process_child_close(pipe_child_ptr));
    unsafe { release_value(pipe_child_ptr) };

    let empty_supervisor_ptr = boxed_value(Value::ProcessSupervisor(ProcessSupervisorValue::new()));
    assert!(expect_variant_ptr(
        super::aurora_direct_process_supervisor_wait(empty_supervisor_ptr, duration_value(0)),
        "SupervisorWait",
        "TimedOut",
    )
    .is_empty());
    expect_result_ok_unit(super::aurora_direct_process_supervisor_start(
        empty_supervisor_ptr,
        string_value("worker"),
        string_vec(&["/bin/sh", "-c", "sleep 1"]),
        boxed_value(Value::Unit),
        super::aurora_direct_map_empty(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        boxed_value(Value::EnumVariant(EnumVariantValue {
            enum_name: "process.RestartPolicy".to_string(),
            variant_name: "Never".to_string(),
            payloads: Vec::new(),
        })),
        duration_value(0),
        int_value(-1),
        bool_value(false),
    ));
    with_cancellation_scope(cancellation, || {
        assert!(expect_variant_ptr(
            super::aurora_direct_process_supervisor_wait(
                empty_supervisor_ptr,
                duration_value(1_000),
            ),
            "SupervisorWait",
            "Cancelled",
        )
        .is_empty());
        assert!(expect_variant_value(
            expect_result_err_payload(super::aurora_direct_process_supervisor_wait_or_none(
                empty_supervisor_ptr,
                duration_value(1_000),
            )),
            "Error",
            "Cancelled",
        )
        .is_empty());
    });
    expect_result_ok_unit(super::aurora_direct_process_supervisor_stop(
        empty_supervisor_ptr,
    ));
    expect_unit(super::aurora_direct_process_supervisor_close(
        empty_supervisor_ptr,
    ));
    unsafe { release_value(empty_supervisor_ptr) };
}

#[test]
fn native_runtime_direct_process_run_wrapper_covers_timeout_result_path() {
    let timed_out = expect_result_err_payload(super::aurora_direct_process_run(
        string_vec(&["/bin/sh", "-c", "sleep 1"]),
        boxed_value(Value::Unit),
        super::aurora_direct_map_empty(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        duration_value(0),
        bool_value(false),
    ));
    assert!(expect_variant_value(timed_out, "Error", "TimedOut").is_empty());
}

#[test]
fn native_runtime_direct_process_supervisor_wrappers_cover_start_wait_and_stop_paths() {
    fn restart_policy_never() -> *mut OpaqueValue {
        boxed_value(Value::EnumVariant(EnumVariantValue {
            enum_name: "process.RestartPolicy".to_string(),
            variant_name: "Never".to_string(),
            payloads: Vec::new(),
        }))
    }

    let supervisor = match unsafe { take_value(super::aurora_direct_process_supervisor()) } {
        Value::ProcessSupervisor(supervisor) => supervisor,
        other => panic!("expected process.Supervisor, found {:?}", other),
    };
    let supervisor_ptr = boxed_value(Value::ProcessSupervisor(supervisor.clone()));
    assert_eq!(
        super::aurora_direct_process_supervisor_is_empty(supervisor_ptr),
        1
    );

    let no_command = expect_result_err_payload(super::aurora_direct_process_supervisor_start(
        supervisor_ptr,
        string_value("empty"),
        super::aurora_direct_vec_empty(),
        boxed_value(Value::Unit),
        super::aurora_direct_map_empty(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        restart_policy_never(),
        duration_value(0),
        int_value(-1),
        bool_value(false),
    ));
    assert!(expect_variant_value(no_command, "Error", "NoCommand").is_empty());

    expect_result_ok_unit(super::aurora_direct_process_supervisor_start(
        supervisor_ptr,
        string_value("worker"),
        string_vec(&["/bin/sh", "-c", "exit 0"]),
        boxed_value(Value::Unit),
        super::aurora_direct_map_empty(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        super::aurora_direct_process_null(),
        restart_policy_never(),
        duration_value(0),
        int_value(-1),
        bool_value(false),
    ));
    assert_eq!(
        super::aurora_direct_process_supervisor_is_empty(supervisor_ptr),
        0
    );

    let wait_payloads = expect_variant_ptr(
        super::aurora_direct_process_supervisor_wait(supervisor_ptr, duration_value(5_000)),
        "SupervisorWait",
        "Event",
    );
    let event = match wait_payloads.as_slice() {
        [Value::EnumVariant(event)] => event,
        other => panic!("expected supervisor event payload, found {:?}", other),
    };
    assert_eq!(event.enum_name, "SupervisorEvent");
    assert_eq!(event.variant_name, "Exited");
    assert!(matches!(event.payloads.as_slice(), [Value::String(name), ..] if name == "worker"));
    assert_eq!(
        super::aurora_direct_process_supervisor_is_empty(supervisor_ptr),
        1
    );

    let empty_wait = expect_result_ok_payload(
        super::aurora_direct_process_supervisor_wait_or_none(supervisor_ptr, duration_value(0)),
    );
    assert!(matches!(
        empty_wait,
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "None"
    ));

    expect_result_ok_unit(super::aurora_direct_process_supervisor_stop(supervisor_ptr));
    expect_unit(super::aurora_direct_process_supervisor_close(
        supervisor_ptr,
    ));
    unsafe { release_value(supervisor_ptr) };
}

#[test]
fn native_runtime_direct_filesystem_wrappers_cover_file_success_paths() {
    let root = std::env::temp_dir().join(format!(
        "aurora-native-fs-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("temp fs root should be created");
    let data_path = root.join("data.txt");
    let data = data_path
        .to_str()
        .expect("temp data path should be valid UTF-8")
        .to_string();
    let child_dir_path = root.join("child");
    let child_dir = child_dir_path
        .to_str()
        .expect("temp child path should be valid UTF-8")
        .to_string();
    let second_path = root.join("second.txt");
    let second = second_path
        .to_str()
        .expect("second path should be valid UTF-8")
        .to_string();
    let root_text = root
        .to_str()
        .expect("temp root path should be valid UTF-8")
        .to_string();

    expect_result_ok_unit(super::aurora_direct_fs_write_string(
        string_value(&data),
        string_value("one"),
    ));
    assert!(expect_bool_boxed(super::aurora_direct_fs_exists(
        string_value(&data)
    )));
    expect_result_ok_unit(super::aurora_direct_fs_append_string(
        string_value(&data),
        string_value("two"),
    ));
    assert_eq!(
        expect_result_ok_string(super::aurora_direct_fs_read_to_string(string_value(&data))),
        "onetwo"
    );

    expect_result_ok_unit(super::aurora_direct_fs_write_bytes(
        string_value(&data),
        int_vec(&[65, 66]),
    ));
    expect_result_ok_unit(super::aurora_direct_fs_append_bytes(
        string_value(&data),
        int_vec(&[67]),
    ));
    assert_eq!(
        expect_result_ok_vec_ints(super::aurora_direct_fs_read_bytes(string_value(&data))),
        vec![65, 66, 67]
    );

    expect_result_ok_unit(super::aurora_direct_fs_create_dir(string_value(&child_dir)));
    let names =
        expect_result_ok_vec_strings(super::aurora_direct_fs_read_dir(string_value(&root_text)));
    assert!(names.contains(&"child".to_string()));
    assert!(names.contains(&"data.txt".to_string()));

    let file_payload = expect_variant_ptr(
        super::aurora_direct_fs_open(string_value(&data)),
        "Result",
        "Ok",
    );
    let file = match file_payload.as_slice() {
        [Value::File(file)] => file.clone(),
        other => panic!("expected opened fs.File, found {:?}", other),
    };
    let file_ptr = boxed_value(Value::File(file));
    assert_eq!(
        expect_result_ok_string(super::aurora_direct_file_read_all(file_ptr)),
        "ABC"
    );
    expect_unit(super::aurora_direct_file_close(file_ptr));
    unsafe { release_value(file_ptr) };

    let bytes_file_payload = expect_variant_ptr(
        super::aurora_direct_fs_open(string_value(&data)),
        "Result",
        "Ok",
    );
    let bytes_file = match bytes_file_payload.as_slice() {
        [Value::File(file)] => file.clone(),
        other => panic!("expected opened fs.File for bytes, found {:?}", other),
    };
    let bytes_file_ptr = boxed_value(Value::File(bytes_file));
    assert_eq!(
        expect_result_ok_vec_ints(super::aurora_direct_file_read_bytes(bytes_file_ptr)),
        vec![65, 66, 67]
    );
    expect_unit(super::aurora_direct_file_close(bytes_file_ptr));
    unsafe { release_value(bytes_file_ptr) };

    let created_payload = expect_variant_ptr(
        super::aurora_direct_fs_create(string_value(&second)),
        "Result",
        "Ok",
    );
    let created = match created_payload.as_slice() {
        [Value::File(file)] => file.clone(),
        other => panic!("expected created fs.File, found {:?}", other),
    };
    let created_ptr = boxed_value(Value::File(created));
    expect_result_ok_unit(super::aurora_direct_file_write_all(
        created_ptr,
        string_value("hi"),
    ));
    expect_result_ok_unit(super::aurora_direct_file_write_bytes(
        created_ptr,
        int_vec(&[33]),
    ));
    expect_result_ok_unit(super::aurora_direct_file_flush(created_ptr));
    expect_unit(super::aurora_direct_file_close(created_ptr));
    unsafe { release_value(created_ptr) };
    assert_eq!(
        expect_result_ok_string(super::aurora_direct_fs_read_to_string(string_value(
            &second
        ))),
        "hi!"
    );

    let append_payload = expect_variant_ptr(
        super::aurora_direct_fs_append(string_value(&second)),
        "Result",
        "Ok",
    );
    let append_file = match append_payload.as_slice() {
        [Value::File(file)] => file.clone(),
        other => panic!("expected append fs.File, found {:?}", other),
    };
    let append_ptr = boxed_value(Value::File(append_file));
    expect_result_ok_unit(super::aurora_direct_file_write_all(
        append_ptr,
        string_value(" again"),
    ));
    expect_unit(super::aurora_direct_file_close(append_ptr));
    unsafe { release_value(append_ptr) };
    assert_eq!(
        expect_result_ok_string(super::aurora_direct_fs_read_to_string(string_value(
            &second
        ))),
        "hi! again"
    );

    let close_payload = expect_variant_ptr(
        super::aurora_direct_fs_open(string_value(&second)),
        "Result",
        "Ok",
    );
    let close_file = match close_payload.as_slice() {
        [Value::File(file)] => file.clone(),
        other => panic!(
            "expected opened fs.File for close(value), found {:?}",
            other
        ),
    };
    let close_ptr = boxed_value(Value::File(close_file));
    expect_unit(super::aurora_direct_close_value(close_ptr, 0));
    unsafe { release_value(close_ptr) };

    expect_result_ok_unit(super::aurora_direct_fs_remove_file(string_value(&data)));
    assert!(!expect_bool_boxed(super::aurora_direct_fs_exists(
        string_value(&data)
    )));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn native_runtime_direct_filesystem_wrappers_cover_io_error_results() {
    fn expect_io_result_error(ptr: *mut OpaqueValue) {
        assert!(matches!(
            expect_result_err_payload(ptr),
            Value::EnumVariant(_)
        ));
    }

    let root = std::env::temp_dir().join(format!(
        "aurora-native-fs-errors-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("temp fs error root should be created");
    let missing = root.join("missing.txt");
    let missing_text = missing
        .to_str()
        .expect("missing path should be valid UTF-8")
        .to_string();
    let directory = root.join("dir");
    std::fs::create_dir_all(&directory).expect("temp child directory should be created");
    let directory_text = directory
        .to_str()
        .expect("directory path should be valid UTF-8")
        .to_string();
    let file_path = root.join("file.txt");
    std::fs::write(&file_path, "data").expect("temp file should be written");
    let file_text = file_path
        .to_str()
        .expect("file path should be valid UTF-8")
        .to_string();

    expect_io_result_error(super::aurora_direct_fs_read_to_string(string_value(
        &missing_text,
    )));
    expect_io_result_error(super::aurora_direct_fs_read_bytes(string_value(
        &missing_text,
    )));
    expect_io_result_error(super::aurora_direct_fs_read_dir(string_value(
        &missing_text,
    )));
    expect_io_result_error(super::aurora_direct_fs_open(string_value(&missing_text)));
    expect_io_result_error(super::aurora_direct_fs_remove_file(string_value(
        &missing_text,
    )));

    expect_io_result_error(super::aurora_direct_fs_write_string(
        string_value(&directory_text),
        string_value("data"),
    ));
    expect_io_result_error(super::aurora_direct_fs_write_bytes(
        string_value(&directory_text),
        int_vec(&[1, 2, 3]),
    ));
    expect_io_result_error(super::aurora_direct_fs_append_string(
        string_value(&directory_text),
        string_value("data"),
    ));
    expect_io_result_error(super::aurora_direct_fs_append_bytes(
        string_value(&directory_text),
        int_vec(&[4, 5, 6]),
    ));
    expect_io_result_error(super::aurora_direct_fs_create(string_value(
        &directory_text,
    )));
    expect_io_result_error(super::aurora_direct_fs_append(string_value(
        &directory_text,
    )));
    expect_io_result_error(super::aurora_direct_fs_create_dir(string_value(&file_text)));

    let file_payload = expect_variant_ptr(
        super::aurora_direct_fs_open(string_value(&file_text)),
        "Result",
        "Ok",
    );
    let file = match file_payload.as_slice() {
        [Value::File(file)] => file.clone(),
        other => panic!("expected fs.File, found {:?}", other),
    };
    let file_ptr = boxed_value(Value::File(file));
    expect_unit(super::aurora_direct_file_close(file_ptr));
    expect_io_result_error(super::aurora_direct_file_read_all(file_ptr));
    expect_io_result_error(super::aurora_direct_file_read_bytes(file_ptr));
    expect_io_result_error(super::aurora_direct_file_write_all(
        file_ptr,
        string_value("closed"),
    ));
    expect_io_result_error(super::aurora_direct_file_write_bytes(
        file_ptr,
        int_vec(&[7, 8]),
    ));
    expect_io_result_error(super::aurora_direct_file_flush(file_ptr));
    unsafe { release_value(file_ptr) };

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn native_runtime_direct_network_wrappers_cover_tcp_udp_http_success_paths() {
    let timeout = duration_value(5_000);

    let tcp_listener = match expect_result_ok_payload(super::aurora_direct_net_listen(
        string_value("127.0.0.1:0"),
    )) {
        Value::TcpListener(listener) => listener,
        other => panic!("expected net.TcpListener, found {:?}", other),
    };
    let tcp_address = expect_result_ok_string(super::aurora_direct_tcp_listener_local_addr(
        boxed_value(Value::TcpListener(tcp_listener.clone())),
    ));
    let tcp_server_listener = tcp_listener.clone();
    let tcp_server = thread::spawn(move || {
        let accepted = match expect_result_ok_payload(super::aurora_direct_tcp_listener_accept(
            boxed_value(Value::TcpListener(tcp_server_listener)),
            duration_value(5_000),
        )) {
            Value::TcpStream(stream) => stream,
            other => panic!("expected accepted net.TcpStream, found {:?}", other),
        };
        let accepted_ptr = boxed_value(Value::TcpStream(accepted.clone()));
        let line = expect_option_some_payload(expect_result_ok_payload(
            super::aurora_direct_tcp_stream_read_line(accepted_ptr, duration_value(5_000)),
        ));
        assert_eq!(line, Value::String("ping".to_string()));
        expect_result_ok_unit(super::aurora_direct_tcp_stream_write_bytes(
            accepted_ptr,
            int_vec(&[112, 111, 110, 103]),
            duration_value(5_000),
        ));
        expect_result_ok_unit(super::aurora_direct_tcp_stream_flush(accepted_ptr));
        assert!(
            expect_result_ok_string(super::aurora_direct_tcp_stream_local_addr(accepted_ptr))
                .contains("127.0.0.1")
        );
        assert!(
            expect_result_ok_string(super::aurora_direct_tcp_stream_peer_addr(accepted_ptr))
                .contains("127.0.0.1")
        );
        expect_unit(super::aurora_direct_tcp_stream_close(accepted_ptr));
    });
    let tcp_client = match expect_result_ok_payload(super::aurora_direct_net_connect(string_value(
        &tcp_address,
    ))) {
        Value::TcpStream(stream) => stream,
        other => panic!("expected connected net.TcpStream, found {:?}", other),
    };
    let tcp_client_ptr = boxed_value(Value::TcpStream(tcp_client));
    expect_result_ok_unit(super::aurora_direct_tcp_stream_write_all(
        tcp_client_ptr,
        string_value("ping\n"),
        timeout,
    ));
    expect_result_ok_unit(super::aurora_direct_tcp_stream_shutdown_write(
        tcp_client_ptr,
    ));
    assert_eq!(
        expect_result_ok_vec_ints(super::aurora_direct_tcp_stream_read_exact(
            tcp_client_ptr,
            int_value(4),
            timeout,
        )),
        vec![112, 111, 110, 103]
    );
    expect_unit(super::aurora_direct_tcp_stream_close(tcp_client_ptr));
    tcp_server
        .join()
        .expect("tcp direct wrapper server should join");
    expect_unit(super::aurora_direct_tcp_listener_close(boxed_value(
        Value::TcpListener(tcp_listener),
    )));

    let shutdown_listener = match expect_result_ok_payload(super::aurora_direct_net_listen(
        string_value("127.0.0.1:0"),
    )) {
        Value::TcpListener(listener) => listener,
        other => panic!("expected shutdown net.TcpListener, found {:?}", other),
    };
    let shutdown_address = expect_result_ok_string(super::aurora_direct_tcp_listener_local_addr(
        boxed_value(Value::TcpListener(shutdown_listener.clone())),
    ));
    let shutdown_server_listener = shutdown_listener.clone();
    let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let shutdown_server = thread::spawn(move || {
        let accepted = match expect_result_ok_payload(super::aurora_direct_tcp_listener_accept(
            boxed_value(Value::TcpListener(shutdown_server_listener)),
            duration_value(5_000),
        )) {
            Value::TcpStream(stream) => stream,
            other => panic!(
                "expected shutdown accepted net.TcpStream, found {:?}",
                other
            ),
        };
        let accepted_ptr = boxed_value(Value::TcpStream(accepted));
        accepted_tx
            .send(())
            .expect("shutdown server should signal accepted connection");
        done_rx
            .recv_timeout(StdDuration::from_secs(5))
            .expect("shutdown client should finish");
        expect_unit(super::aurora_direct_tcp_stream_close(accepted_ptr));
    });
    let shutdown_client = match expect_result_ok_payload(super::aurora_direct_net_connect(
        string_value(&shutdown_address),
    )) {
        Value::TcpStream(stream) => stream,
        other => panic!("expected shutdown client net.TcpStream, found {:?}", other),
    };
    let shutdown_client_ptr = boxed_value(Value::TcpStream(shutdown_client));
    accepted_rx
        .recv_timeout(StdDuration::from_secs(5))
        .expect("shutdown server should accept connection");
    let _ = unsafe {
        take_value(super::aurora_direct_tcp_stream_shutdown_read(
            shutdown_client_ptr,
        ))
    };
    let _ = unsafe {
        take_value(super::aurora_direct_tcp_stream_shutdown_both(
            shutdown_client_ptr,
        ))
    };
    expect_unit(super::aurora_direct_tcp_stream_close(shutdown_client_ptr));
    done_tx
        .send(())
        .expect("shutdown client should signal completion");
    shutdown_server
        .join()
        .expect("tcp shutdown wrapper server should join");
    expect_unit(super::aurora_direct_tcp_listener_close(boxed_value(
        Value::TcpListener(shutdown_listener),
    )));

    let udp_sender = match expect_result_ok_payload(super::aurora_direct_net_udp_bind(
        string_value("127.0.0.1:0"),
    )) {
        Value::UdpSocket(socket) => socket,
        other => panic!("expected sender net.UdpSocket, found {:?}", other),
    };
    let udp_receiver = match expect_result_ok_payload(super::aurora_direct_net_udp_bind(
        string_value("127.0.0.1:0"),
    )) {
        Value::UdpSocket(socket) => socket,
        other => panic!("expected receiver net.UdpSocket, found {:?}", other),
    };
    let udp_receiver_address = expect_result_ok_string(super::aurora_direct_udp_socket_local_addr(
        boxed_value(Value::UdpSocket(udp_receiver.clone())),
    ));
    expect_result_ok_unit(super::aurora_direct_udp_socket_send_text(
        boxed_value(Value::UdpSocket(udp_sender.clone())),
        string_value(&udp_receiver_address),
        string_value("hello"),
        timeout,
    ));
    let datagram = match expect_option_some_payload(expect_result_ok_payload(
        super::aurora_direct_udp_socket_recv_from(
            boxed_value(Value::UdpSocket(udp_receiver.clone())),
            int_value(64),
            timeout,
        ),
    )) {
        Value::UdpDatagram(datagram) => datagram,
        other => panic!("expected net.UdpDatagram, found {:?}", other),
    };
    let reply_address = expect_string(super::aurora_direct_udp_datagram_address(boxed_value(
        Value::UdpDatagram(datagram.clone()),
    )));
    assert_eq!(
        expect_vec_ints(super::aurora_direct_udp_datagram_bytes(boxed_value(
            Value::UdpDatagram(datagram.clone()),
        ))),
        vec![104, 101, 108, 108, 111]
    );
    assert_eq!(
        expect_result_ok_string(super::aurora_direct_udp_datagram_text(boxed_value(
            Value::UdpDatagram(datagram),
        ))),
        "hello"
    );
    expect_result_ok_unit(super::aurora_direct_udp_socket_send_bytes(
        boxed_value(Value::UdpSocket(udp_receiver.clone())),
        string_value(&reply_address),
        int_vec(&[111, 107]),
        timeout,
    ));
    let udp_reply = expect_option_some_payload(expect_result_ok_payload(
        super::aurora_direct_udp_socket_recv(
            boxed_value(Value::UdpSocket(udp_sender.clone())),
            int_value(64),
            timeout,
        ),
    ));
    assert_eq!(expect_vec_ints(boxed_value(udp_reply)), vec![111, 107]);
    let udp_peer_error = expect_result_err_payload(super::aurora_direct_udp_socket_peer_addr(
        boxed_value(Value::UdpSocket(udp_sender.clone())),
    ));
    assert!(matches!(udp_peer_error, Value::EnumVariant(_)));
    expect_unit(super::aurora_direct_udp_socket_close(boxed_value(
        Value::UdpSocket(udp_sender),
    )));
    expect_unit(super::aurora_direct_udp_socket_close(boxed_value(
        Value::UdpSocket(udp_receiver),
    )));

    let http_listener = match expect_result_ok_payload(super::aurora_direct_net_http_listen(
        string_value("127.0.0.1:0"),
    )) {
        Value::HttpListener(listener) => listener,
        other => panic!("expected net.HttpListener, found {:?}", other),
    };
    let http_address = expect_result_ok_string(super::aurora_direct_http_listener_local_addr(
        boxed_value(Value::HttpListener(http_listener.clone())),
    ));
    let http_server_listener = http_listener.clone();
    let http_server = thread::spawn(move || {
        for (path, expected_body, response_body, use_bytes) in [
            ("/direct-text", "hello", "ack", false),
            ("/direct-bytes", "raw", "raw-ok", true),
        ] {
            let exchange =
                match expect_result_ok_payload(super::aurora_direct_http_listener_accept(
                    boxed_value(Value::HttpListener(http_server_listener.clone())),
                    duration_value(5_000),
                )) {
                    Value::HttpExchange(exchange) => exchange,
                    other => panic!("expected net.HttpExchange, found {:?}", other),
                };
            assert_eq!(
                expect_string(super::aurora_direct_http_exchange_method(boxed_value(
                    Value::HttpExchange(exchange.clone()),
                ))),
                "POST"
            );
            assert_eq!(
                expect_string(super::aurora_direct_http_exchange_path(boxed_value(
                    Value::HttpExchange(exchange.clone()),
                ))),
                path
            );
            match unsafe {
                take_value(super::aurora_direct_http_exchange_headers(boxed_value(
                    Value::HttpExchange(exchange.clone()),
                )))
            } {
                Value::Map(headers) => assert!(!headers.entries.is_empty()),
                other => panic!("expected HTTP header map, found {:?}", other),
            }
            assert_eq!(
                expect_result_ok_string(super::aurora_direct_http_exchange_body_text(boxed_value(
                    Value::HttpExchange(exchange.clone()),
                ))),
                expected_body
            );
            assert_eq!(
                expect_vec_ints(super::aurora_direct_http_exchange_body_bytes(boxed_value(
                    Value::HttpExchange(exchange.clone()),
                ))),
                expected_body
                    .as_bytes()
                    .iter()
                    .map(|byte| i128::from(*byte))
                    .collect::<Vec<_>>()
            );
            if use_bytes {
                expect_result_ok_unit(super::aurora_direct_http_exchange_respond_bytes(
                    boxed_value(Value::HttpExchange(exchange)),
                    int_value(200),
                    int_vec(
                        &response_body
                            .as_bytes()
                            .iter()
                            .map(|byte| i64::from(*byte))
                            .collect::<Vec<_>>(),
                    ),
                    string_map(&[("x-direct", "bytes")]),
                ));
            } else {
                expect_result_ok_unit(super::aurora_direct_http_exchange_respond_text(
                    boxed_value(Value::HttpExchange(exchange)),
                    int_value(200),
                    string_value(response_body),
                    string_map(&[("x-direct", "text")]),
                ));
            }
        }
    });
    let text_response = match expect_result_ok_payload(super::aurora_direct_net_http_request_text(
        string_value("POST"),
        string_value(&format!("http://{http_address}/direct-text")),
        string_value("hello"),
        string_map(&[("x-client", "text")]),
    )) {
        Value::HttpResponse(response) => response,
        other => panic!("expected net.HttpResponse, found {:?}", other),
    };
    assert_eq!(
        super::aurora_direct_http_response_status(boxed_value(Value::HttpResponse(
            text_response.clone(),
        ))),
        200
    );
    assert_eq!(
        expect_string(super::aurora_direct_http_response_reason(boxed_value(
            Value::HttpResponse(text_response.clone()),
        ))),
        "OK"
    );
    match unsafe {
        take_value(super::aurora_direct_http_response_headers(boxed_value(
            Value::HttpResponse(text_response.clone()),
        )))
    } {
        Value::Map(headers) => assert!(!headers.entries.is_empty()),
        other => panic!("expected HTTP response header map, found {:?}", other),
    }
    assert_eq!(
        expect_result_ok_string(super::aurora_direct_http_response_text(boxed_value(
            Value::HttpResponse(text_response.clone()),
        ))),
        "ack"
    );
    assert_eq!(
        expect_vec_ints(super::aurora_direct_http_response_bytes(boxed_value(
            Value::HttpResponse(text_response),
        ))),
        vec![97, 99, 107]
    );

    let bytes_response =
        match expect_result_ok_payload(super::aurora_direct_net_http_request_bytes_timeout(
            string_value("POST"),
            string_value(&format!("http://{http_address}/direct-bytes")),
            int_vec(&[114, 97, 119]),
            string_map(&[("x-client", "bytes")]),
            timeout,
        )) {
            Value::HttpResponse(response) => response,
            other => panic!("expected net.HttpResponse, found {:?}", other),
        };
    assert_eq!(
        expect_vec_ints(super::aurora_direct_http_response_bytes(boxed_value(
            Value::HttpResponse(bytes_response),
        ))),
        vec![114, 97, 119, 45, 111, 107]
    );
    http_server
        .join()
        .expect("http direct wrapper server should join");
    expect_unit(super::aurora_direct_http_listener_close(boxed_value(
        Value::HttpListener(http_listener),
    )));

    let websocket_listener = match expect_result_ok_payload(
        super::aurora_direct_net_websocket_listen(string_value("127.0.0.1:0")),
    ) {
        Value::WebSocketListener(listener) => listener,
        other => panic!("expected net.WebSocketListener, found {:?}", other),
    };
    let websocket_address =
        expect_result_ok_string(super::aurora_direct_websocket_listener_local_addr(
            boxed_value(Value::WebSocketListener(websocket_listener.clone())),
        ));
    let websocket_server_listener = websocket_listener.clone();
    let websocket_server = thread::spawn(move || {
        let server_socket =
            match expect_result_ok_payload(super::aurora_direct_websocket_listener_accept(
                boxed_value(Value::WebSocketListener(websocket_server_listener)),
                duration_value(5_000),
            )) {
                Value::WebSocket(socket) => socket,
                other => panic!("expected server net.WebSocket, found {:?}", other),
            };
        let server_ptr = boxed_value(Value::WebSocket(server_socket));
        let text = expect_option_some_payload(expect_result_ok_payload(
            super::aurora_direct_websocket_recv_text(server_ptr, duration_value(5_000)),
        ));
        assert_eq!(text, Value::String("hello websocket".to_string()));
        expect_result_ok_unit(super::aurora_direct_websocket_send_bytes(
            server_ptr,
            int_vec(&[111, 107]),
            duration_value(5_000),
        ));
        let bytes = expect_option_some_payload(expect_result_ok_payload(
            super::aurora_direct_websocket_recv_bytes(server_ptr, duration_value(5_000)),
        ));
        assert_eq!(expect_vec_ints(boxed_value(bytes)), vec![1, 2, 3]);
        expect_result_ok_unit(super::aurora_direct_websocket_send_text(
            server_ptr,
            string_value("done"),
            duration_value(5_000),
        ));
        expect_unit(super::aurora_direct_websocket_close(server_ptr));
    });
    let websocket_client =
        match expect_result_ok_payload(super::aurora_direct_net_websocket_connect_timeout(
            string_value(&format!("ws://{websocket_address}")),
            timeout,
        )) {
            Value::WebSocket(socket) => socket,
            other => panic!("expected client net.WebSocket, found {:?}", other),
        };
    let websocket_client_ptr = boxed_value(Value::WebSocket(websocket_client));
    expect_result_ok_unit(super::aurora_direct_websocket_send_text(
        websocket_client_ptr,
        string_value("hello websocket"),
        timeout,
    ));
    let websocket_reply = expect_option_some_payload(expect_result_ok_payload(
        super::aurora_direct_websocket_recv_bytes(websocket_client_ptr, timeout),
    ));
    assert_eq!(
        expect_vec_ints(boxed_value(websocket_reply)),
        vec![111, 107]
    );
    expect_result_ok_unit(super::aurora_direct_websocket_send_bytes(
        websocket_client_ptr,
        int_vec(&[1, 2, 3]),
        timeout,
    ));
    let websocket_done = expect_option_some_payload(expect_result_ok_payload(
        super::aurora_direct_websocket_recv_text(websocket_client_ptr, timeout),
    ));
    assert_eq!(websocket_done, Value::String("done".to_string()));
    expect_unit(super::aurora_direct_websocket_close(websocket_client_ptr));
    websocket_server
        .join()
        .expect("websocket direct wrapper server should join");

    #[cfg(unix)]
    {
        let unix_socket_path = format!(
            "/tmp/a-ndw-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
                % 1_000_000
        );
        let _ = std::fs::remove_file(&unix_socket_path);
        let unix_listener = match expect_result_ok_payload(super::aurora_direct_net_unix_listen(
            string_value(&unix_socket_path),
        )) {
            Value::UnixListener(listener) => listener,
            other => panic!("expected net.UnixListener, found {:?}", other),
        };
        let unix_server_listener = unix_listener.clone();
        let unix_server = thread::spawn(move || {
            let server_stream =
                match expect_result_ok_payload(super::aurora_direct_unix_listener_accept(
                    boxed_value(Value::UnixListener(unix_server_listener)),
                    duration_value(5_000),
                )) {
                    Value::UnixStream(stream) => stream,
                    other => panic!("expected server net.UnixStream, found {:?}", other),
                };
            let server_ptr = boxed_value(Value::UnixStream(server_stream));
            let line = expect_option_some_payload(expect_result_ok_payload(
                super::aurora_direct_unix_stream_read_line(server_ptr, duration_value(5_000)),
            ));
            assert_eq!(line, Value::String("hello unix".to_string()));
            expect_result_ok_unit(super::aurora_direct_unix_stream_write_all(
                server_ptr,
                string_value("unix-ok"),
                duration_value(5_000),
            ));
            expect_unit(super::aurora_direct_unix_stream_close(server_ptr));
        });
        let unix_client = match expect_result_ok_payload(
            super::aurora_direct_net_unix_connect_timeout(string_value(&unix_socket_path), timeout),
        ) {
            Value::UnixStream(stream) => stream,
            other => panic!("expected client net.UnixStream, found {:?}", other),
        };
        let unix_client_ptr = boxed_value(Value::UnixStream(unix_client));
        expect_result_ok_unit(super::aurora_direct_unix_stream_write_all(
            unix_client_ptr,
            string_value("hello unix\n"),
            timeout,
        ));
        assert_eq!(
            expect_result_ok_vec_ints(super::aurora_direct_unix_stream_read_exact(
                unix_client_ptr,
                int_value(7),
                timeout,
            )),
            vec![117, 110, 105, 120, 45, 111, 107]
        );
        expect_unit(super::aurora_direct_unix_stream_close(unix_client_ptr));
        unix_server
            .join()
            .expect("unix direct wrapper server should join");
        expect_unit(super::aurora_direct_unix_listener_close(boxed_value(
            Value::UnixListener(unix_listener),
        )));
        let _ = std::fs::remove_file(&unix_socket_path);
    }
}

#[test]
fn native_runtime_direct_network_wrappers_cover_timeout_and_error_results() {
    fn expect_io_result_error(ptr: *mut OpaqueValue) {
        assert!(matches!(
            expect_result_err_payload(ptr),
            Value::EnumVariant(_)
        ));
    }

    expect_io_result_error(super::aurora_direct_net_connect(string_value(
        "127.0.0.1:0",
    )));
    expect_io_result_error(super::aurora_direct_net_connect_timeout(
        string_value("127.0.0.1:0"),
        duration_value(1),
    ));
    expect_io_result_error(super::aurora_direct_net_listen(string_value(
        "127.0.0.1:not-a-port",
    )));

    let tcp_listener = match expect_result_ok_payload(super::aurora_direct_net_listen(
        string_value("127.0.0.1:0"),
    )) {
        Value::TcpListener(listener) => listener,
        other => panic!("expected timeout net.TcpListener, found {:?}", other),
    };
    expect_io_result_error(super::aurora_direct_tcp_listener_accept(
        boxed_value(Value::TcpListener(tcp_listener.clone())),
        duration_value(0),
    ));
    expect_unit(super::aurora_direct_tcp_listener_close(boxed_value(
        Value::TcpListener(tcp_listener),
    )));

    expect_io_result_error(super::aurora_direct_net_udp_bind(string_value(
        "127.0.0.1:not-a-port",
    )));
    let udp_socket = match expect_result_ok_payload(super::aurora_direct_net_udp_bind(
        string_value("127.0.0.1:0"),
    )) {
        Value::UdpSocket(socket) => socket,
        other => panic!("expected timeout net.UdpSocket, found {:?}", other),
    };
    let udp_recv = expect_result_ok_payload(super::aurora_direct_udp_socket_recv(
        boxed_value(Value::UdpSocket(udp_socket.clone())),
        int_value(16),
        duration_value(0),
    ));
    assert!(matches!(
        udp_recv,
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "None"
    ));
    let udp_recv_from = expect_result_ok_payload(super::aurora_direct_udp_socket_recv_from(
        boxed_value(Value::UdpSocket(udp_socket.clone())),
        int_value(16),
        duration_value(0),
    ));
    assert!(matches!(
        udp_recv_from,
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "None"
    ));
    expect_unit(super::aurora_direct_udp_socket_close(boxed_value(
        Value::UdpSocket(udp_socket),
    )));

    expect_io_result_error(super::aurora_direct_net_http_listen(string_value(
        "127.0.0.1:not-a-port",
    )));
    let http_listener = match expect_result_ok_payload(super::aurora_direct_net_http_listen(
        string_value("127.0.0.1:0"),
    )) {
        Value::HttpListener(listener) => listener,
        other => panic!("expected timeout net.HttpListener, found {:?}", other),
    };
    expect_io_result_error(super::aurora_direct_http_listener_accept(
        boxed_value(Value::HttpListener(http_listener.clone())),
        duration_value(0),
    ));
    expect_unit(super::aurora_direct_http_listener_close(boxed_value(
        Value::HttpListener(http_listener),
    )));

    expect_io_result_error(super::aurora_direct_net_http_request_text_timeout(
        string_value("GET"),
        string_value("not-a-url"),
        string_value(""),
        string_map(&[]),
        duration_value(1),
    ));
    expect_io_result_error(super::aurora_direct_net_http_request_bytes(
        string_value("POST"),
        string_value("not-a-url"),
        int_vec(&[1, 2]),
        string_map(&[]),
    ));

    expect_io_result_error(super::aurora_direct_net_tls_listen(
        string_value("127.0.0.1:0"),
        string_value("/tmp/aurora-missing-cert.pem"),
        string_value("/tmp/aurora-missing-key.pem"),
    ));
    expect_io_result_error(super::aurora_direct_net_tls_connect(
        string_value("127.0.0.1:0"),
        string_value("localhost"),
        string_value("/tmp/aurora-missing-ca.pem"),
    ));
    expect_io_result_error(super::aurora_direct_net_tls_connect_timeout(
        string_value("127.0.0.1:0"),
        string_value("localhost"),
        string_value("/tmp/aurora-missing-ca.pem"),
        duration_value(1),
    ));

    expect_io_result_error(super::aurora_direct_net_websocket_listen(string_value(
        "127.0.0.1:not-a-port",
    )));
    expect_io_result_error(super::aurora_direct_net_websocket_connect(string_value(
        "not-a-url",
    )));

    let websocket_listener = match expect_result_ok_payload(
        super::aurora_direct_net_websocket_listen(string_value("127.0.0.1:0")),
    ) {
        Value::WebSocketListener(listener) => listener,
        other => panic!("expected timeout net.WebSocketListener, found {:?}", other),
    };
    expect_io_result_error(super::aurora_direct_websocket_listener_accept(
        boxed_value(Value::WebSocketListener(websocket_listener.clone())),
        duration_value(0),
    ));
    expect_unit(super::aurora_direct_close_value(
        boxed_value(Value::WebSocketListener(websocket_listener)),
        0,
    ));

    #[cfg(unix)]
    {
        let unix_socket_path = format!(
            "/tmp/a-ndw-error-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
                % 1_000_000
        );
        let _ = std::fs::remove_file(&unix_socket_path);
        let unix_listener = match expect_result_ok_payload(super::aurora_direct_net_unix_listen(
            string_value(&unix_socket_path),
        )) {
            Value::UnixListener(listener) => listener,
            other => panic!("expected timeout net.UnixListener, found {:?}", other),
        };
        expect_io_result_error(super::aurora_direct_unix_listener_accept(
            boxed_value(Value::UnixListener(unix_listener.clone())),
            duration_value(0),
        ));
        expect_unit(super::aurora_direct_unix_listener_close(boxed_value(
            Value::UnixListener(unix_listener),
        )));
        let _ = std::fs::remove_file(&unix_socket_path);
        expect_io_result_error(super::aurora_direct_net_unix_connect(string_value(
            &unix_socket_path,
        )));
    }
}

#[test]
fn sqrt_helper_matches_standard_library() {
    assert_eq!(super::aurora_direct_sqrt_f64(25.0), 5.0);
}

#[test]
fn direct_runtime_string_and_numeric_helpers_cover_builtin_surface() {
    assert_eq!(
        super::aurora_direct_string_len(string_value("é🎉e\u{301}")),
        4
    );
    assert_eq!(
        super::aurora_direct_string_byte_len(string_value("é🎉e\u{301}")),
        9
    );
    assert_eq!(
        super::aurora_direct_string_contains(string_value("  Aurora Repo  "), string_value("Repo"),),
        1
    );
    assert_eq!(
        super::aurora_direct_string_starts_with(
            string_value("  Aurora Repo  "),
            string_value("  A"),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_string_ends_with(string_value("  Aurora Repo  "), string_value("o  "),),
        1
    );
    assert_eq!(
        expect_vec_strings(super::aurora_direct_string_split(
            string_value("a,b,c"),
            string_value(","),
        )),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
    assert_eq!(
        expect_string(super::aurora_direct_string_replace(
            string_value("Aurora compiler"),
            string_value("compiler"),
            string_value("runtime"),
        )),
        "Aurora runtime"
    );
    assert_eq!(
        expect_string(super::aurora_direct_string_to_lower(string_value("AuRoRa"))),
        "aurora"
    );
    assert_eq!(
        expect_string(super::aurora_direct_string_to_upper(string_value("AuRoRa"))),
        "AURORA"
    );
    assert_eq!(
        expect_option_some_string(super::aurora_direct_string_strip_prefix(
            string_value("prefix-core"),
            string_value("prefix-"),
        )),
        "core"
    );
    expect_option_none(super::aurora_direct_string_strip_prefix(
        string_value("prefix-core"),
        string_value("core"),
    ));
    assert_eq!(
        expect_option_some_string(super::aurora_direct_string_strip_suffix(
            string_value("core-suffix"),
            string_value("-suffix"),
        )),
        "core"
    );
    expect_option_none(super::aurora_direct_string_strip_suffix(
        string_value("core-suffix"),
        string_value("prefix"),
    ));
    assert_eq!(
        expect_string(super::aurora_direct_string_trim(string_value(
            " \tAurora\n"
        ))),
        "Aurora"
    );
    assert_eq!(
        expect_string(super::aurora_direct_string_join(
            string_value(", "),
            string_vec(&["Ada", "Linus", "Grace"]),
        )),
        "Ada, Linus, Grace"
    );
    assert_eq!(expect_int(super::aurora_direct_abs(int_value(-7))), 7);
    assert_eq!(expect_int(super::aurora_direct_abs(int_value(7))), 7);
    assert_eq!(
        expect_float(super::aurora_direct_abs(float_value(-3.5))),
        3.5
    );
    assert_eq!(
        expect_int(super::aurora_direct_min(int_value(4), int_value(9))),
        4
    );
    assert_eq!(
        expect_int(super::aurora_direct_min(int_value(9), int_value(4))),
        4
    );
    assert_eq!(
        expect_float(super::aurora_direct_min(float_value(4.5), float_value(9.5))),
        4.5
    );
    assert_eq!(
        expect_float(super::aurora_direct_min(float_value(9.5), float_value(4.5))),
        4.5
    );
    assert_eq!(
        expect_int(super::aurora_direct_max(int_value(4), int_value(9))),
        9
    );
    assert_eq!(
        expect_int(super::aurora_direct_max(int_value(9), int_value(4))),
        9
    );
    assert_eq!(
        expect_float(super::aurora_direct_max(float_value(4.5), float_value(9.5))),
        9.5
    );
    assert_eq!(
        expect_float(super::aurora_direct_max(float_value(9.5), float_value(4.5))),
        9.5
    );
    assert_eq!(
        expect_float(super::aurora_direct_sqrt(float_value(81.0))),
        9.0
    );
    assert_eq!(
        expect_result_ok_int(super::aurora_direct_parse_int32(string_value("123"))),
        123
    );
    assert_eq!(
        expect_result_ok_int(super::aurora_direct_parse_int64(string_value("-456"))),
        -456
    );
    assert_eq!(
        expect_result_ok_float(super::aurora_direct_parse_float64(string_value("1.5e2"))),
        150.0
    );
    assert!(
        expect_result_err_string(super::aurora_direct_parse_int32(string_value("oops")))
            .contains("invalid")
    );
    assert!(
        expect_result_err_string(super::aurora_direct_parse_int64(string_value("oops")))
            .contains("invalid")
    );
    assert!(
        expect_result_err_string(super::aurora_direct_parse_float64(string_value("oops")))
            .contains("invalid")
    );
    assert!(
        expect_result_err_string(super::aurora_direct_parse_float64(string_value("inf")))
            .contains("float must be finite")
    );
    assert_eq!(
        expect_string(super::aurora_direct_stringify_value(bool_value(true))),
        "true"
    );
    expect_unit(super::aurora_direct_box_unit());
    assert_eq!(
        expect_int(super::aurora_direct_box_uint_literal(b"42".as_ptr(), 2)),
        42
    );
    assert_eq!(
        expect_string(super::aurora_direct_stringify_value(duration_value(5))),
        "5ms"
    );
}

#[test]
fn direct_runtime_vec_helpers_cover_collection_surface() {
    let vec = super::aurora_direct_vec_empty();
    assert_eq!(super::aurora_direct_vec_len(vec), 0);
    assert_eq!(super::aurora_direct_vec_is_empty(vec), 1);

    expect_unit(super::aurora_direct_vec_push_in_place(vec, int_value(1)));
    expect_unit(super::aurora_direct_vec_push_in_place(vec, int_value(2)));
    expect_unit(super::aurora_direct_vec_push_in_place(vec, int_value(3)));
    assert_eq!(super::aurora_direct_vec_len(vec), 3);
    assert_eq!(
        expect_option_some_int(super::aurora_direct_vec_pop_in_place(vec)),
        3
    );
    assert_eq!(
        expect_option_some_int(super::aurora_direct_vec_get(vec, 1)),
        2
    );
    assert_eq!(
        expect_option_some_int(super::aurora_direct_vec_set_in_place(vec, 1, int_value(5))),
        2
    );
    assert_eq!(
        expect_option_some_int(super::aurora_direct_vec_remove_in_place(vec, 0)),
        1
    );
    assert_eq!(super::aurora_direct_vec_contains(vec, int_value(5)), 1);
    assert_eq!(
        super::aurora_direct_vec_insert_in_place(vec, 1, int_value(8)),
        1
    );
    assert_eq!(super::aurora_direct_vec_swap_in_place(vec, 0, 1), 1);
    expect_unit(super::aurora_direct_vec_reverse_in_place(vec));
    assert_eq!(expect_int(super::aurora_direct_vec_index(vec, 0, 1, 1)), 5);
    assert_eq!(
        expect_option_some_int(super::aurora_direct_vec_index_option(vec, 1)),
        8
    );
    expect_unit(super::aurora_direct_vec_set_index_in_place(
        vec,
        1,
        int_value(42),
        1,
        1,
    ));
    expect_unit(super::aurora_direct_vec_extend_in_place(
        vec,
        int_vec(&[7, 9]),
    ));
    assert_eq!(
        expect_vec_ints(super::aurora_direct_clone_value(vec)),
        vec![5, 42, 7, 9]
    );
    expect_unit(super::aurora_direct_vec_clear_in_place(vec));
    assert_eq!(super::aurora_direct_vec_len(vec), 0);
    expect_option_none(super::aurora_direct_vec_pop_in_place(vec));
    expect_option_none(super::aurora_direct_vec_index_option(vec, 0));
}

#[test]
fn direct_runtime_vec_helpers_normalize_negative_indices_uniformly() {
    let vec = int_vec(&[10, 20, 30, 40]);

    assert_eq!(
        expect_int(super::aurora_direct_vec_index(vec, -1, 1, 1)),
        40
    );
    expect_unit(super::aurora_direct_vec_set_index_in_place(
        vec,
        -2,
        int_value(35),
        1,
        1,
    ));
    assert_eq!(
        expect_option_some_int(super::aurora_direct_vec_get(vec, -2)),
        35
    );
    expect_option_none(super::aurora_direct_vec_get(vec, -5));
    assert_eq!(
        expect_option_some_int(super::aurora_direct_vec_set_in_place(
            vec,
            -4,
            int_value(11),
        )),
        10
    );
    assert_eq!(
        expect_option_some_int(super::aurora_direct_vec_remove_in_place(vec, -2)),
        35
    );
    assert_eq!(super::aurora_direct_vec_swap_in_place(vec, -1, -3), 1);
    assert_eq!(
        super::aurora_direct_vec_insert_in_place(vec, -1, int_value(99)),
        1
    );
    assert_eq!(
        expect_vec_ints(super::aurora_direct_clone_value(vec)),
        vec![40, 20, 99, 11]
    );
}

#[test]
fn direct_runtime_map_and_set_helpers_cover_collection_surface() {
    let map = super::aurora_direct_map_empty();
    assert_eq!(super::aurora_direct_map_len(map), 0);
    assert_eq!(super::aurora_direct_map_is_empty(map), 1);
    expect_option_none(super::aurora_direct_map_set_in_place(
        map,
        string_value("name"),
        int_value(1),
    ));
    assert_eq!(
        expect_option_some_int(super::aurora_direct_map_set_in_place(
            map,
            string_value("name"),
            int_value(2),
        )),
        1
    );
    expect_option_none(super::aurora_direct_map_set_in_place(
        map,
        string_value("count"),
        int_value(3),
    ));
    assert_eq!(
        expect_option_some_int(super::aurora_direct_map_get(map, string_value("name"))),
        2
    );
    assert_eq!(
        super::aurora_direct_map_contains_key(map, string_value("count")),
        1
    );
    assert_eq!(
        expect_option_some_int(super::aurora_direct_map_remove_in_place(
            map,
            string_value("count"),
        )),
        3
    );
    assert_eq!(
        expect_vec_strings(super::aurora_direct_map_keys(map)),
        vec!["name".to_string()]
    );
    assert_eq!(
        expect_vec_ints(super::aurora_direct_map_values(map)),
        vec![2]
    );

    let entries = unsafe { take_value(super::aurora_direct_map_items(map)) };
    match entries {
        Value::Vec(values) => {
            assert_eq!(values.elements.len(), 1);
            let Value::Instance(entry) = &values.elements[0] else {
                panic!("expected map entry instance");
            };
            assert_eq!(entry.class_name, "MapEntry");
            assert_eq!(
                entry.fields.get("key"),
                Some(&Value::String("name".to_string()))
            );
            assert_eq!(
                entry.fields.get("value"),
                Some(&Value::Int(IntegerValue::from_signed(2)))
            );
        }
        other => panic!("expected vec of map entries, found {:?}", other),
    }
    assert!(matches!(
        unsafe { take_value(super::aurora_direct_map_entries(map)) },
        Value::Vec(values) if values.elements.len() == 1
    ));
    assert_eq!(
        expect_int(super::aurora_direct_map_index(
            map,
            string_value("name"),
            1,
            1,
        )),
        2
    );
    expect_unit(super::aurora_direct_map_set_index_in_place(
        map,
        string_value("status"),
        int_value(7),
        1,
        1,
    ));
    expect_unit(super::aurora_direct_map_extend_in_place(map, {
        let other = super::aurora_direct_map_empty();
        expect_option_none(super::aurora_direct_map_set_in_place(
            other,
            string_value("status"),
            int_value(9),
        ));
        other
    }));
    assert_eq!(
        expect_vec_ints(super::aurora_direct_map_values(map)),
        vec![2, 9]
    );
    expect_unit(super::aurora_direct_map_clear_in_place(map));
    assert_eq!(super::aurora_direct_map_len(map), 0);
    expect_option_none(super::aurora_direct_map_get(map, string_value("missing")));
    expect_option_none(super::aurora_direct_map_remove_in_place(
        map,
        string_value("missing"),
    ));
    assert_eq!(
        super::aurora_direct_map_contains_key(map, string_value("missing")),
        0
    );

    let set = super::aurora_direct_set_empty();
    assert_eq!(super::aurora_direct_set_len(set), 0);
    assert_eq!(super::aurora_direct_set_is_empty(set), 1);
    assert_eq!(
        super::aurora_direct_set_insert_in_place(set, int_value(3)),
        1
    );
    assert_eq!(
        super::aurora_direct_set_insert_in_place(set, int_value(3)),
        0
    );
    assert_eq!(super::aurora_direct_set_contains(set, int_value(3)), 1);
    assert_eq!(
        expect_option_some_int(super::aurora_direct_set_index_option(set, 0)),
        3
    );
    assert_eq!(
        super::aurora_direct_set_remove_in_place(set, int_value(3)),
        1
    );
    expect_option_none(super::aurora_direct_set_index_option(set, 0));
    assert_eq!(
        super::aurora_direct_set_remove_in_place(set, int_value(3)),
        0
    );
}

unsafe extern "C" fn test_native_thunk(args: *const i64, len: usize) -> *mut OpaqueValue {
    let args = std::slice::from_raw_parts(args, len);
    let total = args
        .iter()
        .map(|arg| match value_ref(*arg as *mut OpaqueValue) {
            Value::Int(value) => value.as_i128().expect("expected signed integer") as i64,
            other => panic!("expected int arg, found {:?}", other),
        })
        .sum();
    super::aurora_direct_box_i64(total)
}

#[test]
fn direct_runtime_scalar_and_concurrency_helpers_cover_remaining_surface() {
    assert_eq!(super::aurora_direct_unbox_i64(int_value(17)), 17);
    assert_eq!(super::aurora_direct_unbox_f64(float_value(2.5)), 2.5);
    assert_eq!(super::aurora_direct_unbox_bool(bool_value(true)), 1);
    assert_eq!(super::aurora_direct_value_as_condition(bool_value(true)), 1);
    assert_eq!(super::aurora_direct_value_as_condition(int_value(0)), 0);
    assert_eq!(super::aurora_direct_value_as_condition(int_value(2)), 1);
    assert_eq!(
        super::aurora_direct_value_as_condition(super::aurora_direct_box_unit()),
        0
    );
    assert_eq!(
        expect_int(super::aurora_direct_unary_value(0, int_value(-7))),
        7
    );
    assert!(!expect_bool_boxed(super::aurora_direct_unary_value(
        1,
        bool_value(true),
    )));
    assert_eq!(
        expect_bool_boxed(super::aurora_direct_unary_value_at(
            1,
            bool_value(false),
            1,
            1
        )),
        true
    );
    assert_eq!(
        expect_int(super::aurora_direct_binary_value(
            0,
            int_value(4),
            int_value(5)
        )),
        9
    );
    assert_eq!(
        expect_int(super::aurora_direct_binary_value(
            1,
            int_value(9),
            int_value(4),
        )),
        5
    );
    assert_eq!(
        expect_int(super::aurora_direct_binary_value(
            2,
            int_value(6),
            int_value(7),
        )),
        42
    );
    assert_eq!(
        expect_int(super::aurora_direct_binary_value(
            3,
            int_value(9),
            int_value(2),
        )),
        4
    );
    assert_eq!(
        expect_int(super::aurora_direct_binary_value(
            4,
            int_value(9),
            int_value(4),
        )),
        1
    );
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        5,
        int_value(4),
        int_value(4),
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        6,
        int_value(4),
        int_value(5),
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        7,
        int_value(4),
        int_value(5),
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        8,
        int_value(5),
        int_value(5),
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        9,
        int_value(6),
        int_value(5),
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        10,
        int_value(6),
        int_value(6),
    )));
    assert!(!expect_bool_boxed(super::aurora_direct_binary_value(
        11,
        bool_value(true),
        bool_value(false),
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        12,
        bool_value(true),
        bool_value(false),
    )));
    assert_eq!(
        expect_string(super::aurora_direct_binary_value_at(
            0,
            string_value("aurora"),
            string_value(" repo"),
            1,
            1,
        )),
        "aurora repo"
    );
    for (op, left, right, expected) in [
        (6, int_value(4), int_value(5), true),
        (7, int_value(4), int_value(5), true),
        (8, int_value(5), int_value(5), true),
        (9, int_value(6), int_value(5), true),
        (10, int_value(6), int_value(6), true),
        (11, bool_value(true), bool_value(false), false),
        (12, bool_value(false), bool_value(true), true),
    ] {
        assert_eq!(
            expect_bool_boxed(super::aurora_direct_binary_value_at(op, left, right, 2, 3)),
            expected
        );
    }
    assert_eq!(
        expect_float(super::aurora_direct_cast_value(
            int_value(9),
            b"float64".as_ptr(),
            "float64".len(),
        )),
        9.0
    );
    assert_eq!(
        expect_int(super::aurora_direct_cast_value_at(
            float_value(9.8),
            b"int32".as_ptr(),
            "int32".len(),
            1,
            1,
        )),
        9
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            string_value("aurora"),
            b"String".as_ptr(),
            "String".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(bool_value(false), b"bool".as_ptr(), "bool".len()),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            super::aurora_direct_vec_empty(),
            b"Vec".as_ptr(),
            "Vec".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            super::aurora_direct_set_empty(),
            b"Set".as_ptr(),
            "Set".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            super::aurora_direct_map_empty(),
            b"Map".as_ptr(),
            "Map".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            duration_value(5),
            b"Duration".as_ptr(),
            "Duration".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            boxed_value(Value::Range(RangeValue { start: 1, end: 4 })),
            b"Range".as_ptr(),
            "Range".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            boxed_value(Value::Channel(ChannelValue::new())),
            b"Queue".as_ptr(),
            "Queue".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            boxed_value(Value::Channel(ChannelValue::new())),
            b"Queue".as_ptr(),
            "Queue".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            boxed_value(Value::Task(TaskValue::from_handle(thread::spawn(|| Ok(
                Value::Unit
            ))))),
            b"Task".as_ptr(),
            "Task".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            boxed_value(Value::TaskGroup(TaskGroupValue::new(
                &CancellationContext::default()
            ))),
            b"TaskGroup".as_ptr(),
            "TaskGroup".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            boxed_value(Value::ModuleNamespace(ModuleNamespaceValue {
                path: "pkg.tools".to_string(),
            })),
            b"module pkg.tools".as_ptr(),
            "module pkg.tools".len(),
        ),
        1
    );

    let ready = super::aurora_direct_enum_variant(
        b"Status".as_ptr(),
        "Status".len(),
        b"Ready".as_ptr(),
        "Ready".len(),
        std::ptr::null_mut(),
        0,
    );
    assert_eq!(
        super::aurora_direct_variant_matches(
            ready,
            b"Status".as_ptr(),
            "Status".len(),
            b"Ready".as_ptr(),
            "Ready".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_variant_matches(
            int_value(1),
            b"Status".as_ptr(),
            "Status".len(),
            b"Ready".as_ptr(),
            "Ready".len(),
        ),
        0
    );
    let payloads = super::aurora_direct_arg_buffer_new(1);
    super::aurora_direct_arg_buffer_store(payloads, 0, string_value("payload") as i64);
    let boxed_payload = super::aurora_direct_enum_variant(
        b"Option".as_ptr(),
        "Option".len(),
        b"Some".as_ptr(),
        "Some".len(),
        payloads,
        1,
    );
    assert_eq!(
        expect_string(super::aurora_direct_variant_payload(boxed_payload, 0)),
        "payload"
    );

    let field_names = [b"value".as_ptr()];
    let field_name_lengths = ["value".len()];
    let field_values = [int_value(11)];
    let instance = super::aurora_direct_instance_new(
        b"Counter".as_ptr(),
        "Counter".len(),
        field_names.as_ptr(),
        field_name_lengths.as_ptr(),
        field_values.as_ptr(),
        1,
    );
    assert_eq!(
        expect_int(super::aurora_direct_instance_get_field(
            instance,
            b"value".as_ptr(),
            "value".len(),
        )),
        11
    );
    let empty_instance = super::aurora_direct_instance_empty(b"Counter".as_ptr(), "Counter".len());
    assert_eq!(
        expect_int(super::aurora_direct_instance_get_field(
            super::aurora_direct_instance_set_field(
                empty_instance,
                b"value".as_ptr(),
                "value".len(),
                int_value(13),
            ),
            b"value".as_ptr(),
            "value".len(),
        )),
        13
    );

    let buffer = super::aurora_direct_arg_buffer_new(2);
    super::aurora_direct_arg_buffer_store(buffer, 0, int_value(20) as i64);
    super::aurora_direct_arg_buffer_store(buffer, 1, int_value(22) as i64);
    let started_sum = run_lightweight_root_task(move || {
        let group = super::aurora_direct_task_group_new();
        let task = unsafe {
            take_value(super::aurora_direct_start_task_call(
                test_native_thunk as *const () as usize as i64,
                buffer,
                2,
                1,
                group,
            ))
        };
        let Value::Task(task) = task else {
            panic!("task start should return a task value");
        };
        Ok(unsafe {
            take_value(super::aurora_direct_task_join(boxed_value(Value::Task(
                task,
            ))))
        })
    })
    .expect("task start should run inside lightweight scheduler");
    assert_eq!(expect_task_result_ready_int(boxed_value(started_sum)), 42);

    let join_error = run_lightweight_root_task(move || {
        Ok(unsafe {
            take_value(super::aurora_direct_task_join(boxed_value(Value::Task(
                TaskValue::from_handle(thread::spawn(|| Err(Diagnostic::new("boom")))),
            ))))
        })
    })
    .expect("task join error should run inside lightweight scheduler");
    assert_eq!(
        expect_task_result_error_message(boxed_value(join_error)),
        "boom"
    );

    let channel = super::aurora_direct_channel_new(std::ptr::null_mut());
    let send_ok = unsafe { take_value(super::aurora_direct_channel_send(channel, int_value(9))) };
    match send_ok {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Ok" => {}
        other => panic!("expected Result.Ok(Unit), found {:?}", other),
    }
    assert_eq!(
        expect_queue_receive_item_int(super::aurora_direct_channel_recv(channel)),
        9
    );
    expect_unit(super::aurora_direct_channel_close(channel));
    match unsafe { take_value(super::aurora_direct_channel_send(channel, int_value(7))) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Err" => {}
        other => panic!(
            "expected Result.Err(SendError.Closed(...)), found {:?}",
            other
        ),
    }
    let closed_try_send =
        expect_result_err_payload(super::aurora_direct_channel_try_send(channel, int_value(8)));
    assert_eq!(
        expect_variant_value(closed_try_send, "SendError", "Closed").len(),
        1
    );
    let closed_timeout_send = expect_result_err_payload(
        super::aurora_direct_channel_send_timeout_value(channel, int_value(10), duration_value(0)),
    );
    assert_eq!(
        expect_variant_value(closed_timeout_send, "SendError", "Closed").len(),
        1
    );
    expect_queue_receive_closed(super::aurora_direct_channel_recv(channel));
    expect_queue_receive_closed(super::aurora_direct_channel_recv_timeout_value(
        channel,
        duration_value(0),
    ));

    let timeout_channel = super::aurora_direct_channel_new(std::ptr::null_mut());
    expect_result_ok_unit(super::aurora_direct_channel_try_send(
        timeout_channel,
        int_value(15),
    ));
    assert_eq!(
        expect_queue_receive_item_int(super::aurora_direct_channel_recv_timeout_value(
            timeout_channel,
            duration_value(0),
        )),
        15
    );

    let bounded_channel = super::aurora_direct_channel_new(int_value(1));
    expect_result_ok_unit(super::aurora_direct_channel_send_timeout_value(
        bounded_channel,
        int_value(11),
        duration_value(0),
    ));
    let full_send = expect_result_err_payload(super::aurora_direct_channel_try_send(
        bounded_channel,
        int_value(12),
    ));
    assert!(expect_variant_value(full_send, "SendError", "Full").len() == 1);
    assert_eq!(
        expect_queue_receive_item_int(super::aurora_direct_channel_recv(bounded_channel)),
        11
    );
    expect_result_ok_unit(super::aurora_direct_channel_try_send(
        bounded_channel,
        int_value(13),
    ));
    assert_eq!(
        expect_queue_receive_item_int(super::aurora_direct_channel_recv(bounded_channel)),
        13
    );
    expect_unit(super::aurora_direct_close_value(bounded_channel, 0));
    expect_unit(super::aurora_direct_close_value(
        boxed_value(Value::Unit),
        0,
    ));

    let group = super::aurora_direct_task_group_new();
    expect_unit(super::aurora_direct_task_group_cancel(group));
    assert_eq!(super::aurora_direct_cancelled(), 0);
    expect_unit(super::aurora_direct_task_group_close(group, 0));
    expect_unit(super::aurora_direct_close_value(
        super::aurora_direct_task_group_new(),
        1,
    ));
    let group = boxed_value(Value::TaskGroup(TaskGroupValue::new(
        &CancellationContext::default(),
    )));
    if let Value::TaskGroup(group_value) = unsafe { value_ref(group) } {
        group_value.register_task(TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit))));
    }
    expect_unit(super::aurora_direct_task_group_close(group, 1));
    super::aurora_direct_sleep_ms(0);
    expect_unit(super::aurora_direct_sleep_value(duration_value(0)));
}

#[test]
fn native_runtime_direct_queue_and_task_fallback_wrappers_cover_option_default_paths() {
    let channel = super::aurora_direct_channel_new(std::ptr::null_mut());
    expect_option_none(super::aurora_direct_channel_recv_or_none(channel));
    assert_eq!(
        expect_int(super::aurora_direct_channel_recv_or_value(
            channel,
            int_value(5)
        )),
        5
    );
    expect_variant_ptr(
        super::aurora_direct_channel_recv_timeout_value(channel, duration_value(0)),
        "QueueReceive",
        "TimedOut",
    );
    expect_option_none(super::aurora_direct_channel_recv_or_none_timeout_value(
        channel,
        duration_value(0),
    ));
    assert_eq!(
        expect_int(super::aurora_direct_channel_recv_or_value_timeout_value(
            channel,
            int_value(6),
            duration_value(0),
        )),
        6
    );

    expect_result_ok_unit(super::aurora_direct_channel_try_send(
        channel,
        int_value(21),
    ));
    assert_eq!(
        expect_option_some_int(super::aurora_direct_channel_recv_or_none(channel)),
        21
    );
    expect_result_ok_unit(super::aurora_direct_channel_try_send(
        channel,
        int_value(22),
    ));
    assert_eq!(
        expect_int(super::aurora_direct_channel_recv_or_value(
            channel,
            int_value(7)
        )),
        22
    );
    expect_result_ok_unit(super::aurora_direct_channel_try_send(
        channel,
        int_value(23),
    ));
    assert_eq!(
        expect_option_some_int(super::aurora_direct_channel_recv_or_none_timeout_value(
            channel,
            duration_value(0),
        )),
        23
    );
    expect_result_ok_unit(super::aurora_direct_channel_try_send(
        channel,
        int_value(24),
    ));
    assert_eq!(
        expect_int(super::aurora_direct_channel_recv_or_value_timeout_value(
            channel,
            int_value(8),
            duration_value(0),
        )),
        24
    );
    expect_unit(super::aurora_direct_channel_close(channel));
    expect_option_none(super::aurora_direct_channel_recv_or_none(channel));
    assert_eq!(
        expect_int(super::aurora_direct_channel_recv_or_value(
            channel,
            int_value(9)
        )),
        9
    );

    let bounded = super::aurora_direct_channel_new(int_value(1));
    expect_result_ok_unit(super::aurora_direct_channel_try_send(
        bounded,
        int_value(31),
    ));
    let timed_out = expect_result_err_payload(super::aurora_direct_channel_send_timeout_value(
        bounded,
        int_value(32),
        duration_value(0),
    ));
    assert_eq!(
        expect_variant_value(timed_out, "SendError", "TimedOut").len(),
        1
    );
    expect_unit(super::aurora_direct_channel_close(bounded));

    let slow_task = boxed_value(Value::Task(TaskValue::from_handle(thread::spawn(|| {
        thread::sleep(StdDuration::from_millis(200));
        Ok(Value::Int(IntegerValue::from_signed(77)))
    }))));
    expect_option_none(super::aurora_direct_task_join_or_none(slow_task));
    assert_eq!(
        expect_int(super::aurora_direct_task_join_or_value(
            slow_task,
            int_value(50)
        )),
        50
    );
    expect_option_none(super::aurora_direct_task_join_or_none_timeout_value(
        slow_task,
        duration_value(0),
    ));
    assert_eq!(
        expect_int(super::aurora_direct_task_join_or_value_timeout_value(
            slow_task,
            int_value(51),
            duration_value(0),
        )),
        51
    );
    assert_eq!(
        expect_task_result_ready_int(super::aurora_direct_task_join(slow_task)),
        77
    );
    assert_eq!(
        expect_option_some_int(super::aurora_direct_task_join_or_none(slow_task)),
        77
    );
    assert_eq!(
        expect_option_some_int(super::aurora_direct_task_join_or_none_timeout_value(
            slow_task,
            duration_value(0),
        )),
        77
    );
    assert_eq!(
        expect_int(super::aurora_direct_task_join_or_value(
            slow_task,
            int_value(52)
        )),
        77
    );
    assert_eq!(
        expect_int(super::aurora_direct_task_join_or_value_timeout_value(
            slow_task,
            int_value(53),
            duration_value(0),
        )),
        77
    );

    let error_task = boxed_value(Value::Task(TaskValue::from_handle(thread::spawn(|| {
        Err(Diagnostic::new("task failed"))
    }))));
    assert_eq!(
        expect_task_result_error_message(super::aurora_direct_task_join(error_task)),
        "task failed"
    );
    expect_option_none(super::aurora_direct_task_join_or_none(error_task));
    expect_option_none(super::aurora_direct_task_join_or_none_timeout_value(
        error_task,
        duration_value(0),
    ));
    assert_eq!(
        expect_int(super::aurora_direct_task_join_or_value(
            error_task,
            int_value(54)
        )),
        54
    );
    assert_eq!(
        expect_int(super::aurora_direct_task_join_or_value_timeout_value(
            error_task,
            int_value(55),
            duration_value(0),
        )),
        55
    );
}

#[test]
fn native_runtime_direct_concurrency_wrappers_cover_cancelled_paths() {
    let group = TaskGroupValue::new(&CancellationContext::default());
    let cancellation = group.child_cancellation();
    group.cancel();

    let task = with_cancellation_scope(cancellation, || {
        assert_eq!(super::aurora_direct_cancelled(), 1);

        let bounded = super::aurora_direct_channel_new(int_value(1));
        expect_result_ok_unit(super::aurora_direct_channel_try_send(bounded, int_value(1)));

        let cancelled =
            expect_result_err_payload(super::aurora_direct_channel_send(bounded, int_value(2)));
        assert_eq!(
            expect_variant_value(cancelled, "SendError", "Cancelled").len(),
            1
        );
        let cancelled = expect_result_err_payload(super::aurora_direct_channel_send_timeout_value(
            bounded,
            int_value(3),
            duration_value(1000),
        ));
        assert_eq!(
            expect_variant_value(cancelled, "SendError", "Cancelled").len(),
            1
        );

        let empty = super::aurora_direct_channel_new(std::ptr::null_mut());
        expect_variant_ptr(
            super::aurora_direct_channel_recv(empty),
            "QueueReceive",
            "Cancelled",
        );
        expect_variant_ptr(
            super::aurora_direct_channel_recv_timeout_value(empty, duration_value(1000)),
            "QueueReceive",
            "Cancelled",
        );
        expect_option_none(super::aurora_direct_channel_recv_or_none(empty));
        expect_option_none(super::aurora_direct_channel_recv_or_none_timeout_value(
            empty,
            duration_value(1000),
        ));
        assert_eq!(
            expect_int(super::aurora_direct_channel_recv_or_value(
                empty,
                int_value(4)
            )),
            4
        );
        assert_eq!(
            expect_int(super::aurora_direct_channel_recv_or_value_timeout_value(
                empty,
                int_value(5),
                duration_value(1000),
            )),
            5
        );

        let task_value = TaskValue::from_handle(thread::spawn(|| {
            thread::sleep(StdDuration::from_millis(50));
            Ok(Value::Int(IntegerValue::from_signed(6)))
        }));
        let task = boxed_value(Value::Task(task_value.clone()));
        expect_variant_ptr(
            super::aurora_direct_task_join(task),
            "TaskResult",
            "Cancelled",
        );
        expect_variant_ptr(
            super::aurora_direct_task_join_timeout_value(task, duration_value(1000)),
            "TaskResult",
            "Cancelled",
        );
        expect_option_none(super::aurora_direct_task_join_or_none(task));
        expect_option_none(super::aurora_direct_task_join_or_none_timeout_value(
            task,
            duration_value(1000),
        ));
        assert_eq!(
            expect_int(super::aurora_direct_task_join_or_value(task, int_value(7))),
            7
        );
        assert_eq!(
            expect_int(super::aurora_direct_task_join_or_value_timeout_value(
                task,
                int_value(8),
                duration_value(1000),
            )),
            8
        );

        expect_variant_ptr(
            super::aurora_direct_wait_any(task_vec(&[task_value.clone()])),
            "WaitAny",
            "Cancelled",
        );
        expect_variant_ptr(
            super::aurora_direct_wait_all(task_vec(&[task_value.clone()])),
            "WaitAll",
            "Cancelled",
        );

        task
    });
    assert_eq!(
        expect_task_result_ready_int(super::aurora_direct_task_join(task)),
        6
    );
}

#[test]
fn division_by_zero_helper_exits_with_error() {
    if std::env::var("AURORA_DIRECT_RUNTIME_HELPER").as_deref() == Ok("divzero") {
        super::aurora_direct_runtime_init(
            b"/virtual/test.au".as_ptr(),
            b"/virtual/test.au".len(),
            b"def main() -> int32:\n    print(1 // 0)\n".as_ptr(),
            b"def main() -> int32:\n    print(1 // 0)\n".len(),
        );
        super::aurora_direct_fail_division_by_zero(2, 11);
    }

    let output = Command::new(std::env::current_exe().expect("test binary should exist"))
        .arg("--exact")
        .arg("native_runtime::tests::division_by_zero_helper_exits_with_error")
        .arg("--nocapture")
        .env("AURORA_DIRECT_RUNTIME_HELPER", "divzero")
        .output()
        .expect("child test process should run");

    assert!(
        !output.status.success(),
        "division helper should exit with failure"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("division by zero"),
        "division helper stderr should mention division by zero"
    );
}

#[test]
fn int32_overflow_helper_exits_with_error() {
    if std::env::var("AURORA_DIRECT_RUNTIME_HELPER").as_deref() == Ok("overflow") {
        super::aurora_direct_runtime_init(
            b"/virtual/test.au".as_ptr(),
            b"/virtual/test.au".len(),
            b"def main() -> int32:\n    value: int32 = 999\n".as_ptr(),
            b"def main() -> int32:\n    value: int32 = 999\n".len(),
        );
        super::aurora_direct_fail_int32_overflow(999, 2, 20);
    }

    let output = Command::new(std::env::current_exe().expect("test binary should exist"))
        .arg("--exact")
        .arg("native_runtime::tests::int32_overflow_helper_exits_with_error")
        .arg("--nocapture")
        .env("AURORA_DIRECT_RUNTIME_HELPER", "overflow")
        .output()
        .expect("child test process should run");

    assert!(
        !output.status.success(),
        "overflow helper should exit with failure"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("integer value `999` does not fit in `int32`"),
        "overflow helper stderr should mention the failing int32 value"
    );
}

#[test]
fn wide_integer_overflow_and_cast_helpers_report_precise_diagnostics() {
    const HELPER_ENV: &str = "AURORA_DIRECT_RUNTIME_WIDE_INTEGER_ERROR_HELPER";
    if let Ok(helper) = std::env::var(HELPER_ENV) {
        match helper.as_str() {
            "signed-overflow-with-span" => {
                let source = b"def main() -> int32:\n    value = 9223372036854775807 + 1\n";
                super::aurora_direct_runtime_init(
                    b"/virtual/wide.au".as_ptr(),
                    b"/virtual/wide.au".len(),
                    source.as_ptr(),
                    source.len(),
                );
                super::aurora_direct_fail_integer_overflow(0, 0, i64::MAX as u64, 1, 2, 13);
            }
            "unsigned-underflow-without-span" => {
                super::aurora_direct_fail_integer_overflow(1, 1, 0, 1, 0, 0);
            }
            "integer-cast-with-span" => {
                let source = b"def main() -> int32:\n    value = high as int64\n";
                super::aurora_direct_runtime_init(
                    b"/virtual/cast.au".as_ptr(),
                    b"/virtual/cast.au".len(),
                    source.as_ptr(),
                    source.len(),
                );
                super::aurora_direct_cast_integer_to_integer(u64::MAX, 1, 1, 2, 13);
            }
            "float-cast-without-span" => {
                super::aurora_direct_cast_float_to_integer(4_294_967_296.75, 0, 0, 0);
            }
            other => panic!("unknown wide-integer error helper `{other}`"),
        }
    }

    for (helper, expected_message, expected_location) in [
        (
            "signed-overflow-with-span",
            "integer value `9223372036854775808` does not fit in `int64`",
            Some(" --> /virtual/wide.au:2:13"),
        ),
        (
            "unsigned-underflow-without-span",
            "integer value `-1` does not fit in `uint64`",
            None,
        ),
        (
            "integer-cast-with-span",
            "integer value `18446744073709551615` does not fit in `int64`",
            Some(" --> /virtual/cast.au:2:13"),
        ),
        (
            "float-cast-without-span",
            "integer value `4294967296` does not fit in `int32`",
            None,
        ),
    ] {
        let output = Command::new(std::env::current_exe().expect("test binary should exist"))
            .arg("--exact")
            .arg(
                "native_runtime::tests::wide_integer_overflow_and_cast_helpers_report_precise_diagnostics",
            )
            .arg("--nocapture")
            .env(HELPER_ENV, helper)
            .output()
            .expect("child test process should run");

        assert!(
            !output.status.success(),
            "{helper} should exit with a diagnostic"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.starts_with(&format!("error: {expected_message}\n")),
            "unexpected diagnostic for {helper}:\n{stderr}"
        );
        match expected_location {
            Some(location) => assert!(
                stderr.contains(location),
                "diagnostic for {helper} should include `{location}`:\n{stderr}"
            ),
            None => assert_eq!(
                stderr,
                format!("error: {expected_message}\n"),
                "spanless diagnostic for {helper} should not invent a source location"
            ),
        }
    }
}

#[test]
fn direct_root_entrypoint_helper_exits_for_invalid_thunks_and_return_types() {
    if let Ok(helper) = std::env::var("AURORA_DIRECT_RUNTIME_HELPER") {
        match helper.as_str() {
            "direct-root-null" => unsafe {
                super::aurora_direct_run_root(0);
            },
            "direct-root-string" => {
                unsafe extern "C-unwind" fn returns_string(
                    _args: *const i64,
                    _arg_count: usize,
                ) -> *mut OpaqueValue {
                    super::aurora_direct_string_literal(b"not-int32".as_ptr(), b"not-int32".len())
                }
                unsafe {
                    super::aurora_direct_run_root(returns_string as *const () as usize as i64);
                }
            }
            "direct-call-depth" => unsafe {
                for _ in 0..=super::DIRECT_MAX_CALL_DEPTH {
                    super::aurora_direct_enter_call(2, 3, b"recurse".as_ptr(), b"recurse".len());
                }
            },
            _ => {}
        }
    }

    for (helper, expected) in [
        ("direct-root-null", "invalid direct root thunk pointer"),
        (
            "direct-root-string",
            "direct main entry must return `int32` or `None`, found `String`",
        ),
        (
            "direct-call-depth",
            "maximum call depth of 256 exceeded while calling `recurse`",
        ),
    ] {
        let output = Command::new(std::env::current_exe().expect("test binary should exist"))
            .arg("--exact")
            .arg(
                "native_runtime::tests::direct_root_entrypoint_helper_exits_for_invalid_thunks_and_return_types",
            )
            .arg("--nocapture")
            .env("AURORA_DIRECT_RUNTIME_HELPER", helper)
            .output()
            .expect("child test process should run");

        assert!(
            !output.status.success(),
            "direct root helper should exit with failure for {helper}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "direct root helper stderr should mention {expected}"
        );
    }
}

#[test]
fn native_runtime_entrypoint_guards_invalid_inputs() {
    assert_eq!(
        unsafe {
            crate::mir_runtime::aurora_native_run(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            )
        },
        1
    );

    let mir_json = b"{}";
    let invalid_path = [0xff_u8];
    let source = b"def main() -> int32:\n    return 0\n";
    assert_eq!(
        unsafe {
            crate::mir_runtime::aurora_native_run(
                mir_json.as_ptr(),
                mir_json.len(),
                invalid_path.as_ptr(),
                invalid_path.len(),
                source.as_ptr(),
                source.len(),
            )
        },
        1
    );

    let source_path = b"/tmp/test.au";
    let invalid_source = [0xff_u8];
    assert_eq!(
        unsafe {
            crate::mir_runtime::aurora_native_run(
                mir_json.as_ptr(),
                mir_json.len(),
                source_path.as_ptr(),
                source_path.len(),
                invalid_source.as_ptr(),
                invalid_source.len(),
            )
        },
        1
    );

    assert_eq!(
        render_runtime_diagnostic(crate::diag::Diagnostic::new("oops")),
        "error: oops"
    );
}

#[test]
fn native_runtime_private_value_decoders_cover_success_paths() {
    assert_eq!(
        super::expect_string_value(&Value::String("aurora".to_string()), "text"),
        "aurora"
    );
    assert_eq!(
        super::expect_bytes_value(
            &Value::Vec(VecValue {
                element_type: crate::sema::Type::named("Unknown"),
                elements: vec![
                    Value::Int(IntegerValue::from_signed(65)),
                    Value::Int(IntegerValue::from_signed(66)),
                ],
            }),
            "bytes",
        ),
        vec![65, 66]
    );
    assert!(super::expect_bool_value(&Value::Bool(true), "flag"));
    assert_eq!(
        super::expect_i32_value(&Value::Int(IntegerValue::from_signed(123)), "count"),
        123
    );
    assert_eq!(
        super::expect_headers_map(
            &Value::Map(MapValue {
                key_type: crate::sema::Type::named("Unknown"),
                value_type: crate::sema::Type::named("Unknown"),
                entries: vec![(
                    Value::String("content-type".to_string()),
                    Value::String("text/plain".to_string()),
                )],
            }),
            "headers",
        ),
        vec![("content-type".to_string(), "text/plain".to_string())]
    );
    assert_eq!(
        super::optional_timeout_from_ptr(std::ptr::null_mut(), "timeout"),
        None
    );
    let timeout = duration_value(12);
    assert_eq!(
        super::optional_timeout_from_ptr(timeout, "timeout"),
        Some(StdDuration::from_millis(12))
    );
    unsafe {
        release_value(timeout);
    }
    let unit_timeout = boxed_value(Value::Unit);
    assert_eq!(
        super::process_optional_timeout_from_ptr(unit_timeout, "timeout"),
        None
    );
    unsafe {
        release_value(unit_timeout);
    }
    let negative_timeout = duration_value(-1);
    assert_eq!(
        super::process_optional_timeout_from_ptr(negative_timeout, "timeout"),
        None
    );
    unsafe {
        release_value(negative_timeout);
    }
    let process_timeout = duration_value(34);
    assert_eq!(
        super::process_optional_timeout_from_ptr(process_timeout, "timeout"),
        Some(StdDuration::from_millis(34))
    );
    unsafe {
        release_value(process_timeout);
    }
    let duration = duration_value(56);
    assert_eq!(
        super::duration_from_ptr(duration, "duration"),
        StdDuration::from_millis(56)
    );
    unsafe {
        release_value(duration);
    }
    let unlimited_restarts = int_value(-1);
    assert_eq!(
        super::supervisor_max_restarts_from_ptr(unlimited_restarts, "max_restarts"),
        None
    );
    unsafe {
        release_value(unlimited_restarts);
    }
    let limited_restarts = int_value(3);
    assert_eq!(
        super::supervisor_max_restarts_from_ptr(limited_restarts, "max_restarts"),
        Some(3)
    );
    unsafe {
        release_value(limited_restarts);
    }
    assert_eq!(
        super::expect_command_vec(
            &Value::Vec(VecValue {
                element_type: crate::sema::Type::named("Unknown"),
                elements: vec![
                    Value::String("/bin/echo".to_string()),
                    Value::String("ok".to_string()),
                ],
            }),
            "command",
        ),
        vec!["/bin/echo".to_string(), "ok".to_string()]
    );
    assert_eq!(
        super::expect_optional_string_value(&Value::Unit, "cwd"),
        None
    );
    assert_eq!(
        super::expect_optional_string_value(
            &Value::EnumVariant(EnumVariantValue {
                enum_name: "Option".to_string(),
                variant_name: "None".to_string(),
                payloads: Vec::new(),
            }),
            "cwd",
        ),
        None
    );
    assert_eq!(
        super::expect_optional_string_value(
            &Value::EnumVariant(EnumVariantValue {
                enum_name: "Option".to_string(),
                variant_name: "Some".to_string(),
                payloads: vec![Value::String("/tmp".to_string())],
            }),
            "cwd",
        ),
        Some("/tmp".to_string())
    );
}

#[test]
fn direct_runtime_helper_errors_surface_expected_diagnostics() {
    if let Ok(case) = std::env::var("AURORA_DIRECT_RUNTIME_CASE") {
        match case.as_str() {
            "bytes-value-type" => {
                super::expect_bytes_value(&Value::String("bytes".to_string()), "bytes");
            }
            "bytes-element-range" => {
                super::expect_bytes_value(
                    &Value::Vec(VecValue {
                        element_type: crate::sema::Type::named("uint8"),
                        elements: vec![Value::Int(IntegerValue::from_signed(300))],
                    }),
                    "bytes",
                );
            }
            "bool-value-type" => {
                super::expect_bool_value(&Value::String("flag".to_string()), "flag");
            }
            "i32-overflow" => {
                super::expect_i32_value(
                    &Value::Int(IntegerValue::from_signed(i128::from(i32::MAX) + 1)),
                    "count",
                );
            }
            "i32-value-type" => {
                super::expect_i32_value(&Value::String("count".to_string()), "count");
            }
            "headers-map-type" => {
                super::expect_headers_map(&Value::String("headers".to_string()), "headers");
            }
            "headers-key-type" => {
                super::expect_headers_map(
                    &Value::Map(MapValue {
                        key_type: crate::sema::Type::named("Unknown"),
                        value_type: crate::sema::Type::named("Unknown"),
                        entries: vec![(
                            Value::Int(IntegerValue::from_signed(1)),
                            Value::String("value".to_string()),
                        )],
                    }),
                    "headers",
                );
            }
            "optional-timeout-type" => {
                super::optional_timeout_from_ptr(string_value("slow"), "timeout");
            }
            "optional-timeout-negative" => {
                super::optional_timeout_from_ptr(duration_value(-1), "timeout");
            }
            "process-timeout-type" => {
                super::process_optional_timeout_from_ptr(string_value("slow"), "timeout");
            }
            "duration-type" => {
                super::duration_from_ptr(string_value("slow"), "duration");
            }
            "duration-negative" => {
                super::duration_from_ptr(duration_value(-1), "duration");
            }
            "supervisor-max-too-low" => {
                super::supervisor_max_restarts_from_ptr(int_value(-2), "max_restarts");
            }
            "command-vec-type" => {
                super::expect_command_vec(&Value::String("command".to_string()), "command");
            }
            "command-element-type" => {
                super::expect_command_vec(
                    &Value::Vec(VecValue {
                        element_type: crate::sema::Type::named("String"),
                        elements: vec![Value::Int(IntegerValue::from_signed(1))],
                    }),
                    "command",
                );
            }
            "optional-string-malformed" => {
                super::expect_optional_string_value(
                    &Value::EnumVariant(EnumVariantValue {
                        enum_name: "Option".to_string(),
                        variant_name: "Some".to_string(),
                        payloads: Vec::new(),
                    }),
                    "cwd",
                );
            }
            "optional-string-payload-type" => {
                super::expect_optional_string_value(
                    &Value::EnumVariant(EnumVariantValue {
                        enum_name: "Option".to_string(),
                        variant_name: "Some".to_string(),
                        payloads: vec![Value::Bool(true)],
                    }),
                    "cwd",
                );
            }
            "optional-string-type" => {
                super::expect_optional_string_value(
                    &Value::Int(IntegerValue::from_signed(1)),
                    "cwd",
                );
            }
            "process-start-command-type" => {
                super::aurora_direct_process_start(
                    bool_value(true),
                    boxed_value(Value::Unit),
                    super::aurora_direct_map_empty(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    bool_value(false),
                );
            }
            "process-start-cwd-type" => {
                super::aurora_direct_process_start(
                    string_vec(&["/bin/echo", "ok"]),
                    bool_value(true),
                    super::aurora_direct_map_empty(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    bool_value(false),
                );
            }
            "process-start-env-type" => {
                super::aurora_direct_process_start(
                    string_vec(&["/bin/echo", "ok"]),
                    boxed_value(Value::Unit),
                    bool_value(true),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    bool_value(false),
                );
            }
            "process-start-group-type" => {
                super::aurora_direct_process_start(
                    string_vec(&["/bin/echo", "ok"]),
                    boxed_value(Value::Unit),
                    super::aurora_direct_map_empty(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    string_value("group"),
                );
            }
            "process-run-command-type" => {
                super::aurora_direct_process_run(
                    bool_value(true),
                    boxed_value(Value::Unit),
                    super::aurora_direct_map_empty(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    duration_value(1),
                    bool_value(false),
                );
            }
            "process-run-timeout-type" => {
                super::aurora_direct_process_run(
                    string_vec(&["/bin/echo", "ok"]),
                    boxed_value(Value::Unit),
                    super::aurora_direct_map_empty(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    string_value("slow"),
                    bool_value(false),
                );
            }
            "process-run-group-type" => {
                super::aurora_direct_process_run(
                    string_vec(&["/bin/echo", "ok"]),
                    boxed_value(Value::Unit),
                    super::aurora_direct_map_empty(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    super::aurora_direct_process_null(),
                    duration_value(1),
                    string_value("group"),
                );
            }
            "arg-buffer-negative-size" => {
                super::aurora_direct_arg_buffer_new(-1);
            }
            "arg-buffer-negative-index" => {
                let buffer = super::aurora_direct_arg_buffer_new(1);
                super::aurora_direct_arg_buffer_store(buffer, -1, int_value(1) as i64);
            }
            "cleanup-negative-arg-count" => {
                super::aurora_direct_register_cleanup(1, std::ptr::null_mut(), -1);
            }
            "cleanup-null-thunk" => {
                super::aurora_direct_register_cleanup(0, std::ptr::null_mut(), 0);
            }
            "cleanup-refresh-negative-arg-count" => {
                super::aurora_direct_refresh_cleanup(1, 0, 1, std::ptr::null_mut(), -1);
            }
            "cleanup-refresh-null-thunk" => {
                super::aurora_direct_refresh_cleanup(1, 0, 0, std::ptr::null_mut(), 0);
            }
            "queue-capacity-zero" => {
                super::aurora_direct_channel_new(int_value(0));
            }
            "queue-send-type" => {
                super::aurora_direct_channel_send(bool_value(true), int_value(1));
            }
            "queue-send-timeout-negative" => {
                super::aurora_direct_channel_send_timeout_value(
                    super::aurora_direct_channel_new(std::ptr::null_mut()),
                    int_value(1),
                    duration_value(-1),
                );
            }
            "queue-try-send-type" => {
                super::aurora_direct_channel_try_send(bool_value(true), int_value(1));
            }
            "queue-recv-type" => {
                super::aurora_direct_channel_recv(bool_value(true));
            }
            "queue-recv-timeout-negative" => {
                super::aurora_direct_channel_recv_timeout_value(
                    super::aurora_direct_channel_new(std::ptr::null_mut()),
                    duration_value(-1),
                );
            }
            "queue-recv-in-task-group-queue-type" => {
                super::aurora_direct_channel_recv_in_task_group(
                    bool_value(true),
                    super::aurora_direct_task_group_new(),
                );
            }
            "queue-recv-in-task-group-group-type" => {
                super::aurora_direct_channel_recv_in_task_group(
                    super::aurora_direct_channel_new(std::ptr::null_mut()),
                    bool_value(true),
                );
            }
            "queue-recv-registered-producers-type" => {
                super::aurora_direct_channel_recv_with_registered_producers(bool_value(true));
            }
            "wait-any-timeout-negative" => {
                super::aurora_direct_wait_any_timeout_value(
                    super::aurora_direct_vec_empty(),
                    duration_value(-1),
                );
            }
            "wait-all-timeout-negative" => {
                super::aurora_direct_wait_all_timeout_value(
                    super::aurora_direct_vec_empty(),
                    duration_value(-1),
                );
            }
            "task-group-cancel-type" => {
                super::aurora_direct_task_group_cancel(bool_value(true));
            }
            "task-group-close-type" => {
                super::aurora_direct_task_group_close(bool_value(true), 0);
            }
            "io-write-type" => {
                super::aurora_direct_io_write(bool_value(true));
            }
            "fs-exists-type" => {
                super::aurora_direct_fs_exists(bool_value(true));
            }
            "fs-read-to-string-type" => {
                super::aurora_direct_fs_read_to_string(bool_value(true));
            }
            "fs-read-bytes-type" => {
                super::aurora_direct_fs_read_bytes(bool_value(true));
            }
            "fs-write-string-path-type" => {
                super::aurora_direct_fs_write_string(bool_value(true), string_value("text"));
            }
            "fs-write-string-text-type" => {
                super::aurora_direct_fs_write_string(string_value("/tmp/unused"), bool_value(true));
            }
            "fs-write-bytes-path-type" => {
                super::aurora_direct_fs_write_bytes(bool_value(true), int_vec(&[1, 2]));
            }
            "fs-append-string-path-type" => {
                super::aurora_direct_fs_append_string(bool_value(true), string_value("text"));
            }
            "fs-append-string-text-type" => {
                super::aurora_direct_fs_append_string(
                    string_value("/tmp/unused"),
                    bool_value(true),
                );
            }
            "fs-append-bytes-path-type" => {
                super::aurora_direct_fs_append_bytes(bool_value(true), int_vec(&[1, 2]));
            }
            "fs-append-bytes-bytes-type" => {
                super::aurora_direct_fs_append_bytes(string_value("/tmp/unused"), bool_value(true));
            }
            "fs-create-dir-type" => {
                super::aurora_direct_fs_create_dir(bool_value(true));
            }
            "fs-read-dir-type" => {
                super::aurora_direct_fs_read_dir(bool_value(true));
            }
            "fs-remove-file-type" => {
                super::aurora_direct_fs_remove_file(bool_value(true));
            }
            "fs-open-type" => {
                super::aurora_direct_fs_open(bool_value(true));
            }
            "fs-create-type" => {
                super::aurora_direct_fs_create(bool_value(true));
            }
            "fs-append-type" => {
                super::aurora_direct_fs_append(bool_value(true));
            }
            "file-read-all-type" => {
                super::aurora_direct_file_read_all(bool_value(true));
            }
            "file-read-bytes-type" => {
                super::aurora_direct_file_read_bytes(bool_value(true));
            }
            "file-write-all-text-type" => {
                super::aurora_direct_file_write_all(bool_value(true), bool_value(true));
            }
            "file-write-all-file-type" => {
                super::aurora_direct_file_write_all(bool_value(true), string_value("text"));
            }
            "file-write-bytes-file-type" => {
                super::aurora_direct_file_write_bytes(bool_value(true), int_vec(&[1, 2]));
            }
            "file-flush-type" => {
                super::aurora_direct_file_flush(bool_value(true));
            }
            "file-close-type" => {
                super::aurora_direct_file_close(bool_value(true));
            }
            "contains-arg" => {
                super::aurora_direct_string_contains(string_value("aurora"), bool_value(true));
            }
            "contains-receiver" => {
                super::aurora_direct_string_contains(bool_value(true), string_value("a"));
            }
            "starts-with-arg" => {
                super::aurora_direct_string_starts_with(string_value("aurora"), bool_value(true));
            }
            "starts-with-receiver" => {
                super::aurora_direct_string_starts_with(bool_value(true), string_value("a"));
            }
            "ends-with-arg" => {
                super::aurora_direct_string_ends_with(string_value("aurora"), bool_value(true));
            }
            "ends-with-receiver" => {
                super::aurora_direct_string_ends_with(bool_value(true), string_value("a"));
            }
            "split-arg" => {
                super::aurora_direct_string_split(string_value("a,b"), bool_value(true));
            }
            "split-receiver" => {
                super::aurora_direct_string_split(bool_value(true), string_value(","));
            }
            "replace-from" => {
                super::aurora_direct_string_replace(
                    string_value("aurora"),
                    bool_value(true),
                    string_value("x"),
                );
            }
            "replace-to" => {
                super::aurora_direct_string_replace(
                    string_value("aurora"),
                    string_value("a"),
                    bool_value(true),
                );
            }
            "replace-receiver" => {
                super::aurora_direct_string_replace(
                    bool_value(true),
                    string_value("a"),
                    string_value("x"),
                );
            }
            "string-len-type" => {
                super::aurora_direct_string_len(bool_value(true));
            }
            "invalid-uint-literal" => {
                super::aurora_direct_box_uint_literal(b"abc".as_ptr(), 3);
            }
            "to-lower-receiver" => {
                super::aurora_direct_string_to_lower(bool_value(true));
            }
            "to-upper-receiver" => {
                super::aurora_direct_string_to_upper(bool_value(true));
            }
            "strip-prefix-arg" => {
                super::aurora_direct_string_strip_prefix(string_value("prefix"), bool_value(true));
            }
            "strip-prefix-receiver" => {
                super::aurora_direct_string_strip_prefix(bool_value(true), string_value("p"));
            }
            "strip-suffix-arg" => {
                super::aurora_direct_string_strip_suffix(string_value("suffix"), bool_value(true));
            }
            "strip-suffix-receiver" => {
                super::aurora_direct_string_strip_suffix(bool_value(true), string_value("x"));
            }
            "trim-receiver" => {
                super::aurora_direct_string_trim(bool_value(true));
            }
            "join-part-element" => {
                let vec = super::aurora_direct_vec_empty();
                super::aurora_direct_vec_push_in_place(vec, int_value(1));
                super::aurora_direct_string_join(string_value(", "), vec);
            }
            "join-parts" => {
                super::aurora_direct_string_join(string_value(", "), int_value(1));
            }
            "join-separator" => {
                super::aurora_direct_string_join(bool_value(true), string_vec(&["a", "b"]));
            }
            "abs-type" => {
                super::aurora_direct_abs(string_value("oops"));
            }
            "min-mismatch" => {
                super::aurora_direct_min(int_value(1), float_value(2.0));
            }
            "max-mismatch" => {
                super::aurora_direct_max(int_value(1), float_value(2.0));
            }
            "sqrt-type" => {
                super::aurora_direct_sqrt(int_value(9));
            }
            "parse-int32-type" => {
                super::aurora_direct_parse_int32(bool_value(true));
            }
            "parse-int64-type" => {
                super::aurora_direct_parse_int64(bool_value(true));
            }
            "parse-float64-type" => {
                super::aurora_direct_parse_float64(bool_value(true));
            }
            "map-index-missing" => {
                let map = super::aurora_direct_map_empty();
                expect_option_none(super::aurora_direct_map_set_in_place(
                    map,
                    string_value("name"),
                    int_value(1),
                ));
                super::aurora_direct_map_index(map, string_value("missing"), 2, 7);
            }
            "map-index-missing-no-span" => {
                super::aurora_direct_map_index(
                    super::aurora_direct_map_empty(),
                    string_value("missing"),
                    0,
                    0,
                );
            }
            "vec-extend-type" => {
                super::aurora_direct_vec_extend_in_place(
                    super::aurora_direct_vec_empty(),
                    int_value(1),
                );
            }
            "map-extend-type" => {
                let map = super::aurora_direct_map_empty();
                super::aurora_direct_map_extend_in_place(map, int_value(1));
            }
            "variant-payload-none" => {
                let ready = super::aurora_direct_enum_variant(
                    b"Status".as_ptr(),
                    "Status".len(),
                    b"Ready".as_ptr(),
                    "Ready".len(),
                    std::ptr::null_mut(),
                    0,
                );
                super::aurora_direct_variant_payload(ready, 0);
            }
            "variant-payload-type" => {
                super::aurora_direct_variant_payload(int_value(1), 0);
            }
            "instance-get-missing" => {
                let empty =
                    super::aurora_direct_instance_empty(b"Counter".as_ptr(), "Counter".len());
                super::aurora_direct_instance_get_field(empty, b"value".as_ptr(), "value".len());
            }
            "instance-get-type" => {
                super::aurora_direct_instance_get_field(
                    int_value(1),
                    b"value".as_ptr(),
                    "value".len(),
                );
            }
            "range-current-type" => {
                super::aurora_direct_range_current(int_value(1));
            }
            "range-current-overflow" => {
                let range = boxed_value(Value::Range(RangeValue {
                    start: i128::from(i64::MAX) + 1,
                    end: 0,
                }));
                super::aurora_direct_range_current(range);
            }
            "range-end-type" => {
                super::aurora_direct_range_end(int_value(1));
            }
            "range-end-overflow" => {
                let range = boxed_value(Value::Range(RangeValue {
                    start: 0,
                    end: i128::from(i64::MAX) + 1,
                }));
                super::aurora_direct_range_end(range);
            }
            "range-advance-type" => {
                super::aurora_direct_range_advance(int_value(1));
            }
            "vec-len-type" => {
                super::aurora_direct_vec_len(int_value(1));
            }
            "vec-push-type" => {
                super::aurora_direct_vec_push_in_place(int_value(1), int_value(2));
            }
            "map-len-type" => {
                super::aurora_direct_map_len(int_value(1));
            }
            "map-index-type" => {
                super::aurora_direct_map_index(int_value(1), string_value("name"), 0, 0);
            }
            "map-set-type" => {
                super::aurora_direct_map_set_in_place(
                    int_value(1),
                    string_value("name"),
                    int_value(1),
                );
            }
            "map-set-index-type" => {
                super::aurora_direct_map_set_index_in_place(
                    int_value(1),
                    string_value("name"),
                    int_value(1),
                    0,
                    0,
                );
            }
            "map-clear-type" => {
                super::aurora_direct_map_clear_in_place(int_value(1));
            }
            "map-keys-type" => {
                super::aurora_direct_map_keys(int_value(1));
            }
            "map-values-type" => {
                super::aurora_direct_map_values(int_value(1));
            }
            "map-entries-type" => {
                super::aurora_direct_map_entries(int_value(1));
            }
            "map-extend-target-type" => {
                super::aurora_direct_map_extend_in_place(
                    int_value(1),
                    super::aurora_direct_map_empty(),
                );
            }
            "set-len-type" => {
                super::aurora_direct_set_len(int_value(1));
            }
            "set-is-empty-type" => {
                super::aurora_direct_set_is_empty(int_value(1));
            }
            "set-contains-type" => {
                super::aurora_direct_set_contains(int_value(1), int_value(2));
            }
            "set-insert-type" => {
                super::aurora_direct_set_insert_in_place(int_value(1), int_value(2));
            }
            "set-remove-type" => {
                super::aurora_direct_set_remove_in_place(int_value(1), int_value(2));
            }
            "set-index-type" => {
                super::aurora_direct_set_index_option(int_value(1), 0);
            }
            "tcp-read-all-type" => {
                super::aurora_direct_tcp_stream_read_all(bool_value(true), duration_value(1));
            }
            "tcp-read-line-type" => {
                super::aurora_direct_tcp_stream_read_line(bool_value(true), duration_value(1));
            }
            "tcp-read-bytes-count-type" => {
                super::aurora_direct_tcp_stream_read_bytes(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "tcp-read-bytes-type" => {
                super::aurora_direct_tcp_stream_read_bytes(
                    bool_value(true),
                    int_value(1),
                    duration_value(1),
                );
            }
            "tcp-read-bytes-negative-count" => {
                super::aurora_direct_tcp_stream_read_bytes(
                    bool_value(true),
                    int_value(-1),
                    duration_value(1),
                );
            }
            "tcp-read-exact-count-type" => {
                super::aurora_direct_tcp_stream_read_exact(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "tcp-read-exact-type" => {
                super::aurora_direct_tcp_stream_read_exact(
                    bool_value(true),
                    int_value(1),
                    duration_value(1),
                );
            }
            "tcp-read-exact-negative-count" => {
                super::aurora_direct_tcp_stream_read_exact(
                    bool_value(true),
                    int_value(-1),
                    duration_value(1),
                );
            }
            "tcp-write-all-text-type" => {
                super::aurora_direct_tcp_stream_write_all(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "tcp-write-all-type" => {
                super::aurora_direct_tcp_stream_write_all(
                    bool_value(true),
                    string_value("hello"),
                    duration_value(1),
                );
            }
            "tcp-write-bytes-bytes-type" => {
                super::aurora_direct_tcp_stream_write_bytes(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "tcp-write-bytes-type" => {
                super::aurora_direct_tcp_stream_write_bytes(
                    bool_value(true),
                    int_vec(&[1, 2]),
                    duration_value(1),
                );
            }
            "tcp-shutdown-read-type" => {
                super::aurora_direct_tcp_stream_shutdown_read(bool_value(true));
            }
            "tcp-shutdown-write-type" => {
                super::aurora_direct_tcp_stream_shutdown_write(bool_value(true));
            }
            "tcp-shutdown-both-type" => {
                super::aurora_direct_tcp_stream_shutdown_both(bool_value(true));
            }
            "tcp-flush-type" => {
                super::aurora_direct_tcp_stream_flush(bool_value(true));
            }
            "tcp-local-addr-type" => {
                super::aurora_direct_tcp_stream_local_addr(bool_value(true));
            }
            "tcp-peer-addr-type" => {
                super::aurora_direct_tcp_stream_peer_addr(bool_value(true));
            }
            "tcp-close-type" => {
                super::aurora_direct_tcp_stream_close(bool_value(true));
            }
            "udp-send-text-address-type" => {
                super::aurora_direct_udp_socket_send_text(
                    bool_value(true),
                    bool_value(true),
                    string_value("hello"),
                    duration_value(1),
                );
            }
            "udp-send-text-text-type" => {
                super::aurora_direct_udp_socket_send_text(
                    bool_value(true),
                    string_value("127.0.0.1:9"),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "udp-send-text-type" => {
                super::aurora_direct_udp_socket_send_text(
                    bool_value(true),
                    string_value("127.0.0.1:9"),
                    string_value("hello"),
                    duration_value(1),
                );
            }
            "udp-send-bytes-address-type" => {
                super::aurora_direct_udp_socket_send_bytes(
                    bool_value(true),
                    bool_value(true),
                    int_vec(&[1, 2]),
                    duration_value(1),
                );
            }
            "udp-send-bytes-bytes-type" => {
                super::aurora_direct_udp_socket_send_bytes(
                    bool_value(true),
                    string_value("127.0.0.1:9"),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "udp-send-bytes-type" => {
                super::aurora_direct_udp_socket_send_bytes(
                    bool_value(true),
                    string_value("127.0.0.1:9"),
                    int_vec(&[1, 2]),
                    duration_value(1),
                );
            }
            "udp-recv-count-type" => {
                super::aurora_direct_udp_socket_recv(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "udp-recv-negative-count" => {
                super::aurora_direct_udp_socket_recv(
                    bool_value(true),
                    int_value(-1),
                    duration_value(1),
                );
            }
            "udp-recv-type" => {
                super::aurora_direct_udp_socket_recv(
                    bool_value(true),
                    int_value(1),
                    duration_value(1),
                );
            }
            "udp-recv-from-count-type" => {
                super::aurora_direct_udp_socket_recv_from(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "udp-recv-from-negative-count" => {
                super::aurora_direct_udp_socket_recv_from(
                    bool_value(true),
                    int_value(-1),
                    duration_value(1),
                );
            }
            "udp-recv-from-type" => {
                super::aurora_direct_udp_socket_recv_from(
                    bool_value(true),
                    int_value(1),
                    duration_value(1),
                );
            }
            "udp-local-addr-type" => {
                super::aurora_direct_udp_socket_local_addr(bool_value(true));
            }
            "udp-peer-addr-type" => {
                super::aurora_direct_udp_socket_peer_addr(bool_value(true));
            }
            "udp-close-type" => {
                super::aurora_direct_udp_socket_close(bool_value(true));
            }
            "udp-datagram-address-type" => {
                super::aurora_direct_udp_datagram_address(bool_value(true));
            }
            "udp-datagram-bytes-type" => {
                super::aurora_direct_udp_datagram_bytes(bool_value(true));
            }
            "udp-datagram-text-type" => {
                super::aurora_direct_udp_datagram_text(bool_value(true));
            }
            "process-supervisor-wait-type" => {
                super::aurora_direct_process_supervisor_wait(bool_value(true), duration_value(1));
            }
            "process-supervisor-wait-or-none-type" => {
                super::aurora_direct_process_supervisor_wait_or_none(
                    bool_value(true),
                    duration_value(1),
                );
            }
            "process-supervisor-stop-type" => {
                super::aurora_direct_process_supervisor_stop(bool_value(true));
            }
            "process-supervisor-is-empty-type" => {
                super::aurora_direct_process_supervisor_is_empty(bool_value(true));
            }
            "process-supervisor-close-type" => {
                super::aurora_direct_process_supervisor_close(bool_value(true));
            }
            "process-child-stdin-type" => {
                super::aurora_direct_process_child_stdin(bool_value(true));
            }
            "process-child-stdout-type" => {
                super::aurora_direct_process_child_stdout(bool_value(true));
            }
            "process-child-stderr-type" => {
                super::aurora_direct_process_child_stderr(bool_value(true));
            }
            "process-child-wait-type" => {
                super::aurora_direct_process_child_wait(bool_value(true), duration_value(1));
            }
            "process-child-wait-or-none-type" => {
                super::aurora_direct_process_child_wait_or_none(
                    bool_value(true),
                    duration_value(1),
                );
            }
            "process-child-wait-ok-type" => {
                super::aurora_direct_process_child_wait_ok(bool_value(true), duration_value(1));
            }
            "process-child-kill-type" => {
                super::aurora_direct_process_child_kill(bool_value(true));
            }
            "process-child-terminate-type" => {
                super::aurora_direct_process_child_terminate(bool_value(true));
            }
            "process-child-close-type" => {
                super::aurora_direct_process_child_close(bool_value(true));
            }
            "process-pipe-read-all-type" => {
                super::aurora_direct_process_pipe_read_all(bool_value(true));
            }
            "process-pipe-read-line-type" => {
                super::aurora_direct_process_pipe_read_line(bool_value(true), duration_value(1));
            }
            "process-pipe-read-bytes-count-type" => {
                super::aurora_direct_process_pipe_read_bytes(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "process-pipe-read-bytes-negative-count" => {
                super::aurora_direct_process_pipe_read_bytes(
                    bool_value(true),
                    int_value(-1),
                    duration_value(1),
                );
            }
            "process-pipe-read-bytes-type" => {
                super::aurora_direct_process_pipe_read_bytes(
                    bool_value(true),
                    int_value(1),
                    duration_value(1),
                );
            }
            "process-pipe-write-all-text-type" => {
                super::aurora_direct_process_pipe_write_all(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "process-pipe-write-all-type" => {
                super::aurora_direct_process_pipe_write_all(
                    bool_value(true),
                    string_value("hello"),
                    duration_value(1),
                );
            }
            "process-pipe-write-bytes-bytes-type" => {
                super::aurora_direct_process_pipe_write_bytes(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "process-pipe-write-bytes-type" => {
                super::aurora_direct_process_pipe_write_bytes(
                    bool_value(true),
                    int_vec(&[1, 2]),
                    duration_value(1),
                );
            }
            "process-pipe-flush-type" => {
                super::aurora_direct_process_pipe_flush(bool_value(true));
            }
            "process-pipe-close-type" => {
                super::aurora_direct_process_pipe_close(bool_value(true));
            }
            "process-completed-status-type" => {
                super::aurora_direct_process_completed_status(bool_value(true));
            }
            "process-completed-success-type" => {
                super::aurora_direct_process_completed_success(bool_value(true));
            }
            "process-completed-stdout-type" => {
                super::aurora_direct_process_completed_stdout(bool_value(true));
            }
            "process-completed-stderr-type" => {
                super::aurora_direct_process_completed_stderr(bool_value(true));
            }
            "process-completed-stdout-bytes-type" => {
                super::aurora_direct_process_completed_stdout_bytes(bool_value(true));
            }
            "process-completed-stderr-bytes-type" => {
                super::aurora_direct_process_completed_stderr_bytes(bool_value(true));
            }
            "process-completed-check-type" => {
                super::aurora_direct_process_completed_check(bool_value(true));
            }
            "net-connect-type" => {
                super::aurora_direct_net_connect(bool_value(true));
            }
            "net-connect-timeout-type" => {
                super::aurora_direct_net_connect_timeout(bool_value(true), duration_value(1));
            }
            "net-listen-type" => {
                super::aurora_direct_net_listen(bool_value(true));
            }
            "net-udp-bind-type" => {
                super::aurora_direct_net_udp_bind(bool_value(true));
            }
            "net-unix-listen-type" => {
                super::aurora_direct_net_unix_listen(bool_value(true));
            }
            "net-unix-connect-type" => {
                super::aurora_direct_net_unix_connect(bool_value(true));
            }
            "net-unix-connect-timeout-type" => {
                super::aurora_direct_net_unix_connect_timeout(bool_value(true), duration_value(1));
            }
            "net-tls-listen-address-type" => {
                super::aurora_direct_net_tls_listen(
                    bool_value(true),
                    string_value("/tmp/cert.pem"),
                    string_value("/tmp/key.pem"),
                );
            }
            "net-tls-connect-address-type" => {
                super::aurora_direct_net_tls_connect(
                    bool_value(true),
                    string_value("localhost"),
                    string_value("/tmp/ca.pem"),
                );
            }
            "net-http-listen-type" => {
                super::aurora_direct_net_http_listen(bool_value(true));
            }
            "net-websocket-listen-type" => {
                super::aurora_direct_net_websocket_listen(bool_value(true));
            }
            "net-websocket-connect-type" => {
                super::aurora_direct_net_websocket_connect(bool_value(true));
            }
            "net-websocket-connect-timeout-type" => {
                super::aurora_direct_net_websocket_connect_timeout(
                    bool_value(true),
                    duration_value(1),
                );
            }
            "http-listener-accept-type" => {
                super::aurora_direct_http_listener_accept(bool_value(true), duration_value(1));
            }
            "http-listener-local-addr-type" => {
                super::aurora_direct_http_listener_local_addr(bool_value(true));
            }
            "http-listener-close-type" => {
                super::aurora_direct_http_listener_close(bool_value(true));
            }
            "http-exchange-method-type" => {
                super::aurora_direct_http_exchange_method(bool_value(true));
            }
            "http-exchange-path-type" => {
                super::aurora_direct_http_exchange_path(bool_value(true));
            }
            "http-exchange-headers-type" => {
                super::aurora_direct_http_exchange_headers(bool_value(true));
            }
            "http-exchange-body-text-type" => {
                super::aurora_direct_http_exchange_body_text(bool_value(true));
            }
            "http-exchange-body-bytes-type" => {
                super::aurora_direct_http_exchange_body_bytes(bool_value(true));
            }
            "http-exchange-respond-text-type" => {
                super::aurora_direct_http_exchange_respond_text(
                    bool_value(true),
                    int_value(200),
                    string_value("ok"),
                    string_map(&[]),
                );
            }
            "http-exchange-respond-bytes-type" => {
                super::aurora_direct_http_exchange_respond_bytes(
                    bool_value(true),
                    int_value(200),
                    int_vec(&[1, 2]),
                    string_map(&[]),
                );
            }
            "http-response-status-type" => {
                super::aurora_direct_http_response_status(bool_value(true));
            }
            "http-response-reason-type" => {
                super::aurora_direct_http_response_reason(bool_value(true));
            }
            "http-response-headers-type" => {
                super::aurora_direct_http_response_headers(bool_value(true));
            }
            "http-response-text-type" => {
                super::aurora_direct_http_response_text(bool_value(true));
            }
            "http-response-bytes-type" => {
                super::aurora_direct_http_response_bytes(bool_value(true));
            }
            "websocket-listener-accept-type" => {
                super::aurora_direct_websocket_listener_accept(bool_value(true), duration_value(1));
            }
            "websocket-listener-local-addr-type" => {
                super::aurora_direct_websocket_listener_local_addr(bool_value(true));
            }
            "websocket-send-text-type" => {
                super::aurora_direct_websocket_send_text(
                    bool_value(true),
                    string_value("hello"),
                    duration_value(1),
                );
            }
            "websocket-send-bytes-type" => {
                super::aurora_direct_websocket_send_bytes(
                    bool_value(true),
                    int_vec(&[1, 2]),
                    duration_value(1),
                );
            }
            "websocket-recv-text-type" => {
                super::aurora_direct_websocket_recv_text(bool_value(true), duration_value(1));
            }
            "websocket-recv-bytes-type" => {
                super::aurora_direct_websocket_recv_bytes(bool_value(true), duration_value(1));
            }
            "websocket-close-type" => {
                super::aurora_direct_websocket_close(bool_value(true));
            }
            "unix-listener-accept-type" => {
                super::aurora_direct_unix_listener_accept(bool_value(true), duration_value(1));
            }
            "unix-listener-close-type" => {
                super::aurora_direct_unix_listener_close(bool_value(true));
            }
            "unix-stream-read-line-type" => {
                super::aurora_direct_unix_stream_read_line(bool_value(true), duration_value(1));
            }
            "unix-stream-read-exact-count-type" => {
                super::aurora_direct_unix_stream_read_exact(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "unix-stream-read-exact-negative-count" => {
                super::aurora_direct_unix_stream_read_exact(
                    bool_value(true),
                    int_value(-1),
                    duration_value(1),
                );
            }
            "unix-stream-read-exact-type" => {
                super::aurora_direct_unix_stream_read_exact(
                    bool_value(true),
                    int_value(1),
                    duration_value(1),
                );
            }
            "unix-stream-write-all-text-type" => {
                super::aurora_direct_unix_stream_write_all(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "unix-stream-write-all-type" => {
                super::aurora_direct_unix_stream_write_all(
                    bool_value(true),
                    string_value("hello"),
                    duration_value(1),
                );
            }
            "unix-stream-close-type" => {
                super::aurora_direct_unix_stream_close(bool_value(true));
            }
            "tls-listener-accept-type" => {
                super::aurora_direct_tls_listener_accept(bool_value(true), duration_value(1));
            }
            "tls-listener-local-addr-type" => {
                super::aurora_direct_tls_listener_local_addr(bool_value(true));
            }
            "tls-listener-close-type" => {
                super::aurora_direct_tls_listener_close(bool_value(true));
            }
            "tls-stream-read-line-type" => {
                super::aurora_direct_tls_stream_read_line(bool_value(true), duration_value(1));
            }
            "tls-stream-read-exact-count-type" => {
                super::aurora_direct_tls_stream_read_exact(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "tls-stream-read-exact-negative-count" => {
                super::aurora_direct_tls_stream_read_exact(
                    bool_value(true),
                    int_value(-1),
                    duration_value(1),
                );
            }
            "tls-stream-read-exact-type" => {
                super::aurora_direct_tls_stream_read_exact(
                    bool_value(true),
                    int_value(1),
                    duration_value(1),
                );
            }
            "tls-stream-write-all-text-type" => {
                super::aurora_direct_tls_stream_write_all(
                    bool_value(true),
                    bool_value(true),
                    duration_value(1),
                );
            }
            "tls-stream-write-all-type" => {
                super::aurora_direct_tls_stream_write_all(
                    bool_value(true),
                    string_value("hello"),
                    duration_value(1),
                );
            }
            "tls-stream-close-type" => {
                super::aurora_direct_tls_stream_close(bool_value(true));
            }
            "sleep-ms-negative" => {
                super::aurora_direct_sleep_ms(-1);
            }
            "fail-division-no-span" => {
                super::aurora_direct_fail_division_by_zero(0, 0);
            }
            "fail-int32-overflow-no-span" => {
                super::aurora_direct_fail_int32_overflow(123, 0, 0);
            }
            "vec-index-oob-no-span" => {
                super::aurora_direct_vec_index(int_vec(&[1]), 5, 0, 0);
            }
            "vec-set-oob-no-span" => {
                super::aurora_direct_vec_set_index_in_place(int_vec(&[1]), 5, int_value(9), 0, 0);
            }
            "vec-set-oob-span" => {
                super::aurora_direct_vec_set_index_in_place(int_vec(&[1]), 5, int_value(9), 2, 7);
            }
            "vec-method-set-too-negative" => {
                super::aurora_direct_vec_set_in_place(int_vec(&[1, 2, 3, 4]), -5, int_value(9));
            }
            "vec-method-remove-too-negative" => {
                super::aurora_direct_vec_remove_in_place(int_vec(&[1, 2, 3, 4]), -5);
            }
            "vec-method-swap-too-negative" => {
                super::aurora_direct_vec_swap_in_place(int_vec(&[1, 2, 3, 4]), -5, -1);
            }
            "vec-method-insert-too-negative" => {
                super::aurora_direct_vec_insert_in_place(int_vec(&[1, 2, 3, 4]), -5, int_value(9));
            }
            "vec-method-insert-negative-empty" => {
                super::aurora_direct_vec_insert_in_place(int_vec(&[]), -1, int_value(9));
            }
            "vec-indexed-write-too-negative" => {
                super::aurora_direct_vec_set_index_in_place(
                    int_vec(&[1, 2, 3, 4]),
                    -5,
                    int_value(9),
                    3,
                    7,
                );
            }
            "unbox-i64-overflow" => {
                super::aurora_direct_unbox_i64(boxed_value(Value::Int(
                    IntegerValue::from_literal((i64::MAX as u128) + 1),
                )));
            }
            "unbox-i64-type" => {
                super::aurora_direct_unbox_i64(bool_value(true));
            }
            "unbox-int64-overflow" => {
                super::aurora_direct_unbox_int64(boxed_value(Value::Int(
                    IntegerValue::from_literal((i64::MAX as u128) + 1),
                )));
            }
            "unbox-int64-type" => {
                super::aurora_direct_unbox_int64(bool_value(true));
            }
            "unbox-u64-negative" => {
                super::aurora_direct_unbox_u64(int_value(-1));
            }
            "unbox-u64-overflow" => {
                super::aurora_direct_unbox_u64(boxed_value(Value::Int(
                    IntegerValue::from_literal((u64::MAX as u128) + 1),
                )));
            }
            "unbox-u64-type" => {
                super::aurora_direct_unbox_u64(bool_value(true));
            }
            "unbox-f64-type" => {
                super::aurora_direct_unbox_f64(int_value(1));
            }
            "unbox-bool-type" => {
                super::aurora_direct_unbox_bool(int_value(1));
            }
            "condition-type" => {
                super::aurora_direct_value_as_condition(string_value("aurora"));
            }
            "unary-invalid-op" => {
                super::aurora_direct_unary_value(99, int_value(1));
            }
            "unary-at-no-span" => {
                super::aurora_direct_unary_value_at(0, string_value("aurora"), 0, 0);
            }
            "unary-at-span" => {
                super::aurora_direct_unary_value_at(0, string_value("aurora"), 2, 7);
            }
            "binary-invalid-op" => {
                super::aurora_direct_binary_value(99, int_value(1), int_value(2));
            }
            "binary-floor-zero-no-span" => {
                super::aurora_direct_binary_value(13, int_value(1), int_value(0));
            }
            "binary-at-no-span" => {
                super::aurora_direct_binary_value_at(
                    0,
                    string_value("aurora"),
                    bool_value(true),
                    0,
                    0,
                );
            }
            "binary-at-span" => {
                super::aurora_direct_binary_value_at(
                    0,
                    string_value("aurora"),
                    bool_value(true),
                    2,
                    9,
                );
            }
            "cast-no-span" => {
                super::aurora_direct_cast_value(
                    string_value("aurora"),
                    b"int32".as_ptr(),
                    "int32".len(),
                );
            }
            "cast-at-span" => {
                super::aurora_direct_cast_value_at(
                    string_value("aurora"),
                    b"int32".as_ptr(),
                    "int32".len(),
                    4,
                    3,
                );
            }
            "task-join-error" => {
                let task = boxed_value(Value::Task(TaskValue::from_handle(thread::spawn(|| {
                    Err(Diagnostic::new("boom"))
                }))));
                let joined = super::aurora_direct_task_join(task);
                assert_eq!(expect_task_result_error_message(joined), "boom");
                return;
            }
            other => panic!("unexpected runtime helper case: {other}"),
        }
    }

    for (case, expected) in [
        (
            "bytes-value-type",
            "`bytes` expects `Vec[uint8]`, found `String`",
        ),
        ("bytes-element-range", "`bytes` expects `Vec[uint8]`"),
        ("bool-value-type", "`flag` expects `bool`, found `String`"),
        ("i32-overflow", "`count` expects `int32`"),
        ("i32-value-type", "`count` expects `int32`, found `String`"),
        (
            "headers-map-type",
            "`headers` expects `Map[String, String]`, found `String`",
        ),
        (
            "headers-key-type",
            "`headers` expects `String`, found `integer`",
        ),
        (
            "optional-timeout-type",
            "`timeout` expects `Duration`, found `String`",
        ),
        (
            "optional-timeout-negative",
            "`timeout` duration must be non-negative",
        ),
        (
            "process-timeout-type",
            "`timeout` expects `Duration`, found `String`",
        ),
        (
            "duration-type",
            "`duration` expects `Duration`, found `String`",
        ),
        (
            "duration-negative",
            "`duration` duration must be non-negative",
        ),
        (
            "supervisor-max-too-low",
            "`max_restarts` expects `max_restarts` to be -1 or greater",
        ),
        (
            "command-vec-type",
            "`command` expects `Vec[String]`, found `String`",
        ),
        (
            "command-element-type",
            "`command` expects `String`, found `integer`",
        ),
        (
            "optional-string-malformed",
            "`cwd` expects `Option[String]`, found malformed option payload",
        ),
        (
            "optional-string-payload-type",
            "`cwd` expects `String`, found `bool`",
        ),
        (
            "optional-string-type",
            "`cwd` expects `Option[String]`, found `integer`",
        ),
        (
            "process-start-command-type",
            "`process.start(...)` expects `Vec[String]`, found `bool`",
        ),
        (
            "process-start-cwd-type",
            "`process.start(...)` expects `Option[String]`, found `bool`",
        ),
        (
            "process-start-env-type",
            "`process.start(...)` expects `Map[String, String]`, found `bool`",
        ),
        (
            "process-start-group-type",
            "`process.start(...)` expects `bool`, found `String`",
        ),
        (
            "process-run-command-type",
            "`process.run(...)` expects `Vec[String]`, found `bool`",
        ),
        (
            "process-run-timeout-type",
            "`process.run(...)` expects `Duration`, found `String`",
        ),
        (
            "process-run-group-type",
            "`process.run(...)` expects `bool`, found `String`",
        ),
        ("arg-buffer-negative-size", "invalid arg buffer size"),
        ("arg-buffer-negative-index", "invalid arg index"),
        ("cleanup-negative-arg-count", "invalid cleanup arg count"),
        ("cleanup-null-thunk", "invalid cleanup thunk pointer"),
        (
            "cleanup-refresh-negative-arg-count",
            "invalid cleanup arg count",
        ),
        (
            "cleanup-refresh-null-thunk",
            "invalid cleanup thunk pointer",
        ),
        (
            "queue-capacity-zero",
            "`queue(capacity=...)` expects a positive `int32`",
        ),
        ("queue-send-type", "expected `Queue`, found `bool`"),
        (
            "queue-send-timeout-negative",
            "invalid queue timeout duration",
        ),
        ("queue-try-send-type", "expected `Queue`, found `bool`"),
        ("queue-recv-type", "expected `Queue`, found `bool`"),
        (
            "queue-recv-timeout-negative",
            "invalid queue timeout duration",
        ),
        (
            "queue-recv-in-task-group-queue-type",
            "expected `Queue`, found `bool`",
        ),
        (
            "queue-recv-in-task-group-group-type",
            "expected `TaskGroup`, found `bool`",
        ),
        (
            "queue-recv-registered-producers-type",
            "expected `Queue`, found `bool`",
        ),
        (
            "wait-any-timeout-negative",
            "invalid wait_any timeout duration",
        ),
        (
            "wait-all-timeout-negative",
            "invalid wait_all timeout duration",
        ),
        (
            "task-group-cancel-type",
            "expected `TaskGroup`, found `bool`",
        ),
        (
            "task-group-close-type",
            "expected `TaskGroup`, found `bool`",
        ),
        ("io-write-type", "expected `String`, found `bool`"),
        ("fs-exists-type", "expected `String`, found `bool`"),
        ("fs-read-to-string-type", "expected `String`, found `bool`"),
        ("fs-read-bytes-type", "expected `String`, found `bool`"),
        (
            "fs-write-string-path-type",
            "expected `String`, found `bool`",
        ),
        (
            "fs-write-string-text-type",
            "expected `String`, found `bool`",
        ),
        (
            "fs-write-bytes-path-type",
            "`fs.write_bytes(...)` expects `String`, found `bool`",
        ),
        (
            "fs-append-string-path-type",
            "expected `String`, found `bool`",
        ),
        (
            "fs-append-string-text-type",
            "expected `String`, found `bool`",
        ),
        (
            "fs-append-bytes-path-type",
            "`fs.append_bytes(...)` expects `String`, found `bool`",
        ),
        (
            "fs-append-bytes-bytes-type",
            "`fs.append_bytes(...)` expects `Vec[uint8]`, found `bool`",
        ),
        ("fs-create-dir-type", "expected `String`, found `bool`"),
        ("fs-read-dir-type", "expected `String`, found `bool`"),
        ("fs-remove-file-type", "expected `String`, found `bool`"),
        ("fs-open-type", "expected `String`, found `bool`"),
        ("fs-create-type", "expected `String`, found `bool`"),
        ("fs-append-type", "expected `String`, found `bool`"),
        ("file-read-all-type", "expected `fs.File`, found `bool`"),
        ("file-read-bytes-type", "expected `fs.File`, found `bool`"),
        (
            "file-write-all-text-type",
            "expected `String`, found `bool`",
        ),
        (
            "file-write-all-file-type",
            "expected `fs.File`, found `bool`",
        ),
        (
            "file-write-bytes-file-type",
            "expected `fs.File`, found `bool`",
        ),
        ("file-flush-type", "expected `fs.File`, found `bool`"),
        ("file-close-type", "expected `fs.File`, found `bool`"),
        ("contains-arg", "`contains` requires a `String` argument"),
        ("contains-receiver", "expected `String`, found `bool`"),
        (
            "starts-with-arg",
            "`starts_with` requires a `String` argument",
        ),
        ("starts-with-receiver", "expected `String`, found `bool`"),
        ("ends-with-arg", "`ends_with` requires a `String` argument"),
        ("ends-with-receiver", "expected `String`, found `bool`"),
        ("split-arg", "`split` requires a `String` argument"),
        ("split-receiver", "expected `String`, found `bool`"),
        ("replace-from", "`replace` requires `String` for `from`"),
        ("replace-to", "`replace` requires `String` for `to`"),
        ("replace-receiver", "expected `String`, found `bool`"),
        ("string-len-type", "expected `String`, found `bool`"),
        (
            "invalid-uint-literal",
            "invalid embedded uint literal `abc`",
        ),
        ("to-lower-receiver", "expected `String`, found `bool`"),
        ("to-upper-receiver", "expected `String`, found `bool`"),
        (
            "strip-prefix-arg",
            "`strip_prefix` requires a `String` argument",
        ),
        ("strip-prefix-receiver", "expected `String`, found `bool`"),
        (
            "strip-suffix-arg",
            "`strip_suffix` requires a `String` argument",
        ),
        ("strip-suffix-receiver", "expected `String`, found `bool`"),
        ("trim-receiver", "expected `String`, found `bool`"),
        ("join-part-element", "`join` requires `Vec[String]`"),
        ("join-parts", "`join` requires `Vec[String]`"),
        ("join-separator", "expected `String`, found `bool`"),
        ("abs-type", "`abs(...)` expects an integer or float value"),
        (
            "min-mismatch",
            "`min(...)` expects matching numeric arguments",
        ),
        (
            "max-mismatch",
            "`max(...)` expects matching numeric arguments",
        ),
        ("sqrt-type", "`sqrt(...)` expects `float32` or `float64`"),
        (
            "parse-int32-type",
            "`parse_int32(...)` expects `String`, found `bool`",
        ),
        (
            "parse-int64-type",
            "`parse_int64(...)` expects `String`, found `bool`",
        ),
        (
            "parse-float64-type",
            "`parse_float64(...)` expects `String`, found `bool`",
        ),
        ("map-index-missing", "map key `missing` was not present"),
        (
            "map-index-missing-no-span",
            "map key `missing` was not present",
        ),
        (
            "vec-extend-type",
            "`extend` requires another `Vec[T]` value",
        ),
        (
            "map-extend-type",
            "`extend` requires another `Map[K, V]` value",
        ),
        ("variant-payload-none", "does not carry a payload"),
        (
            "variant-payload-type",
            "expected enum value, found `integer`",
        ),
        (
            "instance-get-missing",
            "class `Counter` has no field `value`",
        ),
        (
            "instance-get-type",
            "cannot access field `value` on non-instance `integer`",
        ),
        ("range-current-type", "expected `Range`, found `integer`"),
        (
            "range-current-overflow",
            "range start is outside host i64 bounds",
        ),
        ("range-end-type", "expected `Range`, found `integer`"),
        ("range-end-overflow", "range end is outside host i64 bounds"),
        ("range-advance-type", "expected `Range`, found `integer`"),
        ("vec-len-type", "expected `Vec`, found `integer`"),
        ("vec-push-type", "expected `Vec`, found `integer`"),
        ("map-len-type", "expected `Map`, found `integer`"),
        ("map-index-type", "expected `Map`, found `integer`"),
        ("map-set-type", "expected `Map`, found `integer`"),
        ("map-set-index-type", "expected `Map`, found `integer`"),
        ("map-clear-type", "expected `Map`, found `integer`"),
        ("map-keys-type", "expected `Map`, found `integer`"),
        ("map-values-type", "expected `Map`, found `integer`"),
        ("map-entries-type", "expected `Map`, found `integer`"),
        ("map-extend-target-type", "expected `Map`, found `integer`"),
        ("set-len-type", "expected `Set`, found `integer`"),
        ("set-is-empty-type", "expected `Set`, found `integer`"),
        ("set-contains-type", "expected `Set`, found `integer`"),
        ("set-insert-type", "expected `Set`, found `integer`"),
        ("set-remove-type", "expected `Set`, found `integer`"),
        ("set-index-type", "expected `Set`, found `integer`"),
        (
            "tcp-read-all-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        (
            "tcp-read-line-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        (
            "tcp-read-bytes-count-type",
            "`read_bytes(...)` expects `int32`, found `bool`",
        ),
        (
            "tcp-read-bytes-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        (
            "tcp-read-bytes-negative-count",
            "`read_bytes(...)` requires a non-negative max_bytes",
        ),
        (
            "tcp-read-exact-count-type",
            "`read_exact(...)` expects `int32`, found `bool`",
        ),
        (
            "tcp-read-exact-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        (
            "tcp-read-exact-negative-count",
            "`read_exact(...)` requires a non-negative count",
        ),
        ("tcp-write-all-text-type", "expected `String`, found `bool`"),
        (
            "tcp-write-all-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        (
            "tcp-write-bytes-bytes-type",
            "`write_bytes(...)` expects `Vec[uint8]`, found `bool`",
        ),
        (
            "tcp-write-bytes-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        (
            "tcp-shutdown-read-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        (
            "tcp-shutdown-write-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        (
            "tcp-shutdown-both-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        ("tcp-flush-type", "expected `net.TcpStream`, found `bool`"),
        (
            "tcp-local-addr-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        (
            "tcp-peer-addr-type",
            "expected `net.TcpStream`, found `bool`",
        ),
        ("tcp-close-type", "expected `net.TcpStream`, found `bool`"),
        (
            "udp-send-text-address-type",
            "`send_text(...)` expects `String`, found `bool`",
        ),
        (
            "udp-send-text-text-type",
            "`send_text(...)` expects `String`, found `bool`",
        ),
        (
            "udp-send-text-type",
            "expected `net.UdpSocket`, found `bool`",
        ),
        (
            "udp-send-bytes-address-type",
            "`send_bytes(...)` expects `String`, found `bool`",
        ),
        (
            "udp-send-bytes-bytes-type",
            "`send_bytes(...)` expects `Vec[uint8]`, found `bool`",
        ),
        (
            "udp-send-bytes-type",
            "expected `net.UdpSocket`, found `bool`",
        ),
        (
            "udp-recv-count-type",
            "`recv(...)` expects `int32`, found `bool`",
        ),
        (
            "udp-recv-negative-count",
            "`recv(...)` requires a non-negative max_bytes",
        ),
        ("udp-recv-type", "expected `net.UdpSocket`, found `bool`"),
        (
            "udp-recv-from-count-type",
            "`recv_from(...)` expects `int32`, found `bool`",
        ),
        (
            "udp-recv-from-negative-count",
            "`recv_from(...)` requires a non-negative max_bytes",
        ),
        (
            "udp-recv-from-type",
            "expected `net.UdpSocket`, found `bool`",
        ),
        (
            "udp-local-addr-type",
            "expected `net.UdpSocket`, found `bool`",
        ),
        (
            "udp-peer-addr-type",
            "expected `net.UdpSocket`, found `bool`",
        ),
        ("udp-close-type", "expected `net.UdpSocket`, found `bool`"),
        (
            "udp-datagram-address-type",
            "expected `net.UdpDatagram`, found `bool`",
        ),
        (
            "udp-datagram-bytes-type",
            "expected `net.UdpDatagram`, found `bool`",
        ),
        (
            "udp-datagram-text-type",
            "expected `net.UdpDatagram`, found `bool`",
        ),
        (
            "process-supervisor-wait-type",
            "expected `process.Supervisor`, found `bool`",
        ),
        (
            "process-supervisor-wait-or-none-type",
            "expected `process.Supervisor`, found `bool`",
        ),
        (
            "process-supervisor-stop-type",
            "expected `process.Supervisor`, found `bool`",
        ),
        (
            "process-supervisor-is-empty-type",
            "expected `process.Supervisor`, found `bool`",
        ),
        (
            "process-supervisor-close-type",
            "expected `process.Supervisor`, found `bool`",
        ),
        (
            "process-child-stdin-type",
            "expected `process.Child`, found `bool`",
        ),
        (
            "process-child-stdout-type",
            "expected `process.Child`, found `bool`",
        ),
        (
            "process-child-stderr-type",
            "expected `process.Child`, found `bool`",
        ),
        (
            "process-child-wait-type",
            "expected `process.Child`, found `bool`",
        ),
        (
            "process-child-wait-or-none-type",
            "expected `process.Child`, found `bool`",
        ),
        (
            "process-child-wait-ok-type",
            "expected `process.Child`, found `bool`",
        ),
        (
            "process-child-kill-type",
            "expected `process.Child`, found `bool`",
        ),
        (
            "process-child-terminate-type",
            "expected `process.Child`, found `bool`",
        ),
        (
            "process-child-close-type",
            "expected `process.Child`, found `bool`",
        ),
        (
            "process-pipe-read-all-type",
            "expected `process.Pipe`, found `bool`",
        ),
        (
            "process-pipe-read-line-type",
            "expected `process.Pipe`, found `bool`",
        ),
        (
            "process-pipe-read-bytes-count-type",
            "`read_bytes(...)` expects `int32`, found `bool`",
        ),
        (
            "process-pipe-read-bytes-negative-count",
            "`read_bytes(...)` expects a non-negative `max_bytes`",
        ),
        (
            "process-pipe-read-bytes-type",
            "expected `process.Pipe`, found `bool`",
        ),
        (
            "process-pipe-write-all-text-type",
            "`write_all(...)` expects `String`, found `bool`",
        ),
        (
            "process-pipe-write-all-type",
            "expected `process.Pipe`, found `bool`",
        ),
        (
            "process-pipe-write-bytes-bytes-type",
            "`write_bytes(...)` expects `Vec[uint8]`, found `bool`",
        ),
        (
            "process-pipe-write-bytes-type",
            "expected `process.Pipe`, found `bool`",
        ),
        (
            "process-pipe-flush-type",
            "expected `process.Pipe`, found `bool`",
        ),
        (
            "process-pipe-close-type",
            "expected `process.Pipe`, found `bool`",
        ),
        (
            "process-completed-status-type",
            "expected `process.Completed`, found `bool`",
        ),
        (
            "process-completed-success-type",
            "expected `process.Completed`, found `bool`",
        ),
        (
            "process-completed-stdout-type",
            "expected `process.Completed`, found `bool`",
        ),
        (
            "process-completed-stderr-type",
            "expected `process.Completed`, found `bool`",
        ),
        (
            "process-completed-stdout-bytes-type",
            "expected `process.Completed`, found `bool`",
        ),
        (
            "process-completed-stderr-bytes-type",
            "expected `process.Completed`, found `bool`",
        ),
        (
            "process-completed-check-type",
            "expected `process.Completed`, found `bool`",
        ),
        ("net-connect-type", "expected `String`, found `bool`"),
        (
            "net-connect-timeout-type",
            "expected `String`, found `bool`",
        ),
        ("net-listen-type", "expected `String`, found `bool`"),
        ("net-udp-bind-type", "expected `String`, found `bool`"),
        ("net-unix-listen-type", "expected `String`, found `bool`"),
        ("net-unix-connect-type", "expected `String`, found `bool`"),
        (
            "net-unix-connect-timeout-type",
            "expected `String`, found `bool`",
        ),
        (
            "net-tls-listen-address-type",
            "`net.tls_listen(...)` expects `String`, found `bool`",
        ),
        (
            "net-tls-connect-address-type",
            "`net.tls_connect(...)` expects `String`, found `bool`",
        ),
        ("net-http-listen-type", "expected `String`, found `bool`"),
        (
            "net-websocket-listen-type",
            "expected `String`, found `bool`",
        ),
        (
            "net-websocket-connect-type",
            "expected `String`, found `bool`",
        ),
        (
            "net-websocket-connect-timeout-type",
            "expected `String`, found `bool`",
        ),
        (
            "http-listener-accept-type",
            "expected `net.HttpListener`, found `bool`",
        ),
        (
            "http-listener-local-addr-type",
            "expected `net.HttpListener`, found `bool`",
        ),
        (
            "http-listener-close-type",
            "expected `net.HttpListener`, found `bool`",
        ),
        (
            "http-exchange-method-type",
            "expected `net.HttpExchange`, found `bool`",
        ),
        (
            "http-exchange-path-type",
            "expected `net.HttpExchange`, found `bool`",
        ),
        (
            "http-exchange-headers-type",
            "expected `net.HttpExchange`, found `bool`",
        ),
        (
            "http-exchange-body-text-type",
            "expected `net.HttpExchange`, found `bool`",
        ),
        (
            "http-exchange-body-bytes-type",
            "expected `net.HttpExchange`, found `bool`",
        ),
        (
            "http-exchange-respond-text-type",
            "expected `net.HttpExchange`, found `bool`",
        ),
        (
            "http-exchange-respond-bytes-type",
            "expected `net.HttpExchange`, found `bool`",
        ),
        (
            "http-response-status-type",
            "expected `net.HttpResponse`, found `bool`",
        ),
        (
            "http-response-reason-type",
            "expected `net.HttpResponse`, found `bool`",
        ),
        (
            "http-response-headers-type",
            "expected `net.HttpResponse`, found `bool`",
        ),
        (
            "http-response-text-type",
            "expected `net.HttpResponse`, found `bool`",
        ),
        (
            "http-response-bytes-type",
            "expected `net.HttpResponse`, found `bool`",
        ),
        (
            "websocket-listener-accept-type",
            "expected `net.WebSocketListener`, found `bool`",
        ),
        (
            "websocket-listener-local-addr-type",
            "expected `net.WebSocketListener`, found `bool`",
        ),
        (
            "websocket-send-text-type",
            "expected `net.WebSocket`, found `bool`",
        ),
        (
            "websocket-send-bytes-type",
            "expected `net.WebSocket`, found `bool`",
        ),
        (
            "websocket-recv-text-type",
            "expected `net.WebSocket`, found `bool`",
        ),
        (
            "websocket-recv-bytes-type",
            "expected `net.WebSocket`, found `bool`",
        ),
        (
            "websocket-close-type",
            "expected `net.WebSocket`, found `bool`",
        ),
        (
            "unix-listener-accept-type",
            "expected `net.UnixListener`, found `bool`",
        ),
        (
            "unix-listener-close-type",
            "expected `net.UnixListener`, found `bool`",
        ),
        (
            "unix-stream-read-line-type",
            "expected `net.UnixStream`, found `bool`",
        ),
        (
            "unix-stream-read-exact-count-type",
            "`read_exact(...)` expects `int32`, found `bool`",
        ),
        (
            "unix-stream-read-exact-negative-count",
            "`read_exact(...)` requires a non-negative count",
        ),
        (
            "unix-stream-read-exact-type",
            "expected `net.UnixStream`, found `bool`",
        ),
        (
            "unix-stream-write-all-text-type",
            "`write_all(...)` expects `String`, found `bool`",
        ),
        (
            "unix-stream-write-all-type",
            "expected `net.UnixStream`, found `bool`",
        ),
        (
            "unix-stream-close-type",
            "expected `net.UnixStream`, found `bool`",
        ),
        (
            "tls-listener-accept-type",
            "expected `net.TlsListener`, found `bool`",
        ),
        (
            "tls-listener-local-addr-type",
            "expected `net.TlsListener`, found `bool`",
        ),
        (
            "tls-listener-close-type",
            "expected `net.TlsListener`, found `bool`",
        ),
        (
            "tls-stream-read-line-type",
            "expected `net.TlsStream`, found `bool`",
        ),
        (
            "tls-stream-read-exact-count-type",
            "`read_exact(...)` expects `int32`, found `bool`",
        ),
        (
            "tls-stream-read-exact-negative-count",
            "`read_exact(...)` requires a non-negative count",
        ),
        (
            "tls-stream-read-exact-type",
            "expected `net.TlsStream`, found `bool`",
        ),
        (
            "tls-stream-write-all-text-type",
            "`write_all(...)` expects `String`, found `bool`",
        ),
        (
            "tls-stream-write-all-type",
            "expected `net.TlsStream`, found `bool`",
        ),
        (
            "tls-stream-close-type",
            "expected `net.TlsStream`, found `bool`",
        ),
        ("sleep-ms-negative", "invalid sleep duration"),
        ("fail-division-no-span", "division by zero"),
        (
            "fail-int32-overflow-no-span",
            "integer value `123` does not fit in `int32`",
        ),
        (
            "vec-index-oob-no-span",
            "vector index `5` is out of bounds for length `1`",
        ),
        (
            "vec-set-oob-no-span",
            "vector index `5` is out of bounds for length `1`",
        ),
        (
            "vec-set-oob-span",
            "vector index `5` is out of bounds for length `1`",
        ),
        (
            "vec-method-set-too-negative",
            "vector set index `-5` is out of bounds for length `4`",
        ),
        (
            "vec-method-remove-too-negative",
            "vector remove index `-5` is out of bounds for length `4`",
        ),
        (
            "vec-method-swap-too-negative",
            "vector swap indices `-5` and `-1` are out of bounds for length `4`",
        ),
        (
            "vec-method-insert-too-negative",
            "vector insert index `-5` is out of bounds for length `4`",
        ),
        (
            "vec-method-insert-negative-empty",
            "vector insert index `-1` is out of bounds for length `0`",
        ),
        (
            "vec-indexed-write-too-negative",
            "vector index `-5` is out of bounds for length `4`",
        ),
        (
            "unbox-i64-overflow",
            "direct backend expected an integer that fits in host i64",
        ),
        (
            "unbox-i64-type",
            "direct backend expected `int32`, found `bool`",
        ),
        (
            "unbox-int64-overflow",
            "integer value `9223372036854775808` does not fit in `int64`",
        ),
        (
            "unbox-int64-type",
            "direct backend expected `int64`, found `bool`",
        ),
        (
            "unbox-u64-negative",
            "direct backend expected an integer that fits in host u64",
        ),
        (
            "unbox-u64-overflow",
            "direct backend expected an integer that fits in host u64",
        ),
        (
            "unbox-u64-type",
            "direct backend expected `uint64`, found `bool`",
        ),
        (
            "unbox-f64-type",
            "direct backend expected `float64`, found `integer`",
        ),
        (
            "unbox-bool-type",
            "direct backend expected `bool`, found `integer`",
        ),
        (
            "condition-type",
            "direct backend cannot use `String` as a branch condition",
        ),
        ("unary-invalid-op", "unknown unary opcode `99`"),
        (
            "unary-at-no-span",
            "unary `-` expects a numeric value, found `String`",
        ),
        (
            "unary-at-span",
            "unary `-` expects a numeric value, found `String`",
        ),
        ("binary-invalid-op", "unknown binary opcode `99`"),
        ("binary-floor-zero-no-span", "division by zero"),
        (
            "binary-at-no-span",
            "unsupported `+` operands `String` and `bool`",
        ),
        (
            "binary-at-span",
            "unsupported `+` operands `String` and `bool`",
        ),
        (
            "cast-no-span",
            "casts are only supported between numeric types, found `String` and `int32`",
        ),
        (
            "cast-at-span",
            "casts are only supported between numeric types, found `String` and `int32`",
        ),
    ] {
        let output = Command::new(std::env::current_exe().expect("test binary should exist"))
            .arg("--exact")
            .arg("native_runtime::tests::direct_runtime_helper_errors_surface_expected_diagnostics")
            .arg("--nocapture")
            .env("AURORA_DIRECT_RUNTIME_CASE", case)
            .output()
            .expect("child test process should run");

        assert!(!output.status.success(), "helper case `{case}` should fail");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "helper case `{case}` stderr should mention `{expected}`"
        );
    }
}

#[test]
fn native_runtime_scalar_helpers_cover_comparisons_unary_ops_and_metadata() {
    assert_eq!(render_bool(0), "false");
    assert_eq!(render_bool(9), "true");
    assert_eq!(
        int32_overflow_message(12),
        "integer value `12` does not fit in `int32`"
    );
    assert_eq!(runtime_span(3, 4), Some(crate::diag::Span::new(3, 4)));
    assert_eq!(runtime_span(0, 4), None);
    assert_eq!(normalize_vec_index(3, 5), Some(3));
    assert_eq!(normalize_vec_index(-1, 5), Some(4));
    assert_eq!(normalize_vec_index(-6, 5), None);

    assert_eq!(value_type_name(&Value::Bool(true)), "bool");
    assert_eq!(value_type_name(&Value::Unit), "None");
    assert_eq!(
        value_type_name(&Value::ModuleNamespace(ModuleNamespaceValue {
            path: "pkg.tools".to_string(),
        })),
        "module pkg.tools"
    );
    assert_eq!(
        value_type_name(&Value::Instance(InstanceValue {
            class_name: "Point".to_string(),
            fields: Default::default(),
        })),
        "Point"
    );
    assert_eq!(
        value_type_name(&Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: Vec::new(),
        })),
        "Status"
    );
    assert_eq!(
        value_type_name(&Value::Int(IntegerValue::from_signed(1))),
        "integer"
    );
    assert_eq!(value_type_name(&Value::Float(1.5)), "float64");
    assert_eq!(
        value_type_name(&Value::Vec(VecValue {
            element_type: crate::sema::Type::named("int32"),
            elements: Vec::new(),
        })),
        "Vec"
    );
    assert_eq!(
        value_type_name(&Value::Set(SetValue {
            element_type: crate::sema::Type::named("String"),
            elements: Vec::new(),
        })),
        "Set"
    );
    assert_eq!(
        value_type_name(&Value::Map(MapValue {
            key_type: crate::sema::Type::named("String"),
            value_type: crate::sema::Type::named("int32"),
            entries: Vec::new(),
        })),
        "Map"
    );
    assert_eq!(value_type_name(&Value::Duration(5)), "Duration");
    assert_eq!(
        value_type_name(&Value::Range(RangeValue { start: 1, end: 2 })),
        "Range"
    );
    assert_eq!(
        value_type_name(&Value::Channel(ChannelValue::new())),
        "Queue"
    );
    assert_eq!(
        value_type_name(&Value::Task(TaskValue::from_handle(thread::spawn(|| Ok(
            Value::Unit
        ))))),
        "Task"
    );
    assert_eq!(
        value_type_name(&Value::TaskGroup(TaskGroupValue::new(
            &CancellationContext::default()
        ))),
        "TaskGroup"
    );
    assert_eq!(
        inferred_collection_type(&Value::Bool(true)),
        crate::sema::Type::named("bool")
    );
    assert_eq!(
        inferred_collection_type(&Value::Float(1.5)),
        crate::sema::Type::named("float64")
    );
    assert_eq!(
        inferred_collection_type(&Value::Int(IntegerValue::from_i32(7))),
        crate::sema::Type::named("int32")
    );
    assert_eq!(
        inferred_collection_type(&Value::Int(IntegerValue::from_signed(7))),
        crate::sema::Type::named("Unknown")
    );
    assert_eq!(
        inferred_collection_type(&Value::String("text".to_string())),
        crate::sema::Type::named("String")
    );
    assert_eq!(
        inferred_collection_type(&Value::Vec(VecValue {
            element_type: crate::sema::Type::named("int32"),
            elements: Vec::new(),
        })),
        crate::sema::Type::Named("Vec".to_string(), vec![crate::sema::Type::named("int32")])
    );
    assert_eq!(
        inferred_collection_type(&Value::Set(SetValue {
            element_type: crate::sema::Type::named("String"),
            elements: Vec::new(),
        })),
        crate::sema::Type::Named("Set".to_string(), vec![crate::sema::Type::named("String")])
    );
    assert_eq!(
        inferred_collection_type(&Value::Map(MapValue {
            key_type: crate::sema::Type::named("String"),
            value_type: crate::sema::Type::named("int32"),
            entries: Vec::new(),
        })),
        crate::sema::Type::Named(
            "Map".to_string(),
            vec![
                crate::sema::Type::named("String"),
                crate::sema::Type::named("int32"),
            ],
        )
    );
    assert_eq!(
        inferred_collection_type(&Value::Duration(5)),
        crate::sema::Type::named("Duration")
    );
    assert_eq!(
        inferred_collection_type(&Value::Range(RangeValue { start: 1, end: 2 })),
        crate::sema::Type::named("Range")
    );
    assert_eq!(
        inferred_collection_type(&Value::Instance(InstanceValue {
            class_name: "Point".to_string(),
            fields: Default::default(),
        })),
        crate::sema::Type::named("Point")
    );
    assert_eq!(
        inferred_collection_type(&Value::Channel(ChannelValue::new())),
        crate::sema::Type::named("Queue")
    );
    assert_eq!(
        inferred_collection_type(&Value::Task(TaskValue::from_handle(thread::spawn(|| Ok(
            Value::Unit
        ))))),
        crate::sema::Type::named("Task")
    );
    assert_eq!(
        inferred_collection_type(&Value::TaskGroup(TaskGroupValue::new(
            &CancellationContext::default()
        ))),
        crate::sema::Type::named("TaskGroup")
    );

    assert_eq!(
        compare_values(
            Value::Int(IntegerValue::from_signed(1)),
            Value::Int(IntegerValue::from_signed(2)),
            BinaryOp::Less,
        )
        .expect("int comparison should work"),
        Value::Bool(true)
    );
    assert_eq!(
        eval_binary_value(
            Value::Int(IntegerValue::from_signed(i128::MIN)),
            Value::Int(IntegerValue::from_literal(1)),
            BinaryOp::Sub,
        )
        .expect_err("subtraction beyond the signed integer range should fail")
        .message,
        "integer overflow"
    );
    assert_eq!(
        eval_binary_value(
            Value::Int(IntegerValue::from_literal(u128::MAX)),
            Value::Int(IntegerValue::from_literal(2)),
            BinaryOp::Mul,
        )
        .expect_err("unsigned multiplication beyond u128 should fail")
        .message,
        "integer overflow"
    );
    assert_eq!(
        eval_binary_value(Value::Float(1.0), Value::Float(0.0), BinaryOp::Mod)
            .expect_err("float modulo by zero should fail")
            .message,
        "division by zero"
    );
    assert_eq!(
        compare_values(
            Value::String("b".to_string()),
            Value::String("a".to_string()),
            BinaryOp::Greater,
        )
        .expect("string comparison should work"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(Value::Float(1.5), Value::Float(1.5), BinaryOp::LessEq)
            .expect("float comparison should work"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(
            Value::String("a".to_string()),
            Value::String("b".to_string()),
            BinaryOp::Less,
        )
        .expect("string less-than should work"),
        Value::Bool(true)
    );
    let compare_error = compare_values(
        Value::Bool(true),
        Value::String("x".to_string()),
        BinaryOp::Less,
    )
    .expect_err("unsupported comparison should fail");
    assert!(compare_error.message.contains("unsupported comparison"));
    assert_eq!(
        compare_values(
            Value::Vec(VecValue {
                element_type: crate::sema::Type::named("int32"),
                elements: vec![Value::Int(IntegerValue::from_signed(1))],
            }),
            Value::Vec(VecValue {
                element_type: crate::sema::Type::named("int32"),
                elements: vec![Value::Int(IntegerValue::from_signed(1))],
            }),
            BinaryOp::Eq,
        )
        .expect("equality should work for runtime values"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(Value::Bool(true), Value::Bool(false), BinaryOp::NotEq,)
            .expect("inequality should work for runtime values"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(
            Value::Int(IntegerValue::from_signed(2)),
            Value::Int(IntegerValue::from_signed(2)),
            BinaryOp::LessEq,
        )
        .expect("int less-equal should work"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(
            Value::Int(IntegerValue::from_signed(3)),
            Value::Int(IntegerValue::from_signed(2)),
            BinaryOp::Greater,
        )
        .expect("int greater-than should work"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(
            Value::Int(IntegerValue::from_signed(3)),
            Value::Int(IntegerValue::from_signed(3)),
            BinaryOp::GreaterEq,
        )
        .expect("int greater-equal should work"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(Value::Float(1.5), Value::Float(2.5), BinaryOp::Less,)
            .expect("float less-than should work"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(Value::Float(3.5), Value::Float(2.5), BinaryOp::Greater,)
            .expect("float greater-than should work"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(
            Value::String("a".to_string()),
            Value::String("a".to_string()),
            BinaryOp::LessEq,
        )
        .expect("string less-equal should work"),
        Value::Bool(true)
    );
    assert_eq!(
        compare_values(
            Value::String("b".to_string()),
            Value::String("a".to_string()),
            BinaryOp::GreaterEq,
        )
        .expect("string greater-equal should work"),
        Value::Bool(true)
    );
    let int_operator_error = compare_values(
        Value::Int(IntegerValue::from_signed(1)),
        Value::Int(IntegerValue::from_signed(2)),
        BinaryOp::Add,
    )
    .expect_err("unsupported int comparison operators should fail");
    assert!(int_operator_error
        .message
        .contains("unsupported comparison operator"));
    let float_operator_error = compare_values(Value::Float(1.0), Value::Float(2.0), BinaryOp::Add)
        .expect_err("unsupported float comparison operators should fail");
    assert!(float_operator_error
        .message
        .contains("unsupported comparison operator"));
    let string_operator_error = compare_values(
        Value::String("a".to_string()),
        Value::String("b".to_string()),
        BinaryOp::Add,
    )
    .expect_err("unsupported string comparison operators should fail");
    assert!(string_operator_error
        .message
        .contains("unsupported comparison operator"));

    assert_eq!(
        eval_binary_value(Value::Bool(true), Value::Bool(false), BinaryOp::And)
            .expect("logical and should work"),
        Value::Bool(false)
    );
    assert_eq!(
        eval_binary_value(Value::Bool(true), Value::Bool(false), BinaryOp::Or)
            .expect("logical or should work"),
        Value::Bool(true)
    );
    assert_eq!(
        eval_binary_value(Value::Bool(false), Value::Bool(false), BinaryOp::Or)
            .expect("logical or should preserve false"),
        Value::Bool(false)
    );
    assert_eq!(
        eval_binary_value(
            Value::Int(IntegerValue::from_signed(4)),
            Value::Int(IntegerValue::from_signed(5)),
            BinaryOp::Add,
        )
        .expect("int addition should work"),
        Value::Int(IntegerValue::from_signed(9))
    );
    assert_eq!(
        eval_binary_value(
            Value::Int(IntegerValue::from_signed(9)),
            Value::Int(IntegerValue::from_signed(4)),
            BinaryOp::Sub,
        )
        .expect("int subtraction should work"),
        Value::Int(IntegerValue::from_signed(5))
    );
    assert_eq!(
        eval_binary_value(
            Value::Int(IntegerValue::from_signed(3)),
            Value::Int(IntegerValue::from_signed(4)),
            BinaryOp::Mul,
        )
        .expect("int multiplication should work"),
        Value::Int(IntegerValue::from_signed(12))
    );
    assert_eq!(
        eval_binary_value(
            Value::Int(IntegerValue::from_signed(9)),
            Value::Int(IntegerValue::from_signed(3)),
            BinaryOp::Div,
        )
        .expect("int division should work"),
        Value::Int(IntegerValue::from_signed(3))
    );
    assert_eq!(
        eval_binary_value(
            Value::Int(IntegerValue::from_signed(9)),
            Value::Int(IntegerValue::from_signed(4)),
            BinaryOp::Mod,
        )
        .expect("int modulo should work"),
        Value::Int(IntegerValue::from_signed(1))
    );
    assert_eq!(
        eval_binary_value(Value::Float(9.0), Value::Float(2.0), BinaryOp::Div)
            .expect("float division should work"),
        Value::Float(4.5)
    );
    assert_eq!(
        eval_binary_value(Value::Float(9.0), Value::Float(2.0), BinaryOp::Add)
            .expect("float addition should work"),
        Value::Float(11.0)
    );
    assert_eq!(
        eval_binary_value(Value::Float(9.0), Value::Float(2.0), BinaryOp::Sub)
            .expect("float subtraction should work"),
        Value::Float(7.0)
    );
    assert_eq!(
        eval_binary_value(Value::Float(9.0), Value::Float(2.0), BinaryOp::Mul)
            .expect("float multiplication should work"),
        Value::Float(18.0)
    );
    assert_eq!(
        eval_binary_value(Value::Float(9.0), Value::Float(4.0), BinaryOp::Mod)
            .expect("float modulo should work"),
        Value::Float(1.0)
    );
    assert_eq!(
        eval_binary_value(
            Value::String("au".to_string()),
            Value::String("rora".to_string()),
            BinaryOp::Add,
        )
        .expect("string concatenation should work"),
        Value::String("aurora".to_string())
    );
    let add_error = eval_binary_value(
        Value::String("a".to_string()),
        Value::Bool(true),
        BinaryOp::Add,
    )
    .expect_err("unsupported add should fail");
    assert!(add_error.message.contains("unsupported `+` operands"));
    let and_error = eval_binary_value(
        Value::Bool(true),
        Value::Int(IntegerValue::from_signed(1)),
        BinaryOp::And,
    )
    .expect_err("logical and should reject non-bools");
    assert!(and_error
        .message
        .contains("logical `and` expects bool operands"));
    let or_error = eval_binary_value(
        Value::Bool(true),
        Value::Int(IntegerValue::from_signed(1)),
        BinaryOp::Or,
    )
    .expect_err("logical or should reject non-bools");
    assert!(or_error
        .message
        .contains("logical `or` expects bool operands"));
    let div_zero = eval_binary_value(
        Value::Int(IntegerValue::from_signed(1)),
        Value::Int(IntegerValue::zero()),
        BinaryOp::Div,
    )
    .expect_err("division by zero should fail");
    assert_eq!(div_zero.message, "division by zero");
    let mod_zero = eval_binary_value(
        Value::Int(IntegerValue::from_signed(1)),
        Value::Int(IntegerValue::zero()),
        BinaryOp::Mod,
    )
    .expect_err("modulo by zero should fail");
    assert_eq!(mod_zero.message, "division by zero");
    let sub_error = eval_binary_value(
        Value::Bool(true),
        Value::Int(IntegerValue::from_signed(1)),
        BinaryOp::Sub,
    )
    .expect_err("invalid subtraction should fail");
    assert!(sub_error.message.contains("unsupported `-` operands"));
    let mul_error = eval_binary_value(
        Value::Bool(true),
        Value::Int(IntegerValue::from_signed(1)),
        BinaryOp::Mul,
    )
    .expect_err("invalid multiplication should fail");
    assert!(mul_error.message.contains("unsupported `*` operands"));
    let div_error = eval_binary_value(
        Value::Bool(true),
        Value::Int(IntegerValue::from_signed(1)),
        BinaryOp::Div,
    )
    .expect_err("invalid division should fail");
    assert!(div_error.message.contains("unsupported `/` operands"));
    let mod_error = eval_binary_value(
        Value::Bool(true),
        Value::Int(IntegerValue::from_signed(1)),
        BinaryOp::Mod,
    )
    .expect_err("invalid modulo should fail");
    assert!(mod_error.message.contains("unsupported `%` operands"));

    assert_eq!(
        eval_unary_value(Value::Bool(false), UnaryOp::Not).expect("not should work"),
        Value::Bool(true)
    );
    assert_eq!(
        eval_unary_value(Value::Int(IntegerValue::from_signed(2)), UnaryOp::Neg)
            .expect("integer negation should work"),
        Value::Int(IntegerValue::from_signed(-2))
    );
    assert_eq!(
        eval_unary_value(Value::Float(2.5), UnaryOp::Neg).expect("neg should work"),
        Value::Float(-2.5)
    );
    let not_error = eval_unary_value(Value::Int(IntegerValue::from_signed(1)), UnaryOp::Not)
        .expect_err("invalid unary not should fail");
    assert!(not_error.message.contains("expects `bool`"));
    let unary_error = eval_unary_value(Value::String("x".to_string()), UnaryOp::Neg)
        .expect_err("invalid unary neg should fail");
    assert!(unary_error.message.contains("expects a numeric value"));

    let module_value = super::boxed_value(Value::ModuleNamespace(ModuleNamespaceValue {
        path: "pkg.tools".to_string(),
    }));
    let instance_value = super::boxed_value(Value::Instance(InstanceValue {
        class_name: "Point".to_string(),
        fields: Default::default(),
    }));
    let enum_value = super::boxed_value(Value::EnumVariant(EnumVariantValue {
        enum_name: "Status".to_string(),
        variant_name: "Ready".to_string(),
        payloads: Vec::new(),
    }));
    let unit_value = super::boxed_value(Value::Unit);

    let int64_value = int_value(7);
    assert_eq!(
        super::aurora_direct_value_type_matches(int64_value, b"int64".as_ptr(), "int64".len(),),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(int64_value, b"int32".as_ptr(), "int32".len(),),
        0
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(int64_value, b"uint64".as_ptr(), "uint64".len(),),
        0
    );
    let int32_value = super::aurora_direct_box_i32(7);
    assert_eq!(
        super::aurora_direct_value_type_matches(int32_value, b"int32".as_ptr(), "int32".len(),),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(int32_value, b"int64".as_ptr(), "int64".len(),),
        0
    );
    let uint64_value = super::aurora_direct_box_u64(7);
    assert_eq!(
        super::aurora_direct_value_type_matches(uint64_value, b"uint64".as_ptr(), "uint64".len(),),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(uint64_value, b"int64".as_ptr(), "int64".len(),),
        0
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            float_value(3.5),
            b"float32".as_ptr(),
            "float32".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(unit_value, b"None".as_ptr(), "None".len(),),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(
            module_value,
            b"module pkg.tools".as_ptr(),
            "module pkg.tools".len(),
        ),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(instance_value, b"Point".as_ptr(), "Point".len(),),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(enum_value, b"Status".as_ptr(), "Status".len(),),
        1
    );
}

#[test]
fn native_runtime_resource_metadata_reports_maintained_type_names() {
    let mut file_path = std::env::temp_dir();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    file_path.push(format!(
        "aurora-native-resource-metadata-{}-{timestamp}.txt",
        std::process::id()
    ));
    let file = FileValue::create(file_path.to_str().expect("temp path should be valid UTF-8"))
        .expect("temp file should be created");
    assert_value_metadata(&Value::File(file.clone()), "fs.File", "fs.File");
    assert_direct_type_match(Value::File(file.clone()), "fs.File");
    close_via_direct(Value::File(file.clone()));
    let _ = std::fs::remove_file(&file_path);

    let tcp_listener =
        TcpListenerValue::bind("127.0.0.1:0").expect("tcp listener should bind locally");
    let tcp_address = tcp_listener
        .local_addr()
        .expect("tcp listener should expose a local address");
    let accept_listener = tcp_listener.clone();
    let accept_thread = thread::spawn(move || {
        accept_listener
            .accept(Some(StdDuration::from_secs(1)), None)
            .expect("tcp listener should accept local client")
    });
    let tcp_stream = TcpStreamValue::connect(&tcp_address, Some(StdDuration::from_secs(1)), None)
        .expect("tcp stream should connect locally");
    let accepted_stream = accept_thread
        .join()
        .expect("tcp accept worker should join successfully");
    assert_value_metadata(
        &Value::TcpListener(tcp_listener.clone()),
        "net.TcpListener",
        "net.TcpListener",
    );
    assert_direct_type_match(Value::TcpListener(tcp_listener.clone()), "net.TcpListener");
    assert_value_metadata(
        &Value::TcpStream(tcp_stream.clone()),
        "net.TcpStream",
        "net.TcpStream",
    );
    assert_direct_type_match(Value::TcpStream(tcp_stream.clone()), "net.TcpStream");
    close_via_direct(Value::TcpStream(tcp_stream.clone()));
    close_via_direct(Value::TcpStream(accepted_stream.clone()));
    close_via_direct(Value::TcpListener(tcp_listener.clone()));

    let udp_socket = UdpSocketValue::bind("127.0.0.1:0").expect("udp socket should bind locally");
    assert_value_metadata(
        &Value::UdpSocket(udp_socket.clone()),
        "net.UdpSocket",
        "net.UdpSocket",
    );
    assert_direct_type_match(Value::UdpSocket(udp_socket.clone()), "net.UdpSocket");
    close_via_direct(Value::UdpSocket(udp_socket.clone()));
    let udp_datagram = UdpDatagramValue {
        address: "127.0.0.1:9".to_string(),
        data: vec![1, 2, 3],
    };
    assert_value_metadata(
        &Value::UdpDatagram(udp_datagram.clone()),
        "net.UdpDatagram",
        "net.UdpDatagram",
    );
    assert_direct_type_match(Value::UdpDatagram(udp_datagram), "net.UdpDatagram");

    #[cfg(unix)]
    {
        let mut socket_path = std::path::PathBuf::from("/tmp");
        socket_path.push(format!(
            "aura-nrm-{}-{}.sock",
            std::process::id(),
            timestamp % 1_000_000
        ));
        let _ = std::fs::remove_file(&socket_path);
        let unix_listener = UnixListenerValue::bind(
            socket_path
                .to_str()
                .expect("unix socket path should be valid UTF-8"),
        )
        .expect("unix listener should bind locally");
        assert_value_metadata(
            &Value::UnixListener(unix_listener.clone()),
            "net.UnixListener",
            "net.UnixListener",
        );
        assert_direct_type_match(
            Value::UnixListener(unix_listener.clone()),
            "net.UnixListener",
        );
        let accept_listener = unix_listener.clone();
        let unix_accept_thread = thread::spawn(move || {
            accept_listener
                .accept(Some(StdDuration::from_secs(1)), None)
                .expect("unix listener should accept local client")
        });
        let unix_stream = UnixStreamValue::connect(
            socket_path
                .to_str()
                .expect("unix socket path should be valid UTF-8"),
            Some(StdDuration::from_secs(1)),
            None,
        )
        .expect("unix stream should connect locally");
        let accepted_unix_stream = unix_accept_thread
            .join()
            .expect("unix accept worker should join successfully");
        assert_value_metadata(
            &Value::UnixStream(unix_stream.clone()),
            "net.UnixStream",
            "net.UnixStream",
        );
        assert_direct_type_match(Value::UnixStream(unix_stream.clone()), "net.UnixStream");
        assert_direct_type_match(
            Value::UnixStream(accepted_unix_stream.clone()),
            "net.UnixStream",
        );
        close_via_direct(Value::UnixStream(unix_stream.clone()));
        close_via_direct(Value::UnixStream(accepted_unix_stream.clone()));
        close_via_direct(Value::UnixListener(unix_listener.clone()));
        let _ = std::fs::remove_file(&socket_path);
    }

    let certificate =
        generate_simple_self_signed(vec!["localhost".to_string()]).expect("cert generation");
    let cert_path = std::env::temp_dir().join(format!(
        "aurora-native-resource-metadata-{}-{timestamp}.cert.pem",
        std::process::id()
    ));
    let key_path = std::env::temp_dir().join(format!(
        "aurora-native-resource-metadata-{}-{timestamp}.key.pem",
        std::process::id()
    ));
    std::fs::write(&cert_path, certificate.cert.pem().as_bytes()).expect("write cert pem");
    std::fs::write(&key_path, certificate.key_pair.serialize_pem().as_bytes())
        .expect("write key pem");
    let tls_listener = TlsListenerValue::bind(
        "127.0.0.1:0",
        cert_path.to_str().expect("cert path should be valid UTF-8"),
        key_path.to_str().expect("key path should be valid UTF-8"),
    )
    .expect("tls listener should bind locally");
    let tls_address = tls_listener
        .local_addr()
        .expect("tls listener should expose a local address");
    assert_value_metadata(
        &Value::TlsListener(tls_listener.clone()),
        "net.TlsListener",
        "net.TlsListener",
    );
    assert_direct_type_match(Value::TlsListener(tls_listener.clone()), "net.TlsListener");
    let accept_listener = tls_listener.clone();
    let tls_accept_thread = thread::spawn(move || {
        accept_listener
            .accept(Some(StdDuration::from_secs(1)), None)
            .expect("tls listener should accept local client")
    });
    let tls_stream = TlsStreamValue::connect(
        &tls_address,
        "localhost",
        Some(cert_path.to_str().expect("cert path should be valid UTF-8")),
        Some(StdDuration::from_secs(1)),
        None,
    )
    .expect("tls stream should connect locally");
    let accepted_tls_stream = tls_accept_thread
        .join()
        .expect("tls accept worker should join successfully");
    assert_value_metadata(
        &Value::TlsStream(tls_stream.clone()),
        "net.TlsStream",
        "net.TlsStream",
    );
    assert_direct_type_match(Value::TlsStream(tls_stream.clone()), "net.TlsStream");
    assert_direct_type_match(
        Value::TlsStream(accepted_tls_stream.clone()),
        "net.TlsStream",
    );
    let tls_client_ptr = boxed_value(Value::TlsStream(tls_stream.clone()));
    let tls_server_ptr = boxed_value(Value::TlsStream(accepted_tls_stream.clone()));
    expect_result_ok_unit(super::aurora_direct_tls_stream_write_all(
        tls_client_ptr,
        string_value("hello tls\n"),
        duration_value(5_000),
    ));
    let tls_line = expect_option_some_payload(expect_result_ok_payload(
        super::aurora_direct_tls_stream_read_line(tls_server_ptr, duration_value(5_000)),
    ));
    assert_eq!(tls_line, Value::String("hello tls".to_string()));
    expect_unit(super::aurora_direct_tls_stream_close(tls_client_ptr));
    expect_unit(super::aurora_direct_tls_stream_close(tls_server_ptr));
    unsafe {
        release_value(tls_client_ptr);
        release_value(tls_server_ptr);
    }
    close_via_direct(Value::TlsListener(tls_listener.clone()));
    let _ = std::fs::remove_file(&cert_path);
    let _ = std::fs::remove_file(&key_path);

    let http_listener =
        HttpListenerValue::bind("127.0.0.1:0").expect("http listener should bind locally");
    let http_listener_address = http_listener
        .local_addr()
        .expect("http listener should expose a local address");
    assert_value_metadata(
        &Value::HttpListener(http_listener.clone()),
        "net.HttpListener",
        "net.HttpListener",
    );
    assert_direct_type_match(
        Value::HttpListener(http_listener.clone()),
        "net.HttpListener",
    );
    let accept_listener = http_listener.clone();
    let http_exchange_thread = thread::spawn(move || {
        let exchange = accept_listener
            .accept(Some(StdDuration::from_secs(1)), None)
            .expect("http listener should accept local client");
        assert_value_metadata(
            &Value::HttpExchange(exchange.clone()),
            "net.HttpExchange",
            "net.HttpExchange",
        );
        assert_direct_type_match(Value::HttpExchange(exchange.clone()), "net.HttpExchange");
        exchange
            .respond_text(204, "", Vec::new())
            .expect("http exchange should respond");
    });
    let mut http_client = std::net::TcpStream::connect(&http_listener_address)
        .expect("http metadata client should connect");
    http_client
        .write_all(b"GET /metadata HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n")
        .expect("http metadata client should write request");
    let mut http_response_bytes = Vec::new();
    http_client
        .read_to_end(&mut http_response_bytes)
        .expect("http metadata client should read response");
    assert!(
        http_response_bytes.starts_with(b"HTTP/1.1 204"),
        "http metadata response should report 204"
    );
    http_exchange_thread
        .join()
        .expect("http exchange worker should join successfully");
    close_via_direct(Value::HttpListener(http_listener.clone()));

    let http_server =
        std::net::TcpListener::bind("127.0.0.1:0").expect("http fixture should bind locally");
    let http_address = http_server
        .local_addr()
        .expect("http fixture should expose a local address");
    let http_thread = thread::spawn(move || {
        let (mut stream, _) = http_server
            .accept()
            .expect("http fixture should accept one request");
        let mut request = [0_u8; 512];
        let _ = stream
            .read(&mut request)
            .expect("http fixture should read request bytes");
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .expect("http fixture should write response");
    });
    let http_response = HttpResponseValue::request_text(
        "GET",
        &format!("http://{http_address}/"),
        "",
        Vec::new(),
        Some(StdDuration::from_secs(1)),
        None,
    )
    .expect("http response should be read from local fixture");
    http_thread
        .join()
        .expect("http fixture worker should join successfully");
    assert_value_metadata(
        &Value::HttpResponse(http_response.clone()),
        "net.HttpResponse",
        "net.HttpResponse",
    );
    assert_direct_type_match(Value::HttpResponse(http_response), "net.HttpResponse");

    let ws_listener =
        WebSocketListenerValue::bind("127.0.0.1:0").expect("websocket listener should bind");
    let ws_address = ws_listener
        .local_addr()
        .expect("websocket listener should expose a local address");
    assert_value_metadata(
        &Value::WebSocketListener(ws_listener.clone()),
        "net.WebSocketListener",
        "net.WebSocketListener",
    );
    assert_direct_type_match(
        Value::WebSocketListener(ws_listener.clone()),
        "net.WebSocketListener",
    );
    let accept_listener = ws_listener.clone();
    let ws_accept_thread = thread::spawn(move || {
        accept_listener
            .accept(Some(StdDuration::from_secs(1)))
            .expect("websocket listener should accept local client")
    });
    let ws_client = WebSocketValue::connect(
        &format!("ws://{ws_address}"),
        Some(StdDuration::from_secs(1)),
    )
    .expect("websocket client should connect locally");
    let ws_server = ws_accept_thread
        .join()
        .expect("websocket accept worker should join successfully");
    assert_value_metadata(
        &Value::WebSocket(ws_client.clone()),
        "net.WebSocket",
        "net.WebSocket",
    );
    assert_direct_type_match(Value::WebSocket(ws_client.clone()), "net.WebSocket");
    assert_direct_type_match(Value::WebSocket(ws_server.clone()), "net.WebSocket");
    close_via_direct(Value::WebSocket(ws_client.clone()));
    close_via_direct(Value::WebSocket(ws_server.clone()));

    let child = ProcessChildValue::spawn(
        vec![
            std::env::current_exe()
                .expect("current test binary should be available")
                .to_string_lossy()
                .into_owned(),
            "--help".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("process child should spawn");
    let stdout_pipe = child.stdout().expect("stdout pipe should be captured");
    assert_value_metadata(
        &Value::ProcessChild(child.clone()),
        "process.Child",
        "process.Child",
    );
    assert_direct_type_match(Value::ProcessChild(child.clone()), "process.Child");
    assert_value_metadata(
        &Value::ProcessPipe(stdout_pipe.clone()),
        "process.Pipe",
        "process.Pipe",
    );
    assert_direct_type_match(Value::ProcessPipe(stdout_pipe.clone()), "process.Pipe");
    close_via_direct(Value::ProcessPipe(stdout_pipe));
    let _ = child.wait(Some(StdDuration::from_secs(1)), None);
    close_via_direct(Value::ProcessChild(child.clone()));

    let completed = ProcessCompletedValue::new(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "process.ExitStatus".to_string(),
            variant_name: "Exited".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(0))],
        }),
        Vec::new(),
        Vec::new(),
    );
    assert_value_metadata(
        &Value::ProcessCompleted(completed.clone()),
        "process.Completed",
        "process.Completed",
    );
    assert_direct_type_match(Value::ProcessCompleted(completed), "process.Completed");
    let supervisor = ProcessSupervisorValue::new();
    assert_value_metadata(
        &Value::ProcessSupervisor(supervisor.clone()),
        "process.Supervisor",
        "process.Supervisor",
    );
    assert_direct_type_match(
        Value::ProcessSupervisor(supervisor.clone()),
        "process.Supervisor",
    );
    close_via_direct(Value::ProcessSupervisor(supervisor));
}

#[test]
fn native_runtime_thread_local_and_pointer_helpers_cover_remaining_paths() {
    assert!(!current_cancellation().is_cancelled());
    let group = TaskGroupValue::new(&crate::runtime_value::CancellationContext::default());
    let child = group.child_cancellation();
    group.cancel();
    let scoped = with_cancellation_scope(child, || current_cancellation().is_cancelled());
    assert!(scoped);
    assert!(!current_cancellation().is_cancelled());

    assert!(super::take_direct_cleanup_registration(42).is_none());
    super::with_direct_task_runtime_state(|state| {
        state.cleanup_draining = true;
    });
    super::drain_direct_cleanup_stack();
    assert!(super::direct_cleanup_is_draining());
    super::with_direct_task_runtime_state(|state| state.cleanup_draining = false);

    super::with_direct_task_runtime_state(|state| state.next_cleanup_id = i64::MAX);
    let max_registration_id = super::push_direct_cleanup_registration(11, std::ptr::null_mut(), 0);
    assert_eq!(max_registration_id, i64::MAX);
    assert!(super::take_direct_cleanup_registration(max_registration_id).is_some());
    super::with_direct_task_runtime_state(|state| {
        assert_eq!(state.next_cleanup_id, 1);
        state.next_cleanup_id = -1;
    });
    let negative_registration_id =
        super::push_direct_cleanup_registration(12, std::ptr::null_mut(), 0);
    assert_eq!(negative_registration_id, -1);
    assert!(super::take_direct_cleanup_registration(negative_registration_id).is_some());
    super::with_direct_task_runtime_state(|state| {
        assert_eq!(state.next_cleanup_id, 1);
        state.next_cleanup_id = 1;
    });
    let cleanup_id = super::aurora_direct_register_cleanup(1, std::ptr::null_mut(), 0);
    assert!(cleanup_id > 0);
    super::aurora_direct_unregister_cleanup(cleanup_id);
    super::aurora_direct_unregister_cleanup(cleanup_id);

    let inactive_cleanup_id = super::aurora_direct_register_cleanup(1, std::ptr::null_mut(), 0);
    assert_eq!(
        super::aurora_direct_refresh_cleanup(0, inactive_cleanup_id, 1, std::ptr::null_mut(), 0),
        0
    );

    let replaced_cleanup_id = super::aurora_direct_register_cleanup(1, std::ptr::null_mut(), 0);
    let refreshed_cleanup_id =
        super::aurora_direct_refresh_cleanup(1, replaced_cleanup_id, 1, std::ptr::null_mut(), 0);
    assert!(refreshed_cleanup_id > 0);
    super::aurora_direct_unregister_cleanup(refreshed_cleanup_id);

    let new_cleanup_id = super::aurora_direct_refresh_cleanup(1, 0, 1, std::ptr::null_mut(), 0);
    assert!(new_cleanup_id > 0);
    super::aurora_direct_unregister_cleanup(new_cleanup_id);

    let primary = super::DirectPrimaryDiagnosticGuard::install(Diagnostic::new("primary"));
    let nested = super::DirectPrimaryDiagnosticGuard::install(Diagnostic::new("nested"));
    assert!(primary.installed);
    assert!(!nested.installed);
    assert_eq!(
        super::direct_primary_runtime_diagnostic()
            .expect("primary diagnostic should be installed")
            .message,
        "primary"
    );
    drop(nested);
    assert!(super::direct_primary_runtime_diagnostic().is_some());
    drop(primary);
    assert!(super::direct_primary_runtime_diagnostic().is_none());

    assert_eq!(
        extract_duration_millis(&Value::Int(IntegerValue::from_signed(7))),
        7
    );
    assert_eq!(extract_duration_millis(&Value::Duration(9)), 9);
    assert_eq!(decode_bytes(b"aurora".as_ptr(), "aurora".len()), "aurora");

    unsafe {
        super::aurora_direct_enter_call(0, 0, b"covered".as_ptr(), b"covered".len());
        super::aurora_direct_enter_call(0, 0, b"covered".as_ptr(), b"covered".len());
        super::aurora_direct_exit_call();
        super::aurora_direct_exit_call();
        super::aurora_direct_exit_call();
    }

    let boxed = boxed_value(Value::Int(IntegerValue::from_signed(5)));
    assert_eq!(
        unsafe { value_ref(boxed) },
        Value::Int(IntegerValue::from_signed(5))
    );
    unsafe {
        value_mut(boxed, |value| match value {
            Value::Int(inner) => *inner = IntegerValue::from_signed(8),
            other => panic!("expected int box, found {:?}", other),
        })
    };
    assert_eq!(expect_int(boxed), 8);

    let vec = super::aurora_direct_vec_empty();
    expect_unit(super::aurora_direct_vec_push_in_place(
        vec,
        string_value("x"),
    ));
    assert_eq!(super::with_vector(vec, |vector| vector.elements.len()), 1);
    super::with_vector_mut(vec, |vector| {
        vector.elements.push(Value::String("y".to_string()));
    });
    assert_eq!(
        expect_vec_strings(super::aurora_direct_clone_value(vec)),
        vec!["x".to_string(), "y".to_string()]
    );

    let map = super::aurora_direct_map_empty();
    expect_option_none(super::aurora_direct_map_set_in_place(
        map,
        string_value("name"),
        int_value(1),
    ));
    assert_eq!(super::with_map(map, |map| map.entries.len()), 1);
    super::with_map_mut(map, |map| {
        map.entries.push((
            Value::String("other".to_string()),
            Value::Int(IntegerValue::from_signed(2)),
        ));
    });
    assert_eq!(
        expect_vec_ints(super::aurora_direct_map_values(map)),
        vec![1, 2]
    );

    let set = super::aurora_direct_set_empty();
    assert_eq!(
        super::aurora_direct_set_insert_in_place(set, string_value("ready")),
        1
    );
    assert_eq!(super::with_set(set, |set| set.elements.len()), 1);
    super::with_set_mut(set, |set| {
        set.elements.push(Value::String("go".to_string()));
    });
    assert_eq!(super::aurora_direct_set_len(set), 2);

    assert_eq!(runtime_span(0, 1), None);
    assert_eq!(runtime_span(1, 0), None);
    assert_eq!(runtime_span(2, 3), Some(crate::diag::Span::new(2, 3)));

    assert_eq!(value_type_name(&Value::Unit), "None");
    assert_eq!(
        value_type_name(&Value::ModuleNamespace(ModuleNamespaceValue {
            path: "pkg.tools".to_string(),
        })),
        "module pkg.tools"
    );
    assert_eq!(
        value_type_name(&Value::Instance(InstanceValue {
            class_name: "Counter".to_string(),
            fields: BTreeMap::new(),
        })),
        "Counter"
    );
    assert_eq!(
        value_type_name(&Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: Vec::new(),
        })),
        "Status"
    );
    assert_eq!(
        value_type_name(&Value::Channel(ChannelValue::new())),
        "Queue"
    );
    assert_eq!(
        value_type_name(&Value::Task(TaskValue::from_handle(thread::spawn(|| Ok(
            Value::Unit
        ))))),
        "Task"
    );
    assert_eq!(
        value_type_name(&Value::TaskGroup(TaskGroupValue::new(
            &CancellationContext::default()
        ))),
        "TaskGroup"
    );

    let plain = render_runtime_diagnostic(Diagnostic::new("plain failure"));
    assert!(plain.contains("error: plain failure"));

    let rendered = render_runtime_diagnostic(Diagnostic::at(Span::new(1, 1), "annotated"));
    assert!(rendered.contains("error: annotated"));
}

#[test]
fn native_runtime_task_boundary_maps_task_signals_and_resumes_unrelated_panics() {
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let plain_panic = panic::catch_unwind(|| {
        super::task_runtime_boundary(|| {
            panic!("plain boundary panic");
        });
    });
    panic::set_hook(previous_hook);
    assert!(plain_panic.is_err());

    let result = run_lightweight_root_task(|| {
        let cancelled_task = spawn_lightweight_task(|| {
            super::task_runtime_boundary(|| -> std::result::Result<Value, Diagnostic> {
                std::panic::panic_any(TaskCancelledSignal);
            })
        })?;
        match cancelled_task
            .wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None)
        {
            TaskWaitStatus::Cancelled => {}
            other => panic!("expected cancelled direct-runtime task, got {other:?}"),
        }

        let failed_task = spawn_lightweight_task(|| {
            super::task_runtime_boundary(|| -> std::result::Result<Value, Diagnostic> {
                std::panic::panic_any(LightweightTaskFailureSignal(Diagnostic::new(
                    "boundary failure",
                )));
            })
        })?;
        match failed_task
            .wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None)
        {
            TaskWaitStatus::Ready(Err(error)) => assert_eq!(error.message, "boundary failure"),
            other => panic!("expected failed direct-runtime task, got {other:?}"),
        }

        Ok(Value::Unit)
    });

    assert_eq!(result.expect("root task should complete"), Value::Unit);
}

#[test]
fn native_runtime_direct_call_depth_is_isolated_across_suspended_tasks() {
    const TASK_COUNT: usize = 1_000;

    let result = run_lightweight_root_task(|| {
        let ready = ChannelValue::new();
        let release = ChannelValue::new();
        let mut tasks = Vec::with_capacity(TASK_COUNT);

        for _ in 0..TASK_COUNT {
            let task_ready = ready.clone();
            let task_release = release.clone();
            tasks.push(spawn_lightweight_task(move || {
                super::with_direct_task_runtime_scope(|| {
                    Ok(super::with_task_runtime_error_capture(|| {
                        task_ready
                            .send(Value::Unit)
                            .expect("ready channel should remain open");
                        unsafe {
                            super::aurora_direct_enter_call(
                                1,
                                1,
                                b"suspended".as_ptr(),
                                b"suspended".len(),
                            );
                        }
                        let _ = task_release.recv_with_cancellation(None, None);
                        unsafe {
                            super::aurora_direct_exit_call();
                        }
                        Value::Unit
                    }))
                })
            })?);
        }

        for _ in 0..TASK_COUNT {
            ready
                .recv_with_cancellation(Some(StdDuration::from_secs(10)), None)
                .ok_or_else(|| Diagnostic::new("timed out waiting for suspended direct tasks"))?;
        }
        release.close();

        for (index, task) in tasks.iter().enumerate() {
            match task
                .wait_result_with_cancellation_observed(Some(StdDuration::from_secs(10)), None)
            {
                TaskWaitStatus::Ready(Ok(Value::Unit)) => {}
                other => {
                    return Err(Diagnostic::new(format!(
                        "suspended direct task {index} did not finish cleanly: {other:?}"
                    )))
                }
            }
        }

        Ok(Value::Unit)
    });

    assert_eq!(
        result.expect("1,000 suspended direct tasks should have independent call depth"),
        Value::Unit
    );
}

#[test]
fn native_runtime_error_capture_is_isolated_across_suspended_tasks() {
    let result = run_lightweight_root_task(|| {
        let ready = ChannelValue::new();
        let release_first = ChannelValue::new();
        let release_second = ChannelValue::new();

        let first_ready = ready.clone();
        let first_release = release_first.clone();
        let first = spawn_lightweight_task(move || {
            super::with_direct_task_runtime_scope(|| {
                Ok(super::with_task_runtime_error_capture(|| {
                    first_ready
                        .send(Value::Unit)
                        .expect("ready channel should remain open");
                    let _ = first_release.recv_with_cancellation(None, None);
                    Value::Unit
                }))
            })
        })?;

        let second_ready = ready.clone();
        let second_release = release_second.clone();
        let second = spawn_lightweight_task(move || {
            super::with_direct_task_runtime_scope(|| {
                let capture_was_preserved = super::with_task_runtime_error_capture(|| {
                    second_ready
                        .send(Value::Unit)
                        .expect("ready channel should remain open");
                    let _ = second_release.recv_with_cancellation(None, None);
                    super::direct_runtime_error_capture_enabled()
                });
                if capture_was_preserved {
                    Ok(Value::Unit)
                } else {
                    Err(Diagnostic::new(
                        "another suspended task cleared direct runtime error capture",
                    ))
                }
            })
        })?;

        for _ in 0..2 {
            ready
                .recv_with_cancellation(Some(StdDuration::from_secs(1)), None)
                .ok_or_else(|| Diagnostic::new("timed out waiting for capture test tasks"))?;
        }

        release_first.close();
        match first.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None) {
            TaskWaitStatus::Ready(Ok(Value::Unit)) => {}
            other => {
                return Err(Diagnostic::new(format!(
                    "first capture test task did not finish cleanly: {other:?}"
                )))
            }
        }

        release_second.close();
        match second.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None) {
            TaskWaitStatus::Ready(Ok(Value::Unit)) => Ok(Value::Unit),
            other => Err(Diagnostic::new(format!(
                "second capture test task did not preserve its state: {other:?}"
            ))),
        }
    });

    assert_eq!(
        result.expect("direct runtime error capture should be task-local"),
        Value::Unit
    );
}

#[test]
fn native_runtime_cleanup_diagnostic_state_is_isolated_across_tasks() {
    let result = run_lightweight_root_task(|| {
        let ready = ChannelValue::new();
        let release = ChannelValue::new();

        let first_ready = ready.clone();
        let first_release = release.clone();
        let first = spawn_lightweight_task(move || {
            super::with_direct_task_runtime_scope(|| {
                let _primary = super::DirectPrimaryDiagnosticGuard::install(Diagnostic::new(
                    "first task primary",
                ));
                let key = super::direct_task_runtime_key();
                super::with_direct_task_runtime_state(|state| state.cleanup_draining = true);
                let _draining = super::DirectCleanupDrainGuard { key };
                first_ready
                    .send(Value::Unit)
                    .expect("ready channel should remain open");
                let _ = first_release.recv_with_cancellation(None, None);
                Ok(Value::Unit)
            })
        })?;

        ready
            .recv_with_cancellation(Some(StdDuration::from_secs(1)), None)
            .ok_or_else(|| Diagnostic::new("timed out waiting for cleanup-state task"))?;

        let second = spawn_lightweight_task(|| {
            super::with_direct_task_runtime_scope(|| {
                let draining = super::direct_cleanup_is_draining();
                let primary = super::direct_primary_runtime_diagnostic();
                if draining || primary.is_some() {
                    return Err(Diagnostic::new(
                        "another suspended task leaked direct cleanup diagnostic state",
                    ));
                }
                Ok(Value::Unit)
            })
        })?;

        let second_status =
            second.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None);
        release.close();
        let first_status =
            first.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None);

        match (first_status, second_status) {
            (TaskWaitStatus::Ready(Ok(Value::Unit)), TaskWaitStatus::Ready(Ok(Value::Unit))) => {
                Ok(Value::Unit)
            }
            other => Err(Diagnostic::new(format!(
                "cleanup diagnostic isolation failed: {other:?}"
            ))),
        }
    });

    assert_eq!(
        result.expect("direct cleanup diagnostic state should be task-local"),
        Value::Unit
    );
}

#[test]
fn native_runtime_task_exit_unwinds_live_drop_values() {
    struct DropProbe(Arc<AtomicBool>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let dropped = Arc::new(AtomicBool::new(false));
    let task_dropped = dropped.clone();
    let result = run_lightweight_root_task(move || {
        let task = spawn_lightweight_task(move || {
            let _probe = DropProbe(task_dropped);
            super::task_runtime_boundary(|| {
                std::panic::panic_any(TaskCancelledSignal);
            });
            #[allow(unreachable_code)]
            Ok(Value::Unit)
        })?;

        match task.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None) {
            TaskWaitStatus::Cancelled => Ok(Value::Unit),
            other => Err(Diagnostic::new(format!(
                "expected cancelled task, got {other:?}"
            ))),
        }
    });

    assert_eq!(result.expect("root task should complete"), Value::Unit);
    assert!(
        dropped.load(Ordering::SeqCst),
        "task exit must unwind live Rust values before reclaiming its coroutine stack"
    );
}

#[test]
fn native_runtime_direct_forced_exit_runs_external_cleanup() {
    struct DropProbe(Arc<AtomicBool>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let cleaned = Arc::new(AtomicBool::new(false));
    let cleanup_probe = DropProbe(cleaned.clone());
    let result = run_lightweight_root_task(move || {
        let task = unsafe {
            crate::runtime_value::spawn_lightweight_task_with_cancellation_and_forced_exit_cleanup(
                CancellationContext::default(),
                || {
                    super::with_direct_task_runtime_scope(|| {
                        super::with_task_runtime_error_capture(|| {
                            super::task_runtime_boundary(|| {
                                std::panic::panic_any(LightweightTaskFailureSignal(
                                    Diagnostic::new("direct task failure"),
                                ));
                            });
                            #[allow(unreachable_code)]
                            Ok(Value::Unit)
                        })
                    })
                },
                move || {
                    super::discard_current_direct_task_runtime_state();
                    drop(cleanup_probe);
                },
            )?
        };

        let status =
            task.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None);
        let stale_child_state =
            super::DIRECT_TASK_RUNTIME_STATES.with(|states| states.borrow().contains_key(&2));
        if stale_child_state {
            return Err(Diagnostic::new(
                "direct forced exit left task-local runtime state behind",
            ));
        }
        match status {
            TaskWaitStatus::Ready(Err(error)) if error.message == "direct task failure" => {
                Ok(Value::Unit)
            }
            other => Err(Diagnostic::new(format!(
                "expected failed direct task, got {other:?}"
            ))),
        }
    });

    assert_eq!(result.expect("root task should complete"), Value::Unit);
    assert!(
        cleaned.load(Ordering::SeqCst),
        "direct forced exit must run its scheduler-owned external cleanup"
    );
}

#[test]
fn native_runtime_releases_cleanup_arguments_when_cleanup_traps() {
    unsafe extern "C-unwind" fn failing_cleanup(
        _args: *const i64,
        _arg_count: usize,
    ) -> *mut OpaqueValue {
        super::runtime_error("cleanup failed")
    }

    let retained = boxed_value(Value::String("retained cleanup argument".to_string()));
    let retained_address = retained as usize;
    let result = run_lightweight_root_task(move || {
        let task = spawn_lightweight_task(move || {
            super::with_direct_task_runtime_scope(|| {
                super::with_task_runtime_error_capture(|| {
                    let retained = retained_address as *mut OpaqueValue;
                    let args = super::aurora_direct_arg_buffer_new(1);
                    super::aurora_direct_arg_buffer_store(args, 0, retained as i64);
                    super::aurora_direct_register_cleanup(
                        failing_cleanup as *const () as usize as i64,
                        args,
                        1,
                    );
                    super::runtime_error("body failed")
                })
            })
        })?;

        match task.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None) {
            TaskWaitStatus::Ready(Err(error)) if error.message == "body failed" => Ok(Value::Unit),
            other => Err(Diagnostic::new(format!(
                "expected primary body failure, got {other:?}"
            ))),
        }
    });

    assert_eq!(result.expect("root task should complete"), Value::Unit);
    assert_eq!(
        unsafe { &*retained }.ref_count.load(Ordering::SeqCst),
        1,
        "cleanup registration must release its retained arguments while unwinding"
    );
    unsafe {
        release_value(retained);
    }
}

#[test]
fn native_runtime_retain_and_release_keep_values_alive_until_last_handle() {
    let boxed = string_value("aurora");
    let retained = unsafe { retain_value(boxed) };

    unsafe { release_value(boxed) };
    assert_eq!(
        unsafe { value_ref(retained) },
        Value::String("aurora".to_string())
    );

    unsafe { release_value(retained) };
}

#[test]
fn native_runtime_arg_buffer_store_retains_opaque_values() {
    let buffer = super::aurora_direct_arg_buffer_new(1);
    let value = string_value("buffered");
    super::aurora_direct_arg_buffer_store(buffer, 0, value as i64);

    unsafe {
        release_value(value);
        let stored = *buffer as *mut OpaqueValue;
        assert_eq!(value_ref(stored), Value::String("buffered".to_string()));
        release_value(stored);
        free_arg_buffer(buffer, 1);
    }
}

#[test]
fn native_runtime_boxing_range_and_condition_helpers_cover_remaining_valid_paths() {
    assert_eq!(expect_float(super::aurora_direct_box_f64(2.5)), 2.5);
    assert!(!expect_bool_boxed(super::aurora_direct_box_bool(0)));
    expect_unit(super::aurora_direct_box_unit());
    assert_eq!(
        expect_int(super::aurora_direct_box_uint_literal(b"42".as_ptr(), 2)),
        42
    );
    assert_eq!(
        expect_string(super::aurora_direct_string_literal(b"aurora".as_ptr(), 6)),
        "aurora"
    );
    assert_eq!(
        expect_string(super::aurora_direct_stringify_value(super::boxed_value(
            Value::Range(RangeValue { start: 2, end: 4 },)
        ))),
        "range(2, 4)"
    );

    let range = super::aurora_direct_range_new(2, 5);
    assert_eq!(super::aurora_direct_range_current(range), 2);
    assert_eq!(super::aurora_direct_range_end(range), 5);
    match unsafe { take_value(super::aurora_direct_range_advance(range)) } {
        Value::Range(advanced) => {
            assert_eq!(advanced.start, 3);
            assert_eq!(advanced.end, 5);
        }
        other => panic!("expected advanced range, found {:?}", other),
    }

    assert_eq!(super::aurora_direct_unbox_i64(int_value(9)), 9);
    assert_eq!(super::aurora_direct_unbox_f64(float_value(1.5)), 1.5);
    assert_eq!(super::aurora_direct_unbox_bool(bool_value(true)), 1);
    assert_eq!(
        super::aurora_direct_value_as_condition(bool_value(false)),
        0
    );
    assert_eq!(super::aurora_direct_value_as_condition(int_value(0)), 0);
    assert_eq!(super::aurora_direct_value_as_condition(int_value(3)), 1);
    assert_eq!(
        super::aurora_direct_value_as_condition(super::aurora_direct_box_unit()),
        0
    );

    let vec = super::aurora_direct_vec_empty();
    assert_eq!(super::aurora_direct_vec_is_empty(vec), 1);
    expect_unit(super::aurora_direct_vec_push_in_place(vec, int_value(1)));
    assert_eq!(super::aurora_direct_vec_len(vec), 1);
    assert_eq!(super::aurora_direct_vec_is_empty(vec), 0);

    let map = super::aurora_direct_map_empty();
    assert_eq!(super::aurora_direct_map_is_empty(map), 1);
    expect_option_none(super::aurora_direct_map_set_in_place(
        map,
        string_value("answer"),
        int_value(42),
    ));
    assert_eq!(super::aurora_direct_map_is_empty(map), 0);

    let set = super::aurora_direct_set_empty();
    assert_eq!(super::aurora_direct_set_is_empty(set), 1);
    assert_eq!(
        super::aurora_direct_set_insert_in_place(set, string_value("ready")),
        1
    );
    assert_eq!(super::aurora_direct_set_is_empty(set), 0);
    expect_option_none(super::aurora_direct_set_index_option(set, 5));
}

#[test]
fn native_runtime_collection_helpers_cover_remaining_success_paths() {
    let vec = int_vec(&[1, 2, 3]);
    assert_eq!(
        expect_option_some_int(super::aurora_direct_vec_index_option(vec, 1)),
        2
    );
    assert_eq!(expect_int(super::aurora_direct_vec_index(vec, 2, 0, 0)), 3);
    expect_unit(super::aurora_direct_vec_set_index_in_place(
        vec,
        0,
        int_value(9),
        0,
        0,
    ));
    assert_eq!(
        expect_vec_ints(super::aurora_direct_clone_value(vec)),
        vec![9, 2, 3]
    );

    let map = super::aurora_direct_map_empty();
    expect_option_none(super::aurora_direct_map_set_in_place(
        map,
        string_value("a"),
        int_value(1),
    ));
    expect_option_none(super::aurora_direct_map_set_in_place(
        map,
        string_value("b"),
        int_value(2),
    ));
    assert_eq!(
        expect_option_some_int(super::aurora_direct_map_get(map, string_value("a"))),
        1
    );
    assert_eq!(
        super::aurora_direct_map_contains_key(map, string_value("b")),
        1
    );
    assert_eq!(
        expect_vec_strings(super::aurora_direct_map_keys(map)),
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(
        expect_vec_ints(super::aurora_direct_map_values(map)),
        vec![1, 2]
    );
    let entries = unsafe { take_value(super::aurora_direct_map_entries(map)) };
    match entries {
        Value::Vec(entries) => {
            assert_eq!(entries.elements.len(), 2);
            assert!(matches!(&entries.elements[0], Value::Instance(_)));
        }
        other => panic!("expected map entries vec, found {:?}", other),
    }
    assert_eq!(
        expect_int(super::aurora_direct_map_index(map, string_value("b"), 0, 0)),
        2
    );
    expect_unit(super::aurora_direct_map_set_index_in_place(
        map,
        string_value("b"),
        int_value(7),
        0,
        0,
    ));
    assert_eq!(
        expect_option_some_int(super::aurora_direct_map_remove_in_place(
            map,
            string_value("a"),
        )),
        1
    );
    expect_unit(super::aurora_direct_map_clear_in_place(map));
    assert_eq!(super::aurora_direct_map_is_empty(map), 1);

    let set = super::aurora_direct_set_empty();
    assert_eq!(
        super::aurora_direct_set_contains(set, string_value("ready")),
        0
    );
    assert_eq!(
        super::aurora_direct_set_insert_in_place(set, string_value("ready")),
        1
    );
    assert_eq!(
        super::aurora_direct_set_contains(set, string_value("ready")),
        1
    );
    assert_eq!(
        super::aurora_direct_set_remove_in_place(set, string_value("ready")),
        1
    );
    assert_eq!(
        super::aurora_direct_set_contains(set, string_value("ready")),
        0
    );

    let other = super::aurora_direct_map_empty();
    expect_option_none(super::aurora_direct_map_set_in_place(
        other,
        string_value("b"),
        int_value(9),
    ));
    expect_option_none(super::aurora_direct_map_set_in_place(
        other,
        string_value("c"),
        int_value(3),
    ));
    expect_unit(super::aurora_direct_map_extend_in_place(map, other));
    assert_eq!(
        expect_vec_strings(super::aurora_direct_map_keys(map)),
        vec!["b".to_string(), "c".to_string()]
    );
    assert_eq!(
        expect_vec_ints(super::aurora_direct_map_values(map)),
        vec![9, 3]
    );
}

#[test]
fn duration_integer_outside_signed_range_helper_exits_with_error() {
    if std::env::var("AURORA_DIRECT_RUNTIME_HELPER").as_deref() == Ok("duration-int-out-of-range") {
        extract_duration_millis(&Value::Int(IntegerValue::from_literal(u128::MAX)));
    }

    let output = Command::new(std::env::current_exe().expect("test binary should exist"))
        .arg("--exact")
        .arg("native_runtime::tests::duration_integer_outside_signed_range_helper_exits_with_error")
        .arg("--nocapture")
        .env("AURORA_DIRECT_RUNTIME_HELPER", "duration-int-out-of-range")
        .output()
        .expect("child test process should run");

    assert!(
        !output.status.success(),
        "duration helper should exit with failure for out-of-range integers"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("expected `Duration`, found an integer outside signed timer range"),
        "duration helper stderr should mention out-of-range integer values"
    );
}

#[test]
fn duration_type_mismatch_helper_exits_with_error() {
    if std::env::var("AURORA_DIRECT_RUNTIME_HELPER").as_deref() == Ok("duration-type") {
        extract_duration_millis(&Value::String("oops".to_string()));
    }

    let output = Command::new(std::env::current_exe().expect("test binary should exist"))
        .arg("--exact")
        .arg("native_runtime::tests::duration_type_mismatch_helper_exits_with_error")
        .arg("--nocapture")
        .env("AURORA_DIRECT_RUNTIME_HELPER", "duration-type")
        .output()
        .expect("child test process should run");

    assert!(
        !output.status.success(),
        "duration helper should exit with failure for wrong value types"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("expected `Duration`, found `String`"),
        "duration helper stderr should mention the wrong runtime type"
    );
}

#[test]
fn native_runtime_operator_and_io_helpers_cover_additional_paths() {
    assert_eq!(render_bool(0), "false");
    assert_eq!(render_bool(7), "true");
    assert_eq!(
        int32_overflow_message(9),
        "integer value `9` does not fit in `int32`"
    );
    assert_eq!(render_float(4.0), "4.0");

    assert_eq!(
        compare_values(
            Value::String("a".to_string()),
            Value::String("b".to_string()),
            BinaryOp::Less
        )
        .expect("string ordering should work"),
        Value::Bool(true)
    );
    let string_compare_error = compare_values(
        Value::String("a".to_string()),
        Value::String("b".to_string()),
        BinaryOp::Add,
    )
    .expect_err("unsupported string comparison operators should fail");
    assert!(string_compare_error
        .message
        .contains("unsupported comparison operator"));

    let int_compare_error = compare_values(
        Value::Int(IntegerValue::from_signed(1)),
        Value::Int(IntegerValue::from_signed(2)),
        BinaryOp::Add,
    )
    .expect_err("unsupported int comparison operators should fail");
    assert!(int_compare_error
        .message
        .contains("unsupported comparison operator"));

    let float_compare_error = compare_values(Value::Float(1.0), Value::Float(2.0), BinaryOp::Add)
        .expect_err("unsupported float comparison operators should fail");
    assert!(float_compare_error
        .message
        .contains("unsupported comparison operator"));

    let mismatch_compare_error = compare_values(
        Value::Bool(true),
        Value::String("no".to_string()),
        BinaryOp::Less,
    )
    .expect_err("mismatched comparisons should fail");
    assert!(mismatch_compare_error
        .message
        .contains("unsupported comparison between"));

    assert_eq!(
        eval_binary_value(Value::Bool(true), Value::Bool(false), BinaryOp::And)
            .expect("bool and should work"),
        Value::Bool(false)
    );
    let and_error = eval_binary_value(
        Value::Int(IntegerValue::from_signed(1)),
        Value::Bool(false),
        BinaryOp::And,
    )
    .expect_err("logical and should require bools");
    assert!(and_error
        .message
        .contains("logical `and` expects bool operands"));

    let or_error = eval_binary_value(
        Value::Bool(true),
        Value::String("x".to_string()),
        BinaryOp::Or,
    )
    .expect_err("logical or should require bools");
    assert!(or_error
        .message
        .contains("logical `or` expects bool operands"));

    let add_error = eval_binary_value(Value::Bool(true), Value::Bool(false), BinaryOp::Add)
        .expect_err("add should reject non-addable types");
    assert!(add_error.message.contains("unsupported `+` operands"));

    let sub_error = eval_binary_value(
        Value::String("a".to_string()),
        Value::String("b".to_string()),
        BinaryOp::Sub,
    )
    .expect_err("sub should reject strings");
    assert!(sub_error.message.contains("unsupported `-` operands"));

    let mul_error = eval_binary_value(Value::Bool(true), Value::Bool(false), BinaryOp::Mul)
        .expect_err("mul should reject bools");
    assert!(mul_error.message.contains("unsupported `*` operands"));

    let div_error = eval_binary_value(Value::Bool(true), Value::Bool(false), BinaryOp::Div)
        .expect_err("div should reject bools");
    assert!(div_error.message.contains("unsupported `/` operands"));

    let mod_error = eval_binary_value(
        Value::String("a".to_string()),
        Value::String("b".to_string()),
        BinaryOp::Mod,
    )
    .expect_err("mod should reject strings");
    assert!(mod_error.message.contains("unsupported `%` operands"));

    let not_error = eval_unary_value(Value::Int(IntegerValue::from_signed(1)), UnaryOp::Not)
        .expect_err("not should reject non-bools");
    assert!(not_error.message.contains("`not` expects `bool`"));

    let neg_error = eval_unary_value(Value::Bool(true), UnaryOp::Neg)
        .expect_err("neg should reject non-numeric values");
    assert!(neg_error
        .message
        .contains("unary `-` expects a numeric value"));

    assert_eq!(super::aurora_direct_value_as_condition(bool_value(true)), 1);
    assert_eq!(super::aurora_direct_value_as_condition(int_value(0)), 0);
    assert_eq!(
        super::aurora_direct_value_as_condition(boxed_value(Value::Unit)),
        0
    );

    assert_eq!(
        expect_int(super::aurora_direct_unary_value(0, int_value(9))),
        -9
    );
    assert_eq!(
        expect_bool_boxed(super::aurora_direct_unary_value_at(
            1,
            bool_value(false),
            3,
            4
        )),
        true
    );

    assert_eq!(
        expect_int(super::aurora_direct_binary_value(
            0,
            int_value(2),
            int_value(3)
        )),
        5
    );
    assert_eq!(
        expect_int(super::aurora_direct_binary_value(
            1,
            int_value(7),
            int_value(4)
        )),
        3
    );
    assert_eq!(
        expect_int(super::aurora_direct_binary_value(
            2,
            int_value(6),
            int_value(5)
        )),
        30
    );
    assert_eq!(
        expect_int(super::aurora_direct_binary_value(
            3,
            int_value(9),
            int_value(2)
        )),
        4
    );
    assert_eq!(
        expect_int(super::aurora_direct_binary_value(
            4,
            int_value(9),
            int_value(2)
        )),
        1
    );
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        5,
        int_value(4),
        int_value(4)
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        6,
        int_value(4),
        int_value(5)
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        7,
        int_value(4),
        int_value(5)
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        8,
        int_value(5),
        int_value(5)
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        9,
        int_value(6),
        int_value(5)
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        10,
        int_value(6),
        int_value(6)
    )));
    assert!(!expect_bool_boxed(super::aurora_direct_binary_value(
        11,
        bool_value(true),
        bool_value(false)
    )));
    assert!(expect_bool_boxed(super::aurora_direct_binary_value(
        12,
        bool_value(false),
        bool_value(true)
    )));
    assert_eq!(
        expect_int(super::aurora_direct_binary_value_at(
            0,
            int_value(10),
            int_value(1),
            5,
            6,
        )),
        11
    );

    let target = "float64";
    assert_eq!(
        expect_float(super::aurora_direct_cast_value(
            int_value(7),
            target.as_ptr(),
            target.len(),
        )),
        7.0
    );
    let target = "int32";
    assert_eq!(
        expect_int(super::aurora_direct_cast_value_at(
            int_value(7),
            target.as_ptr(),
            target.len(),
            7,
            8,
        )),
        7
    );
}
