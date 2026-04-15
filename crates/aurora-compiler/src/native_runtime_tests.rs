
use super::{
    boxed_value, checked_vec_index, checked_vec_index_at, compare_values, current_cancellation,
    decode_bytes, eval_binary_value, eval_unary_value, extract_duration_millis,
    int32_overflow_message, render_bool, render_float, render_runtime_diagnostic, runtime_span,
    value_mut, value_ref, value_type_name, with_cancellation_scope, OpaqueValue,
};
use crate::ast::{BinaryOp, UnaryOp};
use crate::diag::{Diagnostic, Span};
use crate::integer::IntegerValue;
use crate::interpreter::{
    CancellationContext, ChannelValue, EnumVariantValue, InstanceValue, MapValue,
    ModuleNamespaceValue, RangeValue, SetValue, TaskGroupValue, TaskValue, Value, VecValue,
};
use std::collections::BTreeMap;
use std::process::Command;
use std::thread;

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
        super::aurora_direct_vec_push_in_place(vec, int_value(*value));
    }
    vec
}

unsafe fn take_value(ptr: *mut OpaqueValue) -> Value {
    super::take_value(ptr)
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
                Value::String(text) => text,
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
            match *variant.payload.expect("expected option payload") {
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
            match *variant.payload.expect("expected option payload") {
                Value::String(text) => text,
                other => panic!("expected string payload, found {:?}", other),
            }
        }
        other => panic!("expected Option.Some(String), found {:?}", other),
    }
}

fn expect_option_none(ptr: *mut OpaqueValue) {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "None" => {}
        other => panic!("expected Option.None, found {:?}", other),
    }
}

fn expect_result_ok_int(ptr: *mut OpaqueValue) -> i128 {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Ok" =>
        {
            match *variant.payload.expect("expected result payload") {
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
            match *variant.payload.expect("expected result payload") {
                Value::Float(value) => value,
                other => panic!("expected float payload, found {:?}", other),
            }
        }
        other => panic!("expected Result.Ok(float), found {:?}", other),
    }
}

fn expect_result_err_string(ptr: *mut OpaqueValue) -> String {
    match unsafe { take_value(ptr) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Err" =>
        {
            match *variant.payload.expect("expected result payload") {
                Value::String(text) => text,
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
    assert_eq!(
        super::eval_binary_value(Value::Float(9.0), Value::Float(4.0), BinaryOp::Mod,)
            .expect("float modulo should succeed"),
        Value::Float(1.0)
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
fn runtime_init_is_callable() {
    super::aurora_direct_runtime_init(
        b"/virtual/test.au".as_ptr(),
        b"/virtual/test.au".len(),
        b"def main() -> int32:\n    return 0\n".as_ptr(),
        b"def main() -> int32:\n    return 0\n".len(),
    );
}

#[test]
fn direct_print_helpers_are_callable() {
    super::aurora_direct_print_i64(7);
    super::aurora_direct_print_f64(7.0);
    super::aurora_direct_print_bool(0);
    super::aurora_direct_print_bool(1);
}

#[test]
fn sqrt_helper_matches_standard_library() {
    assert_eq!(super::aurora_direct_sqrt_f64(25.0), 5.0);
}

#[test]
fn direct_runtime_string_and_numeric_helpers_cover_builtin_surface() {
    assert_eq!(
        super::aurora_direct_string_len(string_value("  Aurora Repo  ")),
        15
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
        expect_float(super::aurora_direct_min(float_value(4.5), float_value(9.5))),
        4.5
    );
    assert_eq!(
        expect_int(super::aurora_direct_max(int_value(4), int_value(9))),
        9
    );
    assert_eq!(
        expect_float(super::aurora_direct_max(float_value(4.5), float_value(9.5))),
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
}

unsafe extern "C" fn test_native_thunk(args: *const i64, len: usize) -> *mut OpaqueValue {
    let args = std::slice::from_raw_parts(args, len);
    super::aurora_direct_box_i64(args.iter().copied().sum())
}

#[test]
fn direct_runtime_scalar_and_concurrency_helpers_cover_remaining_surface() {
    assert_eq!(super::aurora_direct_unbox_i64(int_value(17)), 17);
    assert_eq!(super::aurora_direct_unbox_f64(float_value(2.5)), 2.5);
    assert_eq!(super::aurora_direct_unbox_bool(bool_value(true)), 1);
    assert_eq!(super::aurora_direct_value_as_condition(bool_value(true)), 1);
    assert_eq!(
        expect_int(super::aurora_direct_unary_value(0, int_value(-7))),
        7
    );
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
        expect_string(super::aurora_direct_binary_value_at(
            0,
            string_value("aurora"),
            string_value(" repo"),
            1,
            1,
        )),
        "aurora repo"
    );
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
            b"Channel".as_ptr(),
            "Channel".len(),
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
    let boxed_payload = super::aurora_direct_enum_variant(
        b"Option".as_ptr(),
        "Option".len(),
        b"Some".as_ptr(),
        "Some".len(),
        string_value("payload"),
    );
    assert_eq!(
        expect_string(super::aurora_direct_variant_payload(boxed_payload)),
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
    super::aurora_direct_arg_buffer_store(buffer, 0, 20);
    super::aurora_direct_arg_buffer_store(buffer, 1, 22);
    let task = super::aurora_direct_spawn_call(
        test_native_thunk as usize as i64,
        buffer,
        2,
        0,
        std::ptr::null_mut(),
    );
    assert_eq!(expect_int(super::aurora_direct_task_join(task)), 42);

    let channel = super::aurora_direct_channel_new();
    let send_ok = unsafe { take_value(super::aurora_direct_channel_send(channel, int_value(9))) };
    match send_ok {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Ok" => {}
        other => panic!("expected Result.Ok(Unit), found {:?}", other),
    }
    assert_eq!(
        expect_option_some_int(super::aurora_direct_channel_recv(channel)),
        9
    );
    assert_eq!(super::aurora_direct_channel_try_recv(channel), 0);
    expect_unit(super::aurora_direct_channel_close(channel));
    match unsafe { take_value(super::aurora_direct_channel_send(channel, int_value(7))) } {
        Value::EnumVariant(variant)
            if variant.enum_name == "Result" && variant.variant_name == "Err" => {}
        other => panic!(
            "expected Result.Err(SendError.Closed(...)), found {:?}",
            other
        ),
    }
    expect_option_none(super::aurora_direct_channel_recv(channel));
    assert_eq!(super::aurora_direct_channel_try_recv(channel), 1);

    let group = super::aurora_direct_task_group_new();
    expect_unit(super::aurora_direct_task_group_cancel(group));
    assert_eq!(super::aurora_direct_cancelled(), 0);
    expect_unit(super::aurora_direct_task_group_close(group, 0));
    let group = boxed_value(Value::TaskGroup(TaskGroupValue::new(
        &CancellationContext::default(),
    )));
    if let Value::TaskGroup(group_value) = unsafe { value_ref(group) } {
        group_value.register_task(TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit))));
    }
    expect_unit(super::aurora_direct_task_group_close(group, 1));

    let deadline = super::aurora_direct_deadline_new(duration_value(0));
    assert_eq!(super::aurora_direct_deadline_ready(deadline), 1);
    assert_eq!(super::aurora_direct_deadline_ready(0), 1);
    super::aurora_direct_sleep_ms(0);
    expect_unit(super::aurora_direct_sleep_value(duration_value(0)));
}

#[test]
fn division_by_zero_helper_exits_with_error() {
    if std::env::var("AURORA_DIRECT_RUNTIME_HELPER").as_deref() == Ok("divzero") {
        super::aurora_direct_runtime_init(
            b"/virtual/test.au".as_ptr(),
            b"/virtual/test.au".len(),
            b"def main() -> int32:\n    print(1 / 0)\n".as_ptr(),
            b"def main() -> int32:\n    print(1 / 0)\n".len(),
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
fn native_runtime_entrypoint_guards_invalid_inputs() {
    assert_eq!(
        crate::mir_runtime::aurora_native_run(
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
        ),
        1
    );

    let mir_json = b"{}";
    let invalid_path = [0xff_u8];
    let source = b"def main() -> int32:\n    return 0\n";
    assert_eq!(
        crate::mir_runtime::aurora_native_run(
            mir_json.as_ptr(),
            mir_json.len(),
            invalid_path.as_ptr(),
            invalid_path.len(),
            source.as_ptr(),
            source.len(),
        ),
        1
    );

    let source_path = b"/tmp/test.au";
    let invalid_source = [0xff_u8];
    assert_eq!(
        crate::mir_runtime::aurora_native_run(
            mir_json.as_ptr(),
            mir_json.len(),
            source_path.as_ptr(),
            source_path.len(),
            invalid_source.as_ptr(),
            invalid_source.len(),
        ),
        1
    );

    assert_eq!(
        render_runtime_diagnostic(crate::diag::Diagnostic::new("oops")),
        "error: oops"
    );
}

#[test]
fn direct_runtime_helper_errors_surface_expected_diagnostics() {
    if let Ok(case) = std::env::var("AURORA_DIRECT_RUNTIME_CASE") {
        match case.as_str() {
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
                );
                super::aurora_direct_variant_payload(ready);
            }
            "variant-payload-type" => {
                super::aurora_direct_variant_payload(int_value(1));
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
            "map-set-type" => {
                super::aurora_direct_map_set_in_place(
                    int_value(1),
                    string_value("name"),
                    int_value(1),
                );
            }
            "set-len-type" => {
                super::aurora_direct_set_len(int_value(1));
            }
            "set-insert-type" => {
                super::aurora_direct_set_insert_in_place(int_value(1), int_value(2));
            }
            "vec-index-negative-at" => {
                checked_vec_index_at(-1, 3, 4);
            }
            "vec-index-oob-no-span" => {
                super::aurora_direct_vec_index(int_vec(&[1]), 5, 0, 0);
            }
            "vec-set-oob-no-span" => {
                super::aurora_direct_vec_set_index_in_place(int_vec(&[1]), 5, int_value(9), 0, 0);
            }
            "unbox-i64-overflow" => {
                super::aurora_direct_unbox_i64(boxed_value(Value::Int(
                    IntegerValue::from_literal((i64::MAX as u128) + 1),
                )));
            }
            "unbox-i64-type" => {
                super::aurora_direct_unbox_i64(bool_value(true));
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
                    Err("boom".to_string())
                }))));
                super::aurora_direct_task_join(task);
            }
            other => panic!("unexpected runtime helper case: {other}"),
        }
    }

    for (case, expected) in [
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
        ("map-set-type", "expected `Map`, found `integer`"),
        ("set-len-type", "expected `Set`, found `integer`"),
        ("set-insert-type", "expected `Set`, found `integer`"),
        (
            "vec-index-negative-at",
            "vector index `-1` cannot be negative",
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
            "unbox-i64-overflow",
            "direct backend expected an integer that fits in host i64",
        ),
        (
            "unbox-i64-type",
            "direct backend expected `int32`, found `bool`",
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
        ("task-join-error", "boom"),
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
    assert_eq!(checked_vec_index(3), 3);
    assert_eq!(checked_vec_index_at(4, 1, 1), 4);

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
            payload: None,
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
        "Channel"
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
        compare_values(
            Value::Int(IntegerValue::from_signed(1)),
            Value::Int(IntegerValue::from_signed(2)),
            BinaryOp::Less,
        )
        .expect("int comparison should work"),
        Value::Bool(true)
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
        payload: None,
    }));
    let unit_value = super::boxed_value(Value::Unit);

    assert_eq!(
        super::aurora_direct_value_type_matches(int_value(7), b"int32".as_ptr(), "int32".len(),),
        1
    );
    assert_eq!(
        super::aurora_direct_value_type_matches(int_value(7), b"uint64".as_ptr(), "uint64".len(),),
        1
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
fn native_runtime_thread_local_and_pointer_helpers_cover_remaining_paths() {
    assert!(!current_cancellation().is_cancelled());
    let group = TaskGroupValue::new(&crate::interpreter::CancellationContext::default());
    let child = group.child_cancellation();
    group.cancel();
    let scoped = with_cancellation_scope(child, || current_cancellation().is_cancelled());
    assert!(scoped);
    assert!(!current_cancellation().is_cancelled());

    assert_eq!(
        extract_duration_millis(&Value::Int(IntegerValue::from_signed(7))),
        7
    );
    assert_eq!(extract_duration_millis(&Value::Duration(9)), 9);
    assert_eq!(decode_bytes(b"aurora".as_ptr(), "aurora".len()), "aurora");

    let boxed = boxed_value(Value::Int(IntegerValue::from_signed(5)));
    assert_eq!(
        unsafe { value_ref(boxed) },
        &Value::Int(IntegerValue::from_signed(5))
    );
    match unsafe { value_mut(boxed) } {
        Value::Int(value) => *value = IntegerValue::from_signed(8),
        other => panic!("expected int box, found {:?}", other),
    }
    assert_eq!(expect_int(boxed), 8);

    let vec = super::aurora_direct_vec_empty();
    expect_unit(super::aurora_direct_vec_push_in_place(
        vec,
        string_value("x"),
    ));
    assert_eq!(super::vector_from_ptr(vec).elements.len(), 1);
    super::vector_from_ptr_mut(vec)
        .elements
        .push(Value::String("y".to_string()));
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
    assert_eq!(super::map_from_ptr(map).entries.len(), 1);
    super::map_from_ptr_mut(map).entries.push((
        Value::String("other".to_string()),
        Value::Int(IntegerValue::from_signed(2)),
    ));
    assert_eq!(
        expect_vec_ints(super::aurora_direct_map_values(map)),
        vec![1, 2]
    );

    let set = super::aurora_direct_set_empty();
    assert_eq!(
        super::aurora_direct_set_insert_in_place(set, string_value("ready")),
        1
    );
    assert_eq!(super::set_from_ptr(set).elements.len(), 1);
    super::set_from_ptr_mut(set)
        .elements
        .push(Value::String("go".to_string()));
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
            payload: None,
        })),
        "Status"
    );
    assert_eq!(
        value_type_name(&Value::Channel(ChannelValue::new())),
        "Channel"
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
}
