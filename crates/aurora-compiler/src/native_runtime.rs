use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::process;
use std::slice;
use std::str;
use std::sync::{Arc, OnceLock, RwLock};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use crate::ast::{BinaryOp, UnaryOp};
use crate::diag::{Diagnostic, Span};
use crate::integer::IntegerValue;
use crate::runtime_value::{
    cast_numeric_value, option_none, option_some, render_float, result_err, result_ok,
    send_error_closed, CancellationContext, ChannelValue, EnumVariantValue, InstanceValue,
    MapValue, RangeValue, SetValue, TaskGroupValue, TaskValue, TryRecvResult, Value, VecValue,
};
use crate::sema::Type;

fn write_stdout(text: &str) {
    let mut stdout = io::stdout().lock();
    let write_result = with_sigpipe_blocked(|| stdout.write_all(text.as_bytes()));
    let flush_result = if write_result.is_ok() {
        with_sigpipe_blocked(|| stdout.flush())
    } else {
        Ok(())
    };
    if let Some(error) = write_result.err().or_else(|| flush_result.err()) {
        if error.kind() == io::ErrorKind::BrokenPipe {
            process::exit(0);
        }
        let _ = writeln!(io::stderr().lock(), "failed to write to stdout: {}", error);
        process::exit(1);
    }
}

#[cfg(unix)]
fn with_sigpipe_blocked<T>(f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    unsafe {
        let mut sigpipe_set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut sigpipe_set);
        libc::sigaddset(&mut sigpipe_set, libc::SIGPIPE);

        let mut old_mask: libc::sigset_t = std::mem::zeroed();
        if libc::pthread_sigmask(libc::SIG_BLOCK, &sigpipe_set, &mut old_mask) != 0 {
            return f();
        }

        let result = f();
        if matches!(&result, Err(error) if error.kind() == io::ErrorKind::BrokenPipe) {
            let mut pending: libc::sigset_t = std::mem::zeroed();
            if libc::sigpending(&mut pending) == 0
                && libc::sigismember(&pending, libc::SIGPIPE) == 1
            {
                let mut received = 0;
                let _ = libc::sigwait(&sigpipe_set, &mut received);
            }
            return result;
        }

        let _ = libc::pthread_sigmask(libc::SIG_SETMASK, &old_mask, std::ptr::null_mut());
        result
    }
}

#[cfg(not(unix))]
fn with_sigpipe_blocked<T>(f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    f()
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

#[repr(transparent)]
pub struct OpaqueValue(RwLock<Value>);

type NativeThunk = unsafe extern "C" fn(*const i64, usize) -> *mut OpaqueValue;

struct ProgramSourceContext {
    path: String,
    source: String,
}

thread_local! {
    static DIRECT_CANCELLATION: RefCell<CancellationContext> =
        RefCell::new(CancellationContext::default());
}

static DIRECT_PROGRAM_SOURCE: OnceLock<ProgramSourceContext> = OnceLock::new();

#[repr(transparent)]
struct Deadline(Instant);

fn current_cancellation() -> CancellationContext {
    DIRECT_CANCELLATION.with(|slot| slot.borrow().clone())
}

fn with_cancellation_scope<T>(cancellation: CancellationContext, work: impl FnOnce() -> T) -> T {
    DIRECT_CANCELLATION.with(|slot| {
        let previous = slot.replace(cancellation);
        let result = work();
        slot.replace(previous);
        result
    })
}

fn extract_duration_millis(value: impl Borrow<Value>) -> i128 {
    match value.borrow() {
        Value::Int(value) => match value.as_i128() {
            Some(value) => value,
            None => {
                runtime_error("expected `Duration`, found an integer outside signed timer range")
            }
        },
        Value::Duration(value) => *value,
        other => runtime_error(format!(
            "expected `Duration`, found `{}`",
            value_type_name(other)
        )),
    }
}

fn boxed_value(value: Value) -> *mut OpaqueValue {
    Arc::into_raw(Arc::new(OpaqueValue(RwLock::new(value)))) as *mut OpaqueValue
}

unsafe fn with_value<T>(ptr: *mut OpaqueValue, read: impl FnOnce(&Value) -> T) -> T {
    let value = ptr
        .as_ref()
        .unwrap_or_else(|| runtime_error("direct runtime received a null opaque value pointer"));
    let guard = value
        .0
        .read()
        .unwrap_or_else(|_| runtime_error("direct runtime value lock was poisoned"));
    read(&guard)
}

unsafe fn value_ref(ptr: *mut OpaqueValue) -> Value {
    with_value(ptr, Clone::clone)
}

unsafe fn value_mut<T>(ptr: *mut OpaqueValue, write: impl FnOnce(&mut Value) -> T) -> T {
    let value = ptr
        .as_ref()
        .unwrap_or_else(|| runtime_error("direct runtime received a null opaque value pointer"));
    let mut guard = value
        .0
        .write()
        .unwrap_or_else(|_| runtime_error("direct runtime value lock was poisoned"));
    write(&mut guard)
}

unsafe fn take_value(ptr: *mut OpaqueValue) -> Value {
    value_ref(ptr)
}

unsafe fn consume_value(ptr: *mut OpaqueValue) -> Value {
    let value = value_ref(ptr);
    aurora_direct_release_value(ptr);
    value
}

fn decode_bytes(ptr: *const u8, len: usize) -> String {
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    str::from_utf8(bytes)
        .unwrap_or_else(|_| runtime_error("aurora direct runtime received invalid UTF-8 bytes"))
        .to_string()
}

fn render_runtime_diagnostic(diagnostic: Diagnostic) -> String {
    if let Some(context) = DIRECT_PROGRAM_SOURCE.get() {
        diagnostic.render_with_source(&context.path, &context.source)
    } else {
        format!("error: {}", diagnostic.message)
    }
}

fn runtime_error(message: impl AsRef<str>) -> ! {
    let _ = writeln!(
        io::stderr().lock(),
        "{}",
        render_runtime_diagnostic(Diagnostic::new(message.as_ref()))
    );
    process::exit(1);
}

fn runtime_error_at(span: Span, message: impl AsRef<str>) -> ! {
    let _ = writeln!(
        io::stderr().lock(),
        "{}",
        render_runtime_diagnostic(Diagnostic::at(span, message.as_ref()))
    );
    process::exit(1);
}

fn runtime_span(line: i64, column: i64) -> Option<Span> {
    if line <= 0 || column <= 0 {
        return None;
    }
    Some(Span::new(line as usize, column as usize))
}

fn value_type_name(value: impl Borrow<Value>) -> String {
    match value.borrow() {
        Value::Int(_) => "integer".to_string(),
        Value::Float(_) => "float64".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::String(_) => "String".to_string(),
        Value::Vec(_) => "Vec".to_string(),
        Value::Set(_) => "Set".to_string(),
        Value::Map(_) => "Map".to_string(),
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
    if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
        return Ok(Value::Bool(match op {
            BinaryOp::Eq => left == right,
            BinaryOp::NotEq => left != right,
            _ => unreachable!("equality branch only handles `==` and `!=`"),
        }));
    }
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(Value::Bool(match op {
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
        (Value::String(left), Value::String(right)) => Ok(Value::Bool(match op {
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
            (Value::Int(left), Value::Int(right)) => match left.checked_add(right) {
                Some(value) => Ok(Value::Int(value)),
                None => Err(Diagnostic::new("integer overflow")),
            },
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left + right)),
            (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
            (left, right) => Err(Diagnostic::new(format!(
                "unsupported `+` operands `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
        BinaryOp::Sub => match (left, right) {
            (Value::Int(left), Value::Int(right)) => match left.checked_sub(right) {
                Some(value) => Ok(Value::Int(value)),
                None => Err(Diagnostic::new("integer overflow")),
            },
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left - right)),
            (left, right) => Err(Diagnostic::new(format!(
                "unsupported `-` operands `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
        BinaryOp::Mul => match (left, right) {
            (Value::Int(left), Value::Int(right)) => match left.checked_mul(right) {
                Some(value) => Ok(Value::Int(value)),
                None => Err(Diagnostic::new("integer overflow")),
            },
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left * right)),
            (left, right) => Err(Diagnostic::new(format!(
                "unsupported `*` operands `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
        BinaryOp::Div => match (left, right) {
            (Value::Int(_), Value::Int(right)) if right.is_zero() => {
                Err(Diagnostic::new("division by zero"))
            }
            (Value::Int(left), Value::Int(right)) => match left.checked_div(right) {
                Some(value) => Ok(Value::Int(value)),
                None => Err(Diagnostic::new("integer overflow")),
            },
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left / right)),
            (left, right) => Err(Diagnostic::new(format!(
                "unsupported `/` operands `{}` and `{}`",
                value_type_name(&left),
                value_type_name(&right)
            ))),
        },
        BinaryOp::Mod => match (left, right) {
            (Value::Int(_), Value::Int(right)) if right.is_zero() => {
                Err(Diagnostic::new("division by zero"))
            }
            (Value::Int(left), Value::Int(right)) => match left.checked_rem(right) {
                Some(value) => Ok(Value::Int(value)),
                None => Err(Diagnostic::new("integer overflow")),
            },
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
        (UnaryOp::Neg, Value::Int(value)) => match value.checked_neg() {
            Some(value) => Ok(Value::Int(value)),
            None => Err(Diagnostic::new("integer overflow")),
        },
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
pub extern "C" fn aurora_direct_runtime_init(
    path_ptr: *const u8,
    path_len: usize,
    source_ptr: *const u8,
    source_len: usize,
) {
    let _ = DIRECT_PROGRAM_SOURCE.set(ProgramSourceContext {
        path: decode_bytes(path_ptr, path_len),
        source: decode_bytes(source_ptr, source_len),
    });
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
    boxed_value(Value::Int(IntegerValue::from_signed(value as i128)))
}

#[no_mangle]
pub extern "C" fn aurora_direct_box_uint_literal(ptr: *const u8, len: usize) -> *mut OpaqueValue {
    let text = decode_bytes(ptr, len);
    let value = match text.parse::<u128>() {
        Ok(value) => value,
        Err(_) => runtime_error(format!("invalid embedded uint literal `{}`", text)),
    };
    boxed_value(Value::Int(IntegerValue::from_literal(value)))
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
pub extern "C" fn aurora_direct_retain_value(value: *mut OpaqueValue) -> *mut OpaqueValue {
    if !value.is_null() {
        unsafe { Arc::increment_strong_count(value as *const OpaqueValue) };
    }
    value
}

#[no_mangle]
pub extern "C" fn aurora_direct_release_value(value: *mut OpaqueValue) {
    if !value.is_null() {
        unsafe {
            drop(Arc::from_raw(value as *const OpaqueValue));
        }
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_literal(ptr: *const u8, len: usize) -> *mut OpaqueValue {
    boxed_value(Value::String(decode_bytes(ptr, len)))
}

#[no_mangle]
pub extern "C" fn aurora_direct_stringify_value(value: *mut OpaqueValue) -> *mut OpaqueValue {
    let rendered = unsafe { value_ref(value) }.render();
    boxed_value(Value::String(rendered))
}

#[no_mangle]
pub extern "C" fn aurora_direct_duration_literal(value: i64) -> *mut OpaqueValue {
    boxed_value(Value::Duration(value as i128))
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_len(value: *mut OpaqueValue) -> i64 {
    match unsafe { value_ref(value) } {
        Value::String(text) => match i64::try_from(text.len()) {
            Ok(length) => length,
            Err(_) => runtime_error("string length does not fit in the direct runtime range"),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_contains(
    value: *mut OpaqueValue,
    needle: *mut OpaqueValue,
) -> i64 {
    let Value::String(needle) = (unsafe { take_value(needle) }) else {
        runtime_error("`contains` requires a `String` argument");
    };
    match unsafe { value_ref(value) } {
        Value::String(text) => i64::from(text.contains(&needle)),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_starts_with(
    value: *mut OpaqueValue,
    prefix: *mut OpaqueValue,
) -> i64 {
    let Value::String(prefix) = (unsafe { take_value(prefix) }) else {
        runtime_error("`starts_with` requires a `String` argument");
    };
    match unsafe { value_ref(value) } {
        Value::String(text) => i64::from(text.starts_with(&prefix)),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_ends_with(
    value: *mut OpaqueValue,
    suffix: *mut OpaqueValue,
) -> i64 {
    let Value::String(suffix) = (unsafe { take_value(suffix) }) else {
        runtime_error("`ends_with` requires a `String` argument");
    };
    match unsafe { value_ref(value) } {
        Value::String(text) => i64::from(text.ends_with(&suffix)),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_split(
    value: *mut OpaqueValue,
    separator: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let Value::String(separator) = (unsafe { take_value(separator) }) else {
        runtime_error("`split` requires a `String` argument");
    };
    match unsafe { value_ref(value) } {
        Value::String(text) => boxed_value(Value::Vec(VecValue {
            element_type: Type::named("String"),
            elements: text
                .split(&separator)
                .map(|part| Value::String(part.to_string()))
                .collect(),
        })),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_replace(
    value: *mut OpaqueValue,
    from: *mut OpaqueValue,
    to: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let Value::String(from) = (unsafe { take_value(from) }) else {
        runtime_error("`replace` requires `String` for `from`");
    };
    let Value::String(to) = (unsafe { take_value(to) }) else {
        runtime_error("`replace` requires `String` for `to`");
    };
    match unsafe { value_ref(value) } {
        Value::String(text) => boxed_value(Value::String(text.replace(&from, &to))),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_to_lower(value: *mut OpaqueValue) -> *mut OpaqueValue {
    match unsafe { value_ref(value) } {
        Value::String(text) => boxed_value(Value::String(text.to_lowercase())),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_to_upper(value: *mut OpaqueValue) -> *mut OpaqueValue {
    match unsafe { value_ref(value) } {
        Value::String(text) => boxed_value(Value::String(text.to_uppercase())),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_strip_prefix(
    value: *mut OpaqueValue,
    prefix: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let Value::String(prefix) = (unsafe { take_value(prefix) }) else {
        runtime_error("`strip_prefix` requires a `String` argument");
    };
    match unsafe { value_ref(value) } {
        Value::String(text) => boxed_value(
            text.strip_prefix(&prefix)
                .map(|rest| option_some(Value::String(rest.to_string())))
                .unwrap_or_else(option_none),
        ),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_strip_suffix(
    value: *mut OpaqueValue,
    suffix: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let Value::String(suffix) = (unsafe { take_value(suffix) }) else {
        runtime_error("`strip_suffix` requires a `String` argument");
    };
    match unsafe { value_ref(value) } {
        Value::String(text) => boxed_value(
            text.strip_suffix(&suffix)
                .map(|rest| option_some(Value::String(rest.to_string())))
                .unwrap_or_else(option_none),
        ),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_trim(value: *mut OpaqueValue) -> *mut OpaqueValue {
    match unsafe { value_ref(value) } {
        Value::String(text) => boxed_value(Value::String(text.trim().to_string())),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_string_join(
    separator: *mut OpaqueValue,
    parts: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let Value::Vec(parts) = (unsafe { take_value(parts) }) else {
        runtime_error("`join` requires `Vec[String]`");
    };
    match unsafe { value_ref(separator) } {
        Value::String(separator) => {
            let mut rendered_parts = Vec::new();
            for value in parts.elements {
                let Value::String(part) = value else {
                    runtime_error("`join` requires `Vec[String]`");
                };
                rendered_parts.push(part);
            }
            boxed_value(Value::String(rendered_parts.join(&separator)))
        }
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_abs(value: *mut OpaqueValue) -> *mut OpaqueValue {
    let value = unsafe { take_value(value) };
    match value {
        Value::Int(IntegerValue::Signed(value)) => value
            .checked_abs()
            .map(IntegerValue::from_signed)
            .map(Value::Int)
            .map(boxed_value)
            .unwrap_or_else(|| runtime_error("`abs(...)` overflowed the signed integer range")),
        Value::Int(IntegerValue::Unsigned(value)) => {
            boxed_value(Value::Int(IntegerValue::Unsigned(value)))
        }
        Value::Float(value) => boxed_value(Value::Float(value.abs())),
        other => runtime_error(format!(
            "`abs(...)` expects an integer or float value, found `{}`",
            value_type_name(&other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_min(
    left: *mut OpaqueValue,
    right: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let left = unsafe { take_value(left) };
    let right = unsafe { take_value(right) };
    let value = match (&left, &right) {
        (Value::Int(left_value), Value::Int(right_value)) => {
            if left_value <= right_value {
                left
            } else {
                right
            }
        }
        (Value::Float(left_value), Value::Float(right_value)) => {
            if left_value <= right_value {
                left
            } else {
                right
            }
        }
        _ => runtime_error("`min(...)` expects matching numeric arguments"),
    };
    boxed_value(value)
}

#[no_mangle]
pub extern "C" fn aurora_direct_max(
    left: *mut OpaqueValue,
    right: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let left = unsafe { take_value(left) };
    let right = unsafe { take_value(right) };
    let value = match (&left, &right) {
        (Value::Int(left_value), Value::Int(right_value)) => {
            if left_value >= right_value {
                left
            } else {
                right
            }
        }
        (Value::Float(left_value), Value::Float(right_value)) => {
            if left_value >= right_value {
                left
            } else {
                right
            }
        }
        _ => runtime_error("`max(...)` expects matching numeric arguments"),
    };
    boxed_value(value)
}

#[no_mangle]
pub extern "C" fn aurora_direct_sqrt(value: *mut OpaqueValue) -> *mut OpaqueValue {
    let value = unsafe { take_value(value) };
    match value {
        Value::Float(value) => boxed_value(Value::Float(value.sqrt())),
        other => runtime_error(format!(
            "`sqrt(...)` expects `float32` or `float64`, found `{}`",
            value_type_name(&other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_parse_int32(value: *mut OpaqueValue) -> *mut OpaqueValue {
    let value = unsafe { take_value(value) };
    match value {
        Value::String(text) => match text.parse::<i32>() {
            Ok(value) => boxed_value(result_ok(Value::Int(IntegerValue::from_signed(
                value as i128,
            )))),
            Err(error) => boxed_value(result_err(Value::String(error.to_string()))),
        },
        other => runtime_error(format!(
            "`parse_int32(...)` expects `String`, found `{}`",
            value_type_name(&other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_parse_int64(value: *mut OpaqueValue) -> *mut OpaqueValue {
    let value = unsafe { take_value(value) };
    match value {
        Value::String(text) => match text.parse::<i64>() {
            Ok(value) => boxed_value(result_ok(Value::Int(IntegerValue::from_signed(
                value as i128,
            )))),
            Err(error) => boxed_value(result_err(Value::String(error.to_string()))),
        },
        other => runtime_error(format!(
            "`parse_int64(...)` expects `String`, found `{}`",
            value_type_name(&other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_parse_float64(value: *mut OpaqueValue) -> *mut OpaqueValue {
    let value = unsafe { take_value(value) };
    match value {
        Value::String(text) => match text.parse::<f64>() {
            Ok(value) => boxed_value(result_ok(Value::Float(value))),
            Err(error) => boxed_value(result_err(Value::String(error.to_string()))),
        },
        other => runtime_error(format!(
            "`parse_float64(...)` expects `String`, found `{}`",
            value_type_name(&other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_range_new(start: i64, end: i64) -> *mut OpaqueValue {
    boxed_value(Value::Range(RangeValue {
        start: start as i128,
        end: end as i128,
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_range_current(range: *mut OpaqueValue) -> i64 {
    match unsafe { value_ref(range) } {
        Value::Range(range) => match i64::try_from(range.start) {
            Ok(start) => start,
            Err(_) => runtime_error("range start is outside host i64 bounds"),
        },
        other => runtime_error(format!(
            "expected `Range`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_range_end(range: *mut OpaqueValue) -> i64 {
    match unsafe { value_ref(range) } {
        Value::Range(range) => match i64::try_from(range.end) {
            Ok(end) => end,
            Err(_) => runtime_error("range end is outside host i64 bounds"),
        },
        other => runtime_error(format!(
            "expected `Range`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_range_advance(range: *mut OpaqueValue) -> *mut OpaqueValue {
    match unsafe { value_ref(range) } {
        Value::Range(range) => boxed_value(Value::Range(RangeValue {
            start: range.start + 1,
            end: range.end,
        })),
        other => runtime_error(format!(
            "expected `Range`, found `{}`",
            value_type_name(other)
        )),
    }
}

fn with_vector<T>(ptr: *mut OpaqueValue, read: impl FnOnce(&VecValue) -> T) -> T {
    unsafe {
        with_value(ptr, |value| match value {
            Value::Vec(vector) => read(vector),
            other => runtime_error(format!(
                "expected `Vec`, found `{}`",
                value_type_name(other)
            )),
        })
    }
}

fn with_vector_mut<T>(ptr: *mut OpaqueValue, write: impl FnOnce(&mut VecValue) -> T) -> T {
    unsafe {
        value_mut(ptr, |value| match value {
            Value::Vec(vector) => write(vector),
            other => runtime_error(format!(
                "expected `Vec`, found `{}`",
                value_type_name(other)
            )),
        })
    }
}

fn with_map<T>(ptr: *mut OpaqueValue, read: impl FnOnce(&MapValue) -> T) -> T {
    unsafe {
        with_value(ptr, |value| match value {
            Value::Map(map) => read(map),
            other => runtime_error(format!(
                "expected `Map`, found `{}`",
                value_type_name(other)
            )),
        })
    }
}

fn with_map_mut<T>(ptr: *mut OpaqueValue, write: impl FnOnce(&mut MapValue) -> T) -> T {
    unsafe {
        value_mut(ptr, |value| match value {
            Value::Map(map) => write(map),
            other => runtime_error(format!(
                "expected `Map`, found `{}`",
                value_type_name(other)
            )),
        })
    }
}

fn with_set<T>(ptr: *mut OpaqueValue, read: impl FnOnce(&SetValue) -> T) -> T {
    unsafe {
        with_value(ptr, |value| match value {
            Value::Set(set) => read(set),
            other => runtime_error(format!(
                "expected `Set`, found `{}`",
                value_type_name(other)
            )),
        })
    }
}

fn with_set_mut<T>(ptr: *mut OpaqueValue, write: impl FnOnce(&mut SetValue) -> T) -> T {
    unsafe {
        value_mut(ptr, |value| match value {
            Value::Set(set) => write(set),
            other => runtime_error(format!(
                "expected `Set`, found `{}`",
                value_type_name(other)
            )),
        })
    }
}

fn checked_vec_index(index: i64) -> usize {
    if index < 0 {
        runtime_error(format!("vector index `{}` cannot be negative", index));
    }
    match usize::try_from(index) {
        Ok(index) => index,
        Err(_) => runtime_error("vector index does not fit in the runtime address space"),
    }
}

fn checked_vec_index_at(index: i64, line: i64, column: i64) -> usize {
    if index < 0 {
        match runtime_span(line, column) {
            Some(span) => {
                runtime_error_at(span, format!("vector index `{}` cannot be negative", index))
            }
            None => runtime_error(format!("vector index `{}` cannot be negative", index)),
        }
    }
    match usize::try_from(index) {
        Ok(index) => index,
        Err(_) => match runtime_span(line, column) {
            Some(span) => runtime_error_at(
                span,
                "vector index does not fit in the runtime address space",
            ),
            None => runtime_error("vector index does not fit in the runtime address space"),
        },
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_empty() -> *mut OpaqueValue {
    boxed_value(Value::Vec(VecValue {
        element_type: Type::named("Unknown"),
        elements: Vec::new(),
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_len(vec: *mut OpaqueValue) -> i64 {
    match i64::try_from(with_vector(vec, |vector| vector.elements.len())) {
        Ok(length) => length,
        Err(_) => runtime_error("vector length does not fit in the direct runtime range"),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_is_empty(vec: *mut OpaqueValue) -> i64 {
    i64::from(with_vector(vec, |vector| vector.elements.is_empty()))
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_push_in_place(
    vec: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let value = unsafe { take_value(value) };
    with_vector_mut(vec, |vector| vector.elements.push(value));
    boxed_value(Value::Unit)
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_pop_in_place(vec: *mut OpaqueValue) -> *mut OpaqueValue {
    let value = with_vector_mut(vec, |vector| vector.elements.pop());
    match value {
        Some(value) => boxed_value(option_some(value)),
        None => boxed_value(option_none()),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_get(vec: *mut OpaqueValue, index: i64) -> *mut OpaqueValue {
    let index = checked_vec_index(index);
    let value = with_vector(vec, |vector| vector.elements.get(index).cloned());
    boxed_value(value.map(option_some).unwrap_or_else(option_none))
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_set_in_place(
    vec: *mut OpaqueValue,
    index: i64,
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let index = checked_vec_index(index);
    let value = unsafe { take_value(value) };
    let previous = with_vector_mut(vec, |vector| {
        if index < vector.elements.len() {
            Some(std::mem::replace(&mut vector.elements[index], value))
        } else {
            None
        }
    });
    boxed_value(previous.map(option_some).unwrap_or_else(option_none))
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_remove_in_place(
    vec: *mut OpaqueValue,
    index: i64,
) -> *mut OpaqueValue {
    let index = checked_vec_index(index);
    let previous = with_vector_mut(vec, |vector| {
        if index < vector.elements.len() {
            Some(vector.elements.remove(index))
        } else {
            None
        }
    });
    boxed_value(previous.map(option_some).unwrap_or_else(option_none))
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_swap_in_place(
    vec: *mut OpaqueValue,
    first: i64,
    second: i64,
) -> i64 {
    let first = checked_vec_index(first);
    let second = checked_vec_index(second);
    let swapped = with_vector_mut(vec, |vector| {
        let swapped = first < vector.elements.len() && second < vector.elements.len();
        if swapped {
            vector.elements.swap(first, second);
        }
        swapped
    });
    i64::from(swapped)
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_contains(
    vec: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> i64 {
    let needle = unsafe { take_value(value) };
    i64::from(with_vector(vec, |vector| {
        vector.elements.iter().any(|candidate| *candidate == needle)
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_insert_in_place(
    vec: *mut OpaqueValue,
    index: i64,
    value: *mut OpaqueValue,
) -> i64 {
    let index = checked_vec_index(index);
    let value = unsafe { take_value(value) };
    let inserted = with_vector_mut(vec, |vector| {
        let inserted = index <= vector.elements.len();
        if inserted {
            vector.elements.insert(index, value);
        }
        inserted
    });
    i64::from(inserted)
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_clear_in_place(vec: *mut OpaqueValue) -> *mut OpaqueValue {
    with_vector_mut(vec, |vector| vector.elements.clear());
    boxed_value(Value::Unit)
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_reverse_in_place(vec: *mut OpaqueValue) -> *mut OpaqueValue {
    with_vector_mut(vec, |vector| vector.elements.reverse());
    boxed_value(Value::Unit)
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_extend_in_place(
    vec: *mut OpaqueValue,
    other: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let other = unsafe { take_value(other) };
    let Value::Vec(other) = other else {
        runtime_error("`extend` requires another `Vec[T]` value");
    };
    with_vector_mut(vec, |vector| vector.elements.extend(other.elements));
    boxed_value(Value::Unit)
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_index(
    vec: *mut OpaqueValue,
    index: i64,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    let index = checked_vec_index_at(index, line, column);
    let (value, len) = with_vector(vec, |vector| {
        (vector.elements.get(index).cloned(), vector.elements.len())
    });
    let Some(value) = value else {
        match runtime_span(line, column) {
            Some(span) => runtime_error_at(
                span,
                format!(
                    "vector index `{}` is out of bounds for length `{}`",
                    index, len
                ),
            ),
            None => runtime_error(format!(
                "vector index `{}` is out of bounds for length `{}`",
                index, len
            )),
        }
    };
    boxed_value(value)
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_index_option(
    vec: *mut OpaqueValue,
    index: i64,
) -> *mut OpaqueValue {
    let index = checked_vec_index(index);
    let value = with_vector(vec, |vector| vector.elements.get(index).cloned());
    boxed_value(value.map(option_some).unwrap_or_else(option_none))
}

#[no_mangle]
pub extern "C" fn aurora_direct_vec_set_index_in_place(
    vec: *mut OpaqueValue,
    index: i64,
    value: *mut OpaqueValue,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    let index = checked_vec_index_at(index, line, column);
    let value = unsafe { take_value(value) };
    let result = with_vector_mut(vec, |vector| {
        if index >= vector.elements.len() {
            Err(vector.elements.len())
        } else {
            vector.elements[index] = value;
            Ok(())
        }
    });
    if let Err(len) = result {
        match runtime_span(line, column) {
            Some(span) => runtime_error_at(
                span,
                format!(
                    "vector index `{}` is out of bounds for length `{}`",
                    index, len
                ),
            ),
            None => runtime_error(format!(
                "vector index `{}` is out of bounds for length `{}`",
                index, len
            )),
        }
    }
    boxed_value(Value::Unit)
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_empty() -> *mut OpaqueValue {
    boxed_value(Value::Map(MapValue {
        key_type: Type::named("Unknown"),
        value_type: Type::named("Unknown"),
        entries: Vec::new(),
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_len(map: *mut OpaqueValue) -> i64 {
    match i64::try_from(with_map(map, |map| map.entries.len())) {
        Ok(length) => length,
        Err(_) => runtime_error("map length does not fit in the direct runtime range"),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_is_empty(map: *mut OpaqueValue) -> i64 {
    i64::from(with_map(map, |map| map.entries.is_empty()))
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_get(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let key = unsafe { take_value(key) };
    let value = with_map(map, |map| {
        map.entries
            .iter()
            .find(|(candidate_key, _)| *candidate_key == key)
            .map(|(_, value)| value.clone())
    });
    boxed_value(value.map(option_some).unwrap_or_else(option_none))
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_set_in_place(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let key = unsafe { take_value(key) };
    let value = unsafe { take_value(value) };
    let previous = with_map_mut(map, |map| {
        if let Some(index) = map
            .entries
            .iter()
            .position(|(candidate_key, _)| *candidate_key == key)
        {
            Some(std::mem::replace(&mut map.entries[index].1, value))
        } else {
            map.entries.push((key, value));
            None
        }
    });
    boxed_value(previous.map(option_some).unwrap_or_else(option_none))
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_remove_in_place(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let key = unsafe { take_value(key) };
    let previous = with_map_mut(map, |map| {
        if let Some(index) = map
            .entries
            .iter()
            .position(|(candidate_key, _)| *candidate_key == key)
        {
            Some(map.entries.remove(index).1)
        } else {
            None
        }
    });
    boxed_value(previous.map(option_some).unwrap_or_else(option_none))
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_contains_key(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
) -> i64 {
    let key = unsafe { take_value(key) };
    i64::from(with_map(map, |map| {
        map.entries
            .iter()
            .any(|(candidate_key, _)| *candidate_key == key)
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_keys(map: *mut OpaqueValue) -> *mut OpaqueValue {
    let (key_type, elements) = with_map(map, |map| {
        (
            map.key_type.clone(),
            map.entries
                .iter()
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>(),
        )
    });
    boxed_value(Value::Vec(VecValue {
        element_type: key_type,
        elements,
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_values(map: *mut OpaqueValue) -> *mut OpaqueValue {
    let (value_type, elements) = with_map(map, |map| {
        (
            map.value_type.clone(),
            map.entries
                .iter()
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>(),
        )
    });
    boxed_value(Value::Vec(VecValue {
        element_type: value_type,
        elements,
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_items(map: *mut OpaqueValue) -> *mut OpaqueValue {
    let (element_type, elements) = with_map(map, |map| {
        (
            Type::Named(
                "MapEntry".to_string(),
                vec![map.key_type.clone(), map.value_type.clone()],
            ),
            map.entries
                .iter()
                .map(|(key, value)| {
                    Value::Instance(InstanceValue {
                        class_name: "MapEntry".to_string(),
                        fields: BTreeMap::from([
                            ("key".to_string(), key.clone()),
                            ("value".to_string(), value.clone()),
                        ]),
                    })
                })
                .collect::<Vec<_>>(),
        )
    });
    boxed_value(Value::Vec(VecValue {
        element_type,
        elements,
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_entries(map: *mut OpaqueValue) -> *mut OpaqueValue {
    aurora_direct_map_items(map)
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_index(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    let key = unsafe { take_value(key) };
    let value = with_map(map, |map| {
        map.entries
            .iter()
            .find(|(candidate_key, _)| *candidate_key == key)
            .map(|(_, value)| value.clone())
    });
    let Some(value) = value else {
        match runtime_span(line, column) {
            Some(span) => {
                runtime_error_at(span, format!("map key `{}` was not present", key.render()))
            }
            None => runtime_error(format!("map key `{}` was not present", key.render())),
        }
    };
    boxed_value(value)
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_set_index_in_place(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
    value: *mut OpaqueValue,
    _line: i64,
    _column: i64,
) -> *mut OpaqueValue {
    let key = unsafe { take_value(key) };
    let value = unsafe { take_value(value) };
    with_map_mut(map, |map| {
        if let Some(index) = map
            .entries
            .iter()
            .position(|(candidate_key, _)| *candidate_key == key)
        {
            map.entries[index].1 = value;
        } else {
            map.entries.push((key, value));
        }
    });
    boxed_value(Value::Unit)
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_clear_in_place(map: *mut OpaqueValue) -> *mut OpaqueValue {
    with_map_mut(map, |map| map.entries.clear());
    boxed_value(Value::Unit)
}

#[no_mangle]
pub extern "C" fn aurora_direct_map_extend_in_place(
    map: *mut OpaqueValue,
    other: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let other = unsafe { take_value(other) };
    let Value::Map(other) = other else {
        runtime_error("`extend` requires another `Map[K, V]` value");
    };
    with_map_mut(map, |map| {
        for (key, value) in other.entries {
            if let Some(index) = map
                .entries
                .iter()
                .position(|(candidate_key, _)| *candidate_key == key)
            {
                map.entries[index].1 = value;
            } else {
                map.entries.push((key, value));
            }
        }
    });
    boxed_value(Value::Unit)
}

#[no_mangle]
pub extern "C" fn aurora_direct_set_empty() -> *mut OpaqueValue {
    boxed_value(Value::Set(SetValue {
        element_type: Type::named("Unknown"),
        elements: Vec::new(),
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_set_len(set: *mut OpaqueValue) -> i64 {
    match i64::try_from(with_set(set, |set| set.elements.len())) {
        Ok(length) => length,
        Err(_) => runtime_error("set length does not fit in the direct runtime range"),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_set_is_empty(set: *mut OpaqueValue) -> i64 {
    i64::from(with_set(set, |set| set.elements.is_empty()))
}

#[no_mangle]
pub extern "C" fn aurora_direct_set_contains(
    set: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> i64 {
    let needle = unsafe { take_value(value) };
    i64::from(with_set(set, |set| {
        set.elements.iter().any(|candidate| *candidate == needle)
    }))
}

#[no_mangle]
pub extern "C" fn aurora_direct_set_insert_in_place(
    set: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> i64 {
    let value = unsafe { take_value(value) };
    let inserted = with_set_mut(set, |set| {
        if set.elements.iter().any(|candidate| *candidate == value) {
            false
        } else {
            set.elements.push(value);
            true
        }
    });
    i64::from(inserted)
}

#[no_mangle]
pub extern "C" fn aurora_direct_set_remove_in_place(
    set: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> i64 {
    let value = unsafe { take_value(value) };
    let removed = with_set_mut(set, |set| {
        if let Some(index) = set
            .elements
            .iter()
            .position(|candidate| *candidate == value)
        {
            set.elements.remove(index);
            true
        } else {
            false
        }
    });
    i64::from(removed)
}

#[no_mangle]
pub extern "C" fn aurora_direct_set_index_option(
    set: *mut OpaqueValue,
    index: i64,
) -> *mut OpaqueValue {
    let index = checked_vec_index(index);
    let value = with_set(set, |set| set.elements.get(index).cloned());
    boxed_value(value.map(option_some).unwrap_or_else(option_none))
}

#[no_mangle]
pub extern "C" fn aurora_direct_clone_value(value: *mut OpaqueValue) -> *mut OpaqueValue {
    boxed_value(unsafe { value_ref(value) })
}

#[no_mangle]
pub extern "C" fn aurora_direct_unbox_i64(value: *mut OpaqueValue) -> i64 {
    match unsafe { value_ref(value) } {
        Value::Int(value) => match value.as_i128().and_then(|value| i64::try_from(value).ok()) {
            Some(value) => value,
            None => runtime_error("direct backend expected an integer that fits in host i64"),
        },
        other => runtime_error(format!(
            "direct backend expected `int32`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_unbox_f64(value: *mut OpaqueValue) -> f64 {
    match unsafe { value_ref(value) } {
        Value::Float(value) => value,
        other => runtime_error(format!(
            "direct backend expected `float64`, found `{}`",
            value_type_name(other)
        )),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_unbox_bool(value: *mut OpaqueValue) -> i64 {
    match unsafe { value_ref(value) } {
        Value::Bool(value) => i64::from(value),
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
        Value::Bool(value) => i64::from(value),
        Value::Int(value) => i64::from(!value.is_zero()),
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
pub extern "C" fn aurora_direct_unary_value_at(
    op: i32,
    value: *mut OpaqueValue,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    let op = match op {
        0 => UnaryOp::Neg,
        1 => UnaryOp::Not,
        other => runtime_error(format!("unknown unary opcode `{}`", other)),
    };
    match eval_unary_value(unsafe { take_value(value) }, op) {
        Ok(value) => boxed_value(value),
        Err(error) => match runtime_span(line, column) {
            Some(span) => runtime_error_at(span, error.message),
            None => runtime_error(error.message),
        },
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
    match eval_binary_value(
        unsafe { take_value(left) },
        unsafe { take_value(right) },
        op,
    ) {
        Ok(value) => boxed_value(value),
        Err(error) => runtime_error(error.message),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_binary_value_at(
    op: i32,
    left: *mut OpaqueValue,
    right: *mut OpaqueValue,
    line: i64,
    column: i64,
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
    match eval_binary_value(
        unsafe { take_value(left) },
        unsafe { take_value(right) },
        op,
    ) {
        Ok(value) => boxed_value(value),
        Err(error) => match runtime_span(line, column) {
            Some(span) => runtime_error_at(span, error.message),
            None => runtime_error(error.message),
        },
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
pub extern "C" fn aurora_direct_cast_value_at(
    value: *mut OpaqueValue,
    target_ptr: *const u8,
    target_len: usize,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    let target = Type::named(decode_bytes(target_ptr, target_len));
    match cast_numeric_value(unsafe { take_value(value) }, &target, None) {
        Ok(value) => boxed_value(value),
        Err(error) => match runtime_span(line, column) {
            Some(span) => runtime_error_at(span, error.message),
            None => runtime_error(error.message),
        },
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
    let matches = match &actual {
        Value::Instance(instance) => instance.class_name == expected,
        Value::EnumVariant(variant) => variant.enum_name == expected,
        Value::String(_) => expected == "String",
        Value::Vec(_) => expected == "Vec",
        Value::Set(_) => expected == "Set",
        Value::Map(_) => expected == "Map",
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
        enum_name: decode_bytes(enum_ptr, enum_len),
        variant_name: decode_bytes(variant_ptr, variant_len),
        payloads: if payload.is_null() {
            Vec::new()
        } else {
            vec![unsafe { take_value(payload) }]
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
pub extern "C" fn aurora_direct_variant_payload(
    value: *mut OpaqueValue,
    index: i64,
) -> *mut OpaqueValue {
    match unsafe { value_ref(value) } {
        Value::EnumVariant(variant) => match variant.payloads.get(index.max(0) as usize) {
            Some(payload) => boxed_value(payload.clone()),
            None => runtime_error(format!(
                "enum variant `{}.{}` does not carry a payload at index {}",
                variant.enum_name, variant.variant_name, index
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
    let class_name = decode_bytes(class_ptr, class_len);
    let names = unsafe { slice::from_raw_parts(names_ptr, count) };
    let lens = unsafe { slice::from_raw_parts(lens_ptr, count) };
    let values = unsafe { slice::from_raw_parts(values_ptr, count) };
    let mut fields = BTreeMap::new();
    for index in 0..count {
        let name = decode_bytes(names[index], lens[index]);
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
        class_name: decode_bytes(class_ptr, class_len),
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
            .get(&field)
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
                .insert(field, unsafe { take_value(new_value) });
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
    let count = match usize::try_from(count) {
        Ok(count) => count,
        Err(_) => runtime_error("invalid arg buffer size"),
    };
    let mut values = vec![0i64; count].into_boxed_slice();
    let ptr = values.as_mut_ptr();
    Box::leak(values);
    ptr
}

#[no_mangle]
pub extern "C" fn aurora_direct_arg_buffer_store(buffer: *mut i64, index: i64, value: i64) {
    let index = match usize::try_from(index) {
        Ok(index) => index,
        Err(_) => runtime_error("invalid arg index"),
    };
    unsafe {
        let previous = *buffer.add(index);
        if previous != 0 {
            aurora_direct_release_value(previous as *mut OpaqueValue);
        }
        if value != 0 {
            aurora_direct_retain_value(value as *mut OpaqueValue);
        }
        *buffer.add(index) = value;
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_channel_new() -> *mut OpaqueValue {
    boxed_value(Value::Channel(ChannelValue::new()))
}

#[no_mangle]
pub extern "C" fn aurora_direct_task_group_new() -> *mut OpaqueValue {
    boxed_value(Value::TaskGroup(TaskGroupValue::new(
        &current_cancellation(),
    )))
}

#[no_mangle]
pub extern "C" fn aurora_direct_cancelled() -> i64 {
    if current_cancellation().is_cancelled() {
        1
    } else {
        0
    }
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
            TryRecvResult::Closed => 1,
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
    let millis = match u64::try_from(millis) {
        Ok(millis) => millis,
        Err(_) => runtime_error("invalid deadline duration"),
    };
    let deadline = Deadline(
        match Instant::now().checked_add(StdDuration::from_millis(millis)) {
            Some(deadline) => deadline,
            None => Instant::now(),
        },
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
    if ready {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_deadline_drop(deadline: i64) {
    let deadline = deadline as usize as *mut Deadline;
    if !deadline.is_null() {
        unsafe {
            drop(Box::from_raw(deadline));
        }
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_sleep_ms(duration: i64) {
    let millis = match u64::try_from(duration) {
        Ok(millis) => millis,
        Err(_) => runtime_error("invalid sleep duration"),
    };
    thread::sleep(StdDuration::from_millis(millis));
}

#[no_mangle]
pub extern "C" fn aurora_direct_sleep_value(duration: *mut OpaqueValue) -> *mut OpaqueValue {
    let millis = extract_duration_millis(unsafe { value_ref(duration) });
    let millis = match u64::try_from(millis) {
        Ok(millis) => millis,
        Err(_) => runtime_error("invalid sleep duration"),
    };
    thread::sleep(StdDuration::from_millis(millis));
    boxed_value(Value::Unit)
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
    let arg_count = match usize::try_from(arg_count) {
        Ok(arg_count) => arg_count,
        Err(_) => runtime_error("invalid spawn arg count"),
    };
    let args = unsafe {
        let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            args_ptr as *mut i64,
            arg_count,
        ));
        boxed.into_vec()
    };
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
            Ok(unsafe { consume_value(result_ptr) })
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
pub extern "C" fn aurora_direct_fail_division_by_zero(line: i64, column: i64) -> ! {
    match runtime_span(line, column) {
        Some(span) => runtime_error_at(span, "division by zero"),
        None => runtime_error("division by zero"),
    }
}

#[no_mangle]
pub extern "C" fn aurora_direct_fail_int32_overflow(value: i64, line: i64, column: i64) -> ! {
    let message = int32_overflow_message(value);
    match runtime_span(line, column) {
        Some(span) => runtime_error_at(span, message),
        None => runtime_error(message),
    }
}

#[cfg(test)]
#[path = "native_runtime_tests.rs"]
mod tests;
