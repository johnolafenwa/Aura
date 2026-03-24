use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::process;
use std::slice;
use std::str;
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use crate::ast::{BinaryOp, UnaryOp};
use crate::diag::Diagnostic;
use crate::interpreter::{
    cast_numeric_value, CancellationContext, ChannelValue, EnumVariantValue, InstanceValue,
    RangeValue, TaskGroupValue, TaskValue, TryRecvResult, Value,
};
use crate::sema::Type;

fn write_stdout(text: &str) {
    let mut stdout = io::stdout().lock();
    if let Err(error) = stdout
        .write_all(text.as_bytes())
        .and_then(|_| stdout.flush())
    {
        if error.kind() == io::ErrorKind::BrokenPipe {
            process::exit(0);
        }
        let _ = writeln!(io::stderr().lock(), "failed to write to stdout: {}", error);
        process::exit(1);
    }
}

fn render_bool(value: i64) -> &'static str {
    if value == 0 {
        "false"
    } else {
        "true"
    }
}

fn int32_overflow_message(value: i64) -> String {
    format!("integer value `{}` does not fit in `int32`", value)
}

fn render_float(value: f64) -> String {
    let mut rendered = value.to_string();
    if !rendered.contains(['.', 'e', 'E']) {
        rendered.push_str(".0");
    }
    rendered
}

#[repr(transparent)]
pub struct OpaqueValue(Value);

type NativeThunk = unsafe extern "C" fn(*const i64, usize) -> *mut OpaqueValue;

thread_local! {
    static DIRECT_CANCELLATION: RefCell<CancellationContext> =
        RefCell::new(CancellationContext::default());
}

#[repr(transparent)]
struct Deadline(Instant);

fn current_cancellation() -> CancellationContext {
    DIRECT_CANCELLATION.with(|slot| slot.borrow().clone())
}

fn with_cancellation_scope<T>(
    cancellation: CancellationContext,
    work: impl FnOnce() -> T,
) -> T {
    DIRECT_CANCELLATION.with(|slot| {
        let previous = slot.replace(cancellation);
        let result = work();
        slot.replace(previous);
        result
    })
}

fn extract_duration_millis(value: &Value) -> i128 {
    match value {
        Value::Int(value) | Value::Duration(value) => *value,
        other => runtime_error(format!(
            "expected `Duration`, found `{}`",
            value_type_name(other)
        )),
    }
}

fn boxed_value(value: Value) -> *mut OpaqueValue {
    Box::into_raw(Box::new(OpaqueValue(value)))
}

unsafe fn value_ref<'a>(ptr: *mut OpaqueValue) -> &'a Value {
    &(*ptr).0
}

unsafe fn take_value(ptr: *mut OpaqueValue) -> Value {
    (*ptr).0.clone()
}

fn decode_bytes<'a>(ptr: *const u8, len: usize) -> &'a str {
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    str::from_utf8(bytes).expect("aurora direct runtime should only receive valid UTF-8")
}

fn runtime_error(message: impl AsRef<str>) -> ! {
    let _ = writeln!(io::stderr().lock(), "{}", message.as_ref());
    process::exit(1);
}

fn value_type_name(value: &Value) -> String {
    match value {
        Value::Int(_) => "int32".to_string(),
        Value::Float(_) => "float64".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::String(_) => "String".to_string(),
        Value::Duration(_) => "Duration".to_string(),
        Value::Range(_) => "Range".to_string(),
        Value::ModuleNamespace(namespace) => format!("module {}", namespace.path),
        Value::Unit => "None".to_string(),
        Value::Instance(instance) => instance.class_name.clone(),
        Value::EnumVariant(variant) => variant.enum_name.clone(),
        Value::Channel(_) => "Channel".to_string(),
        Value::Task(_) => "Task".to_string(),
        Value::TaskGroup(_) => "TaskGroup".to_string(),
    }
}

fn compare_values(
    left: Value,
    right: Value,
    op: BinaryOp,
) -> std::result::Result<Value, Diagnostic> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(Value::Bool(match op {
            BinaryOp::Eq => left == right,
            BinaryOp::NotEq => left != right,
            BinaryOp::Less => left < right,
            BinaryOp::LessEq => left <= right,
            BinaryOp::Greater => left > right,
            BinaryOp::GreaterEq => left >= right,
            _ => {
                return Err(Diagnostic::new(format!(
                    "unsupported comparison operator `{:?}` for int values",
                    op
                )))
            }
        })),
        (Value::Float(left), Value::Float(right)) => Ok(Value::Bool(match op {
            BinaryOp::Eq => left == right,
            BinaryOp::NotEq => left != right,
            BinaryOp::Less => left < right,
            BinaryOp::LessEq => left <= right,
            BinaryOp::Greater => left > right,
            BinaryOp::GreaterEq => left >= right,
            _ => {
                return Err(Diagnostic::new(format!(
                    "unsupported comparison operator `{:?}` for float values",
                    op
                )))
            }
        })),
        (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(match op {
            BinaryOp::Eq => left == right,
            BinaryOp::NotEq => left != right,
            _ => {
                return Err(Diagnostic::new(format!(
                    "unsupported comparison operator `{:?}` for bool values",
                    op
                )))
            }
        })),
        (Value::String(left), Value::String(right)) => Ok(Value::Bool(match op {
            BinaryOp::Eq => left == right,
            BinaryOp::NotEq => left != right,
            BinaryOp::Less => left < right,
            BinaryOp::LessEq => left <= right,
            BinaryOp::Greater => left > right,
            BinaryOp::GreaterEq => left >= right,
            _ => {
                return Err(Diagnostic::new(format!(
                    "unsupported comparison operator `{:?}` for string values",
                    op
                )))
            }
        })),
        (left, right) => Err(Diagnostic::new(format!(
            "unsupported comparison between `{}` and `{}`",
            value_type_name(&left),
            value_type_name(&right)
        ))),
    }
}

fn eval_binary_value(
    left: Value,
    right: Value,
    op: BinaryOp,
) -> std::result::Result<Value, Diagnostic> {
    match op {
        BinaryOp::And => match (left, right) {
            (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left && right)),
            (left, right) => Err(Diagnostic::new(format!(
                "logical `and` expects bool operands, found `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
        BinaryOp::Or => match (left, right) {
            (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left || right)),
            (left, right) => Err(Diagnostic::new(format!(
                "logical `or` expects bool operands, found `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Less
        | BinaryOp::LessEq
        | BinaryOp::Greater
        | BinaryOp::GreaterEq => compare_values(left, right, op),
        BinaryOp::Add => match (left, right) {
            (Value::Int(left), Value::Int(right)) => left
                .checked_add(right)
                .map(Value::Int)
                .ok_or_else(|| Diagnostic::new("integer overflow")),
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left + right)),
            (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
            (left, right) => Err(Diagnostic::new(format!(
                "unsupported `+` operands `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
        BinaryOp::Sub => match (left, right) {
            (Value::Int(left), Value::Int(right)) => left
                .checked_sub(right)
                .map(Value::Int)
                .ok_or_else(|| Diagnostic::new("integer overflow")),
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left - right)),
            (left, right) => Err(Diagnostic::new(format!(
                "unsupported `-` operands `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
        BinaryOp::Mul => match (left, right) {
            (Value::Int(left), Value::Int(right)) => left
                .checked_mul(right)
                .map(Value::Int)
                .ok_or_else(|| Diagnostic::new("integer overflow")),
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left * right)),
            (left, right) => Err(Diagnostic::new(format!(
                "unsupported `*` operands `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
        BinaryOp::Div => match (left, right) {
            (Value::Int(_), Value::Int(0)) => Err(Diagnostic::new("division by zero")),
            (Value::Int(left), Value::Int(right)) => left
                .checked_div(right)
                .map(Value::Int)
                .ok_or_else(|| Diagnostic::new("integer overflow")),
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left / right)),
            (left, right) => Err(Diagnostic::new(format!(
                "unsupported `/` operands `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
        BinaryOp::Mod => match (left, right) {
            (Value::Int(_), Value::Int(0)) => Err(Diagnostic::new("division by zero")),
            (Value::Int(left), Value::Int(right)) => left
                .checked_rem(right)
                .map(Value::Int)
                .ok_or_else(|| Diagnostic::new("integer overflow")),
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left % right)),
            (left, right) => Err(Diagnostic::new(format!(
                "unsupported `%` operands `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
    }
}

fn eval_unary_value(value: Value, op: UnaryOp) -> std::result::Result<Value, Diagnostic> {
    match (op, value) {
        (UnaryOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
        (UnaryOp::Neg, Value::Int(value)) => value
            .checked_neg()
            .map(Value::Int)
            .ok_or_else(|| Diagnostic::new("integer overflow")),
        (UnaryOp::Neg, Value::Float(value)) => Ok(Value::Float(-value)),
        (UnaryOp::Not, other) => Err(Diagnostic::new(format!(
            "`not` expects `bool`, found `{}`",
            value_type_name(&other)
        ))),
        (UnaryOp::Neg, other) => Err(Diagnostic::new(format!(
            "unary `-` expects a numeric value, found `{}`",
            value_type_name(&other)
        ))),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_runtime_init() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_print_i64(value: i64) {
    write_stdout(&format!("{}\n", value));
}

#[no_mangle]
pub extern "C" fn aurora_direct_print_f64(value: f64) {
    write_stdout(&render_float(value));
    write_stdout("\n");
}

#[no_mangle]
pub extern "C" fn aurora_direct_print_bool(value: i64) {
    write_stdout(render_bool(value));
    write_stdout("\n");
}

#[no_mangle]
pub extern "C" fn aurora_direct_box_i64(value: i64) -> *mut OpaqueValue {
    boxed_value(Value::Int(value as i128))
}

#[no_mangle]
pub extern "C" fn aurora_direct_box_f64(value: f64) -> *mut OpaqueValue {
    boxed_value(Value::Float(value))
}

#[no_mangle]
pub extern "C" fn aurora_direct_box_bool(value: i64) -> *mut OpaqueValue {
    boxed_value(Value::Bool(value != 0))
}

#[no_mangle]
pub extern "C" fn aurora_direct_box_unit() -> *mut OpaqueValue {
    boxed_value(Value::Unit)
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_literal(
    ptr: *const u8,
    len: usize,
) -> *mut OpaqueValue {
    boxed_value(Value::String(decode_bytes(ptr, len).to_string()))
}

#[no_mangle]
pub extern "C" fn aurora_direct_duration_literal(value: i64) -> *mut OpaqueValue {
    boxed_value(Value::Duration(value as i128))
}

#[no_mangle]
pub extern "C" fn aurora_direct_clone_value(value: *mut OpaqueValue) -> *mut OpaqueValue {
    boxed_value(unsafe { take_value(value) })
}

#[no_mangle]
pub extern "C" fn aurora_direct_unbox_i64(value: *mut OpaqueValue) -> i64 {
    match unsafe { value_ref(value) } {
        Value::Int(value) => *value as i64,
        other => runtime_error(format!(
            "direct backend expected `int32`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_unbox_f64(value: *mut OpaqueValue) -> f64 {
    match unsafe { value_ref(value) } {
        Value::Float(value) => *value,
        other => runtime_error(format!(
            "direct backend expected `float64`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_unbox_bool(value: *mut OpaqueValue) -> i64 {
    match unsafe { value_ref(value) } {
        Value::Bool(value) => i64::from(*value),
        other => runtime_error(format!(
            "direct backend expected `bool`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_print_value(value: *mut OpaqueValue) {
    write_stdout(unsafe { value_ref(value) }.render().as_str());
    write_stdout("\n");
}

#[no_mangle]
pub extern "C" fn aurora_direct_value_as_condition(value: *mut OpaqueValue) -> i64 {
    match unsafe { value_ref(value) } {
        Value::Bool(value) => i64::from(*value),
        Value::Int(value) => i64::from(*value != 0),
        Value::Unit => 0,
        other => runtime_error(format!(
            "direct backend cannot use `{}` as a branch condition",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_unary_value(op: i32, value: *mut OpaqueValue) -> *mut OpaqueValue {
    let op = match op {
        0 => UnaryOp::Neg,
        1 => UnaryOp::Not,
        other => runtime_error(format!("unknown unary opcode `{}`", other)),
    };
    match eval_unary_value(unsafe { take_value(value) }, op) {
        Ok(value) => boxed_value(value),
        Err(error) => runtime_error(error.message),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_binary_value(
    op: i32,
    left: *mut OpaqueValue,
    right: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let op = match op {
        0 => BinaryOp::Add,
        1 => BinaryOp::Sub,
        2 => BinaryOp::Mul,
        3 => BinaryOp::Div,
        4 => BinaryOp::Mod,
        5 => BinaryOp::Eq,
        6 => BinaryOp::NotEq,
        7 => BinaryOp::Less,
        8 => BinaryOp::LessEq,
        9 => BinaryOp::Greater,
        10 => BinaryOp::GreaterEq,
        11 => BinaryOp::And,
        12 => BinaryOp::Or,
        other => runtime_error(format!("unknown binary opcode `{}`", other)),
    };
    match eval_binary_value(unsafe { take_value(left) }, unsafe { take_value(right) }, op) {
        Ok(value) => boxed_value(value),
        Err(error) => runtime_error(error.message),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_cast_value(
    value: *mut OpaqueValue,
    target_ptr: *const u8,
    target_len: usize,
) -> *mut OpaqueValue {
    let target = Type::named(decode_bytes(target_ptr, target_len));
    match cast_numeric_value(unsafe { take_value(value) }, &target, None) {
        Ok(value) => boxed_value(value),
        Err(error) => runtime_error(error.message),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_value_type_matches(
    value: *mut OpaqueValue,
    type_ptr: *const u8,
    type_len: usize,
) -> i64 {
    let expected = decode_bytes(type_ptr, type_len);
    let actual = unsafe { value_ref(value) };
    let matches = match actual {
        Value::Instance(instance) => instance.class_name == expected,
        Value::EnumVariant(variant) => variant.enum_name == expected,
        Value::String(_) => expected == "String",
        Value::Channel(_) => expected == "Channel",
        Value::Task(_) => expected == "Task",
        Value::TaskGroup(_) => expected == "TaskGroup",
        Value::Duration(_) => expected == "Duration",
        Value::Range(_) => expected == "Range",
        Value::Bool(_) => expected == "bool",
        Value::Float(_) => expected == "float64" || expected == "float32",
        Value::Int(_) => expected.starts_with("int") || expected.starts_with("uint"),
        Value::Unit => expected == "None",
        Value::ModuleNamespace(_) => expected.starts_with("module "),
    };
    i64::from(matches)
}

#[no_mangle]
pub extern "C" fn aurora_direct_enum_variant(
    enum_ptr: *const u8,
    enum_len: usize,
    variant_ptr: *const u8,
    variant_len: usize,
    payload: *mut OpaqueValue,
) -> *mut OpaqueValue {
    boxed_value(Value::EnumVariant(EnumVariantValue {
        enum_name: decode_bytes(enum_ptr, enum_len).to_string(),
        variant_name: decode_bytes(variant_ptr, variant_len).to_string(),
        payload: if payload.is_null() {
            None
        } else {
            Some(Box::new(unsafe { take_value(payload) }))
        },
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_variant_matches(
    value: *mut OpaqueValue,
    enum_ptr: *const u8,
    enum_len: usize,
    variant_ptr: *const u8,
    variant_len: usize,
) -> i64 {
    let expected_enum = decode_bytes(enum_ptr, enum_len);
    let expected_variant = decode_bytes(variant_ptr, variant_len);
    match unsafe { value_ref(value) } {
        Value::EnumVariant(variant) => i64::from(
            variant.enum_name == expected_enum && variant.variant_name == expected_variant,
        ),
        _ => 0,
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_variant_payload(value: *mut OpaqueValue) -> *mut OpaqueValue {
    match unsafe { value_ref(value) } {
        Value::EnumVariant(variant) => match &variant.payload {
            Some(payload) => boxed_value(payload.as_ref().clone()),
            None => runtime_error(format!(
                "enum variant `{}.{}` does not carry a payload",
                variant.enum_name, variant.variant_name
            )),
        },
        other => runtime_error(format!(
            "expected enum value, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_instance_new(
    class_ptr: *const u8,
    class_len: usize,
    names_ptr: *const *const u8,
    lens_ptr: *const usize,
    values_ptr: *const *mut OpaqueValue,
    count: usize,
) -> *mut OpaqueValue {
    let class_name = decode_bytes(class_ptr, class_len).to_string();
    let names = unsafe { slice::from_raw_parts(names_ptr, count) };
    let lens = unsafe { slice::from_raw_parts(lens_ptr, count) };
    let values = unsafe { slice::from_raw_parts(values_ptr, count) };
    let mut fields = BTreeMap::new();
    for index in 0..count {
        let name = decode_bytes(names[index], lens[index]).to_string();
        fields.insert(name, unsafe { take_value(values[index]) });
    }
    boxed_value(Value::Instance(InstanceValue { class_name, fields }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_instance_empty(
    class_ptr: *const u8,
    class_len: usize,
) -> *mut OpaqueValue {
    boxed_value(Value::Instance(InstanceValue {
        class_name: decode_bytes(class_ptr, class_len).to_string(),
        fields: BTreeMap::new(),
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_instance_get_field(
    value: *mut OpaqueValue,
    field_ptr: *const u8,
    field_len: usize,
) -> *mut OpaqueValue {
    let field = decode_bytes(field_ptr, field_len);
    match unsafe { value_ref(value) } {
        Value::Instance(instance) => instance
            .fields
            .get(field)
            .cloned()
            .map(boxed_value)
            .unwrap_or_else(|| {
                runtime_error(format!(
                    "class `{}` has no field `{}`",
                    instance.class_name, field
                ))
            }),
        other => runtime_error(format!(
            "cannot access field `{}` on non-instance `{}`",
            field,
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_instance_set_field(
    value: *mut OpaqueValue,
    field_ptr: *const u8,
    field_len: usize,
    new_value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let field = decode_bytes(field_ptr, field_len);
    match unsafe { value_ref(value) } {
        Value::Instance(instance) => {
            let mut updated = instance.clone();
            updated
                .fields
                .insert(field.to_string(), unsafe { take_value(new_value) });
            boxed_value(Value::Instance(updated))
        }
        other => runtime_error(format!(
            "cannot assign field `{}` on non-instance `{}`",
            field,
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_arg_buffer_new(count: i64) -> *mut i64 {
    let count = usize::try_from(count).unwrap_or_else(|_| runtime_error("invalid arg buffer size"));
    let mut values = vec![0i64; count].into_boxed_slice();
    let ptr = values.as_mut_ptr();
    Box::leak(values);
    ptr
}

#[no_mangle]
pub extern "C" fn aurora_direct_arg_buffer_store(
    buffer: *mut i64,
    index: i64,
    value: i64,
) {
    let index = usize::try_from(index).unwrap_or_else(|_| runtime_error("invalid arg index"));
    unsafe {
        *buffer.add(index) = value;
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_channel_new() -> *mut OpaqueValue {
    boxed_value(Value::Channel(ChannelValue::new()))
}

#[no_mangle]
pub extern "C" fn aurora_direct_task_group_new() -> *mut OpaqueValue {
    boxed_value(Value::TaskGroup(TaskGroupValue::new(&current_cancellation())))
}

#[no_mangle]
pub extern "C" fn aurora_direct_cancelled() -> i64 {
    if current_cancellation().is_cancelled() {
        1
    } else {
        0
    }
}

fn option_some(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Option".to_string(),
        variant_name: "Some".to_string(),
        payload: Some(Box::new(value)),
    })
}

fn option_none() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Option".to_string(),
        variant_name: "None".to_string(),
        payload: None,
    })
}

fn result_ok(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Result".to_string(),
        variant_name: "Ok".to_string(),
        payload: Some(Box::new(value)),
    })
}

fn result_err(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Result".to_string(),
        variant_name: "Err".to_string(),
        payload: Some(Box::new(value)),
    })
}

fn send_error_closed(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SendError".to_string(),
        variant_name: "Closed".to_string(),
        payload: Some(Box::new(value)),
    })
}

#[no_mangle]
pub extern "C" fn aurora_direct_channel_send(
    channel: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    match unsafe { value_ref(channel) } {
        Value::Channel(channel) => match channel.send(unsafe { take_value(value) }) {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(value) => boxed_value(result_err(send_error_closed(value))),
        },
        other => runtime_error(format!(
            "expected `Channel`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_channel_recv(channel: *mut OpaqueValue) -> *mut OpaqueValue {
    match unsafe { value_ref(channel) } {
        Value::Channel(channel) => boxed_value(match channel.recv_blocking() {
            Some(value) => option_some(value),
            None => option_none(),
        }),
        other => runtime_error(format!(
            "expected `Channel`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_channel_try_recv(channel: *mut OpaqueValue) -> i64 {
    match unsafe { value_ref(channel) } {
        Value::Channel(channel) => match channel.try_recv() {
            TryRecvResult::Value(value) => boxed_value(option_some(value)) as i64,
            TryRecvResult::Closed => boxed_value(option_none()) as i64,
            TryRecvResult::Empty => 0,
        },
        other => runtime_error(format!(
            "expected `Channel`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_channel_close(channel: *mut OpaqueValue) -> *mut OpaqueValue {
    match unsafe { value_ref(channel) } {
        Value::Channel(channel) => {
            channel.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `Channel`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_task_join(task: *mut OpaqueValue) -> *mut OpaqueValue {
    match unsafe { value_ref(task) } {
        Value::Task(task) => match task.join_result() {
            Ok(value) => boxed_value(value),
            Err(error) => runtime_error(error),
        },
        other => runtime_error(format!(
            "expected `Task`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_task_group_cancel(group: *mut OpaqueValue) -> *mut OpaqueValue {
    match unsafe { value_ref(group) } {
        Value::TaskGroup(group) => {
            group.cancel();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `TaskGroup`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_task_group_close(
    group: *mut OpaqueValue,
    cancel_before: i64,
) -> *mut OpaqueValue {
    match unsafe { value_ref(group) } {
        Value::TaskGroup(group) => {
            if cancel_before != 0 {
                group.cancel();
            }
            let mut first_error = None;
            for task in group.drain_tasks() {
                if let Err(error) = task.join_result() {
                    group.cancel();
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
            if let Some(error) = first_error {
                runtime_error(error);
            }
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `TaskGroup`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_deadline_new(duration: *mut OpaqueValue) -> i64 {
    let millis = extract_duration_millis(unsafe { value_ref(duration) });
    let millis = u64::try_from(millis).unwrap_or_else(|_| runtime_error("invalid deadline duration"));
    let deadline = Deadline(
        Instant::now()
            .checked_add(StdDuration::from_millis(millis))
            .unwrap_or_else(Instant::now),
    );
    Box::into_raw(Box::new(deadline)) as usize as i64
}

#[no_mangle]
pub extern "C" fn aurora_direct_deadline_ready(deadline: i64) -> i64 {
    let deadline = deadline as usize as *mut Deadline;
    if deadline.is_null() {
        return 1;
    }
    let ready = unsafe { Instant::now() >= (*deadline).0 };
    if ready { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn aurora_direct_sleep_ms(duration: i64) {
    let millis = u64::try_from(duration).unwrap_or_else(|_| runtime_error("invalid sleep duration"));
    thread::sleep(StdDuration::from_millis(millis));
}

#[no_mangle]
pub extern "C" fn aurora_direct_spawn_call(
    thunk_ptr: i64,
    args_ptr: *const i64,
    arg_count: i64,
    detached: i64,
    task_group: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let thunk: NativeThunk = unsafe { std::mem::transmute(thunk_ptr as usize) };
    let arg_count = usize::try_from(arg_count).unwrap_or_else(|_| runtime_error("invalid spawn arg count"));
    let args = unsafe { slice::from_raw_parts(args_ptr, arg_count) }.to_vec();
    let group = if task_group.is_null() {
        None
    } else {
        match unsafe { value_ref(task_group) } {
            Value::TaskGroup(group) => Some(group.clone()),
            other => runtime_error(format!(
                "expected `TaskGroup`, found `{}`",
                value_type_name(other)
            )),
        }
    };
    let cancellation = if let Some(group) = &group {
        group.child_cancellation()
    } else if detached != 0 {
        CancellationContext::default()
    } else {
        current_cancellation()
    };
    let handle = thread::spawn(move || {
        with_cancellation_scope(cancellation, || {
            let result_ptr = unsafe { thunk(args.as_ptr(), args.len()) };
            Ok(unsafe { take_value(result_ptr) })
        })
    });

    if detached != 0 {
        std::mem::drop(handle);
        return boxed_value(Value::Unit);
    }

    let task = TaskValue::from_handle(handle);
    if let Some(group) = group {
        group.register_task(task.clone());
    }
    boxed_value(Value::Task(task))
}

#[no_mangle]
pub extern "C" fn aurora_direct_sqrt_f64(value: f64) -> f64 {
    value.sqrt()
}

#[no_mangle]
pub extern "C" fn aurora_direct_fail_division_by_zero() -> ! {
    let _ = writeln!(io::stderr().lock(), "division by zero");
    process::exit(1);
}

#[no_mangle]
pub extern "C" fn aurora_direct_fail_int32_overflow(value: i64) -> ! {
    let _ = writeln!(io::stderr().lock(), "{}", int32_overflow_message(value));
    process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::{int32_overflow_message, render_bool, render_float};
    use std::process::Command;

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
    fn runtime_init_is_callable() {
        super::aurora_direct_runtime_init();
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
    fn division_by_zero_helper_exits_with_error() {
        if std::env::var("AURORA_DIRECT_RUNTIME_HELPER").as_deref() == Ok("divzero") {
            super::aurora_direct_fail_division_by_zero();
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
            super::aurora_direct_fail_int32_overflow(999);
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
}
