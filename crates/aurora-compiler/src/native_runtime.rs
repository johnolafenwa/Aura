#![cfg_attr(coverage, allow(dead_code))]

use std::borrow::Borrow;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::process;
use std::slice;
use std::str;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration as StdDuration, Instant};

use crate::ast::{BinaryOp, UnaryOp};
use crate::diag::{Diagnostic, Span};
use crate::integer::IntegerValue;
use crate::runtime_value::{
    cancel_current_lightweight_task_boundary, cast_numeric_value,
    current_lightweight_task_cancellation, current_lightweight_task_id,
    decode_process_restart_policy, decode_process_stdio, evaluate_host_builtin,
    fail_current_lightweight_task, io_error, io_read_line, option_none, option_some,
    poll_cancellation, process_error_cancelled, process_error_io, process_error_no_command,
    process_error_spawn, process_error_timed_out, process_exit_status, process_stdio_inherit,
    process_stdio_null, process_stdio_pipe, process_supervisor_wait_cancelled,
    process_supervisor_wait_event, process_supervisor_wait_timed_out, process_wait_cancelled,
    process_wait_exited, process_wait_failed, process_wait_timed_out, queue_receive_cancelled,
    queue_receive_closed, queue_receive_item, queue_receive_timed_out, read_file_limited,
    recv_for_registered_producers_iteration, recv_for_task_group_iteration,
    register_task_as_queue_producer_for_values, render_float, result_err, result_ok,
    run_blocking_io, run_lightweight_root_task, send_error_cancelled, send_error_closed,
    send_error_full, send_error_timed_out, sleep_with_runtime_scheduler,
    spawn_lightweight_task_with_cancellation, task_group_cleanup_should_cancel,
    task_result_cancelled, task_result_error, task_result_ready, task_result_timed_out,
    wait_all_cancelled, wait_all_error, wait_all_ready, wait_all_timed_out, wait_any_cancelled,
    wait_any_error, wait_any_ready, wait_any_timed_out, wait_for_runtime_scheduler,
    CancellationContext, ChannelValue, EnumVariantValue, FileValue, HttpListenerValue,
    HttpResponseValue, InstanceValue, LightweightTaskFailureSignal, MapValue, ProcessChildValue,
    ProcessChildWaitStatus, ProcessCompletedValue, ProcessSupervisorValue,
    ProcessSupervisorWaitStatus, RangeValue, RecvValueResult, RuntimeSchedulerWakeReason,
    SendValueError, SetValue, TaskCancelledSignal, TaskGroupValue, TaskValue, TaskWaitStatus,
    TcpListenerValue, TcpStreamValue, TlsListenerValue, TlsStreamValue, UdpSocketValue,
    UnixListenerValue, UnixStreamValue, Value, VecValue, WebSocketListenerValue, WebSocketValue,
};
use crate::sema::Type;

thread_local! {
    static TASK_RUNTIME_ERROR_CAPTURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static DIRECT_CLEANUP_STACKS: RefCell<BTreeMap<u64, Vec<DirectCleanupRegistration>>> = const { RefCell::new(BTreeMap::new()) };
    static DIRECT_NEXT_CLEANUP_ID: Cell<i64> = const { Cell::new(1) };
    static DIRECT_CLEANUP_DRAINING: Cell<bool> = const { Cell::new(false) };
    static DIRECT_PRIMARY_RUNTIME_DIAGNOSTIC: RefCell<Option<Diagnostic>> = const { RefCell::new(None) };
}

struct DirectCleanupRegistration {
    id: i64,
    thunk_ptr: i64,
    args: *mut i64,
    arg_count: usize,
    call_depth: usize,
}

fn direct_cleanup_key() -> u64 {
    current_lightweight_task_id().unwrap_or(0)
}

fn next_direct_cleanup_id() -> i64 {
    DIRECT_NEXT_CLEANUP_ID.with(|next| {
        let id = next.get();
        let mut next_id = id.checked_add(1).unwrap_or(1);
        if next_id == 0 {
            next_id = 1;
        }
        next.set(next_id);
        id
    })
}

fn push_direct_cleanup_registration(thunk_ptr: i64, args: *mut i64, arg_count: usize) -> i64 {
    let id = next_direct_cleanup_id();
    let cleanup_key = direct_cleanup_key();
    let call_depth = DIRECT_CALL_DEPTH.with(|depth| depth.get());
    DIRECT_CLEANUP_STACKS.with(|stacks| {
        stacks
            .borrow_mut()
            .entry(cleanup_key)
            .or_default()
            .push(DirectCleanupRegistration {
                id,
                thunk_ptr,
                args,
                arg_count,
                call_depth,
            });
    });
    id
}

fn take_direct_cleanup_registration(id: i64) -> Option<DirectCleanupRegistration> {
    if id == 0 {
        return None;
    }
    let cleanup_key = direct_cleanup_key();
    DIRECT_CLEANUP_STACKS.with(|stacks| {
        let mut stacks = stacks.borrow_mut();
        let stack = stacks.get_mut(&cleanup_key)?;
        let registration = stack
            .iter()
            .rposition(|registration| registration.id == id)
            .map(|index| stack.remove(index));
        if stack.is_empty() {
            stacks.remove(&cleanup_key);
        }
        registration
    })
}

struct DirectCleanupDrainGuard;

impl Drop for DirectCleanupDrainGuard {
    fn drop(&mut self) {
        DIRECT_CLEANUP_DRAINING.with(|draining| draining.set(false));
    }
}

struct DirectCallDepthGuard {
    previous: usize,
}

impl Drop for DirectCallDepthGuard {
    fn drop(&mut self) {
        DIRECT_CALL_DEPTH.with(|depth| depth.set(self.previous));
    }
}

struct DirectPrimaryDiagnosticGuard {
    installed: bool,
}

impl DirectPrimaryDiagnosticGuard {
    fn install(diagnostic: Diagnostic) -> Self {
        let installed = DIRECT_PRIMARY_RUNTIME_DIAGNOSTIC.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                false
            } else {
                *slot = Some(diagnostic);
                true
            }
        });
        Self { installed }
    }
}

impl Drop for DirectPrimaryDiagnosticGuard {
    fn drop(&mut self) {
        if self.installed {
            DIRECT_PRIMARY_RUNTIME_DIAGNOSTIC.with(|slot| {
                *slot.borrow_mut() = None;
            });
        }
    }
}

fn direct_primary_runtime_diagnostic() -> Option<Diagnostic> {
    DIRECT_PRIMARY_RUNTIME_DIAGNOSTIC.with(|slot| slot.borrow().clone())
}

fn is_call_depth_diagnostic(diagnostic: &Diagnostic) -> bool {
    diagnostic.message.starts_with("maximum call depth")
}

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
            // `with_sigpipe_blocked` only leaves SIGPIPE ignored on this path because this
            // caller exits the process immediately after observing BrokenPipe.
            process::exit(0);
        }
        let _ = writeln!(io::stderr().lock(), "failed to write to stdout: {}", error);
        process::exit(1);
    }
}

fn write_stdout_result(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    with_sigpipe_blocked(|| stdout.write_all(text.as_bytes()))
}

fn flush_stdout_result() -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    with_sigpipe_blocked(|| stdout.flush())
}

#[cfg(unix)]
fn with_sigpipe_blocked<T>(f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    unsafe {
        let previous_handler = libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        let mut sigpipe_set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut sigpipe_set);
        libc::sigaddset(&mut sigpipe_set, libc::SIGPIPE);

        let mut old_mask: libc::sigset_t = std::mem::zeroed();
        if libc::pthread_sigmask(libc::SIG_BLOCK, &sigpipe_set, &mut old_mask) != 0 {
            let result = f();
            if previous_handler != libc::SIG_ERR {
                let _ = libc::signal(libc::SIGPIPE, previous_handler);
            }
            return result;
        }

        let restore_sigpipe_state = || {
            let _ = libc::pthread_sigmask(libc::SIG_SETMASK, &old_mask, std::ptr::null_mut());
            if previous_handler != libc::SIG_ERR {
                let _ = libc::signal(libc::SIGPIPE, previous_handler);
            }
        };

        let result = f();
        if matches!(&result, Err(error) if error.kind() == io::ErrorKind::BrokenPipe) {
            let mut pending: libc::sigset_t = std::mem::zeroed();
            if libc::sigpending(&mut pending) == 0
                && libc::sigismember(&pending, libc::SIGPIPE) == 1
            {
                let mut received = 0;
                let _ = libc::sigwait(&sigpipe_set, &mut received);
            }
            // Restore the thread's signal mask so the helper does not leak blocked SIGPIPE
            // state. We intentionally keep SIGPIPE ignored on this path because the caller
            // exits immediately after seeing BrokenPipe; restoring the previous disposition
            // before that exit can cause the pending SIGPIPE to terminate the process.
            let _ = libc::pthread_sigmask(libc::SIG_SETMASK, &old_mask, std::ptr::null_mut());
            return result;
        }

        restore_sigpipe_state();
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

pub struct OpaqueValue {
    ref_count: AtomicUsize,
    value: RwLock<Value>,
}

type NativeThunk = unsafe extern "C-unwind" fn(*const i64, usize) -> *mut OpaqueValue;
const DIRECT_MAX_CALL_DEPTH: usize = 256;
const DIRECT_RUNTIME_STACK_SIZE: usize = 64 * 1024 * 1024;

struct ProgramSourceContext {
    path: String,
    source: String,
}

thread_local! {
    static DIRECT_CANCELLATION: RefCell<CancellationContext> =
        RefCell::new(CancellationContext::default());
    static DIRECT_CALL_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

static DIRECT_PROGRAM_SOURCE: OnceLock<ProgramSourceContext> = OnceLock::new();

fn current_cancellation() -> CancellationContext {
    if let Some(cancellation) = current_lightweight_task_cancellation() {
        return cancellation;
    }
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
    Box::into_raw(Box::new(OpaqueValue {
        ref_count: AtomicUsize::new(1),
        value: RwLock::new(value),
    }))
}

// These helpers validate the explicit refcount stored in `OpaqueValue`, but they cannot detect
// stale or forged raw pointers after an object has been freed and the address reused. The
// codegen/runtime ABI must still guarantee that callers only retain or release live values.
fn retain_ref_count(ref_count: &AtomicUsize) -> std::result::Result<(), &'static str> {
    loop {
        let current = ref_count.load(Ordering::Relaxed);
        if current == 0 {
            return Err("attempted to retain an already-released direct runtime value");
        }
        if current == usize::MAX {
            return Err("direct runtime value reference count overflow");
        }
        if ref_count
            .compare_exchange_weak(current, current + 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return Ok(());
        }
    }
}

fn release_ref_count(ref_count: &AtomicUsize) -> std::result::Result<bool, &'static str> {
    loop {
        let current = ref_count.load(Ordering::Acquire);
        if current == 0 {
            return Err("attempted to release an already-released direct runtime value");
        }
        let next = current - 1;
        if ref_count
            .compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ok(next == 0);
        }
    }
}

unsafe fn with_value<T>(ptr: *mut OpaqueValue, read: impl FnOnce(&Value) -> T) -> T {
    let value = match ptr.as_ref() {
        Some(value) => value,
        None => runtime_error("direct runtime received a null opaque value pointer"),
    };
    let guard = match value.value.read() {
        Ok(guard) => guard,
        Err(_) => runtime_error("direct runtime value lock was poisoned"),
    };
    read(&guard)
}

unsafe fn value_ref(ptr: *mut OpaqueValue) -> Value {
    with_value(ptr, Clone::clone)
}

unsafe fn value_mut<T>(ptr: *mut OpaqueValue, write: impl FnOnce(&mut Value) -> T) -> T {
    let value = match ptr.as_ref() {
        Some(value) => value,
        None => runtime_error("direct runtime received a null opaque value pointer"),
    };
    let mut guard = match value.value.write() {
        Ok(guard) => guard,
        Err(_) => runtime_error("direct runtime value lock was poisoned"),
    };
    write(&mut guard)
}

unsafe fn take_value(ptr: *mut OpaqueValue) -> Value {
    value_ref(ptr)
}

unsafe fn consume_value(ptr: *mut OpaqueValue) -> Value {
    let value = value_ref(ptr);
    unsafe {
        aurora_direct_release_value(ptr);
    }
    value
}

#[cfg(coverage)]
#[doc(hidden)]
pub unsafe fn aurora_direct_coverage_clone_value(ptr: *mut OpaqueValue) -> Value {
    unsafe { value_ref(ptr) }
}

unsafe fn consume_opaque_buffer(buffer: *mut i64, count: usize) -> Vec<Value> {
    let handles = unsafe { Vec::from_raw_parts(buffer, count, count) };
    handles
        .into_iter()
        .map(|handle| {
            if handle == 0 {
                runtime_error("direct runtime received a null enum payload handle");
            }
            unsafe { consume_value(handle as *mut OpaqueValue) }
        })
        .collect()
}

fn decode_bytes(ptr: *const u8, len: usize) -> String {
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    str::from_utf8(bytes)
        .unwrap_or_else(|_| runtime_error("aurora direct runtime received invalid UTF-8 bytes"))
        .to_string()
}

fn bytes_vec_value(bytes: Vec<u8>) -> Value {
    Value::Vec(VecValue {
        element_type: Type::named("uint8"),
        elements: bytes
            .into_iter()
            .map(|byte| Value::Int(IntegerValue::from_signed(byte as i128)))
            .collect(),
    })
}

fn headers_map_value(headers: Vec<(String, String)>) -> Value {
    Value::Map(MapValue {
        key_type: Type::named("String"),
        value_type: Type::named("String"),
        entries: headers
            .into_iter()
            .map(|(key, value)| (Value::String(key), Value::String(value)))
            .collect(),
    })
}

fn expect_string_value(value: &Value, label: &str) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => runtime_error(format!(
            "`{}` expects `String`, found `{}`",
            label,
            value_type_name(other)
        )),
    }
}

fn expect_bytes_value(value: &Value, label: &str) -> Vec<u8> {
    match value {
        Value::Vec(vector)
            if (vector.element_type == Type::named("uint8")
                || vector.element_type == Type::named("Unknown"))
                && vector
                    .elements
                    .iter()
                    .all(|element| matches!(element, Value::Int(_))) =>
        {
            let mut bytes = Vec::with_capacity(vector.elements.len());
            for element in &vector.elements {
                let Value::Int(value) = element else {
                    runtime_error(format!("`{}` expects `Vec[uint8]`", label));
                };
                let byte = value
                    .as_i128()
                    .and_then(|value| u8::try_from(value).ok())
                    .unwrap_or_else(|| runtime_error(format!("`{}` expects `Vec[uint8]`", label)));
                bytes.push(byte);
            }
            bytes
        }
        other => runtime_error(format!(
            "`{}` expects `Vec[uint8]`, found `{}`",
            label,
            value_type_name(other)
        )),
    }
}

fn expect_bool_value(value: &Value, label: &str) -> bool {
    match value {
        Value::Bool(value) => *value,
        other => runtime_error(format!(
            "`{}` expects `bool`, found `{}`",
            label,
            value_type_name(other)
        )),
    }
}

fn expect_i32_value(value: &Value, label: &str) -> i32 {
    match value {
        Value::Int(number) => number
            .as_i128()
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or_else(|| runtime_error(format!("`{}` expects `int32`", label))),
        other => runtime_error(format!(
            "`{}` expects `int32`, found `{}`",
            label,
            value_type_name(other)
        )),
    }
}

fn expect_headers_map(value: &Value, label: &str) -> Vec<(String, String)> {
    match value {
        Value::Map(map)
            if (map.key_type == Type::named("String")
                || map.key_type == Type::named("Unknown"))
                && (map.value_type == Type::named("String")
                    || map.value_type == Type::named("Unknown")) =>
        {
            map.entries
                .iter()
                .map(|(key, value)| {
                    (
                        expect_string_value(key, label),
                        expect_string_value(value, label),
                    )
                })
                .collect()
        }
        other => runtime_error(format!(
            "`{}` expects `Map[String, String]`, found `{}`",
            label,
            value_type_name(other)
        )),
    }
}

fn optional_timeout_from_ptr(value: *mut OpaqueValue, label: &str) -> Option<StdDuration> {
    if value.is_null() {
        return None;
    }
    match unsafe { value_ref(value) } {
        Value::Unit => None,
        Value::Duration(duration) => Some(
            u64::try_from(duration)
                .map(StdDuration::from_millis)
                .unwrap_or_else(|_| {
                    runtime_error(format!("`{}` duration must be non-negative", label))
                }),
        ),
        other => runtime_error(format!(
            "`{}` expects `Duration`, found `{}`",
            label,
            value_type_name(other)
        )),
    }
}

fn process_optional_timeout_from_ptr(value: *mut OpaqueValue, label: &str) -> Option<StdDuration> {
    if value.is_null() {
        return None;
    }
    match unsafe { value_ref(value) } {
        Value::Unit => None,
        Value::Duration(duration) if duration < 0 => None,
        Value::Duration(duration) => Some(
            u64::try_from(duration)
                .map(StdDuration::from_millis)
                .unwrap_or_else(|_| {
                    runtime_error(format!("`{}` duration must be non-negative", label))
                }),
        ),
        other => runtime_error(format!(
            "`{}` expects `Duration`, found `{}`",
            label,
            value_type_name(other)
        )),
    }
}

fn duration_from_ptr(value: *mut OpaqueValue, label: &str) -> StdDuration {
    match unsafe { value_ref(value) } {
        Value::Duration(duration) => u64::try_from(duration)
            .map(StdDuration::from_millis)
            .unwrap_or_else(|_| {
                runtime_error(format!("`{}` duration must be non-negative", label))
            }),
        other => runtime_error(format!(
            "`{}` expects `Duration`, found `{}`",
            label,
            value_type_name(other)
        )),
    }
}

fn supervisor_max_restarts_from_ptr(value: *mut OpaqueValue, label: &str) -> Option<i32> {
    let value = expect_i32_value(&unsafe { value_ref(value) }, label);
    if value < -1 {
        runtime_error(format!(
            "`{}` expects `max_restarts` to be -1 or greater",
            label
        ));
    }
    (value >= 0).then_some(value)
}

fn expect_command_vec(value: &Value, label: &str) -> Vec<String> {
    match value {
        Value::Vec(vector)
            if vector.element_type == Type::named("String")
                || vector.element_type == Type::named("Unknown") =>
        {
            vector
                .elements
                .iter()
                .map(|element| expect_string_value(element, label))
                .collect()
        }
        other => runtime_error(format!(
            "`{}` expects `Vec[String]`, found `{}`",
            label,
            value_type_name(other)
        )),
    }
}

fn expect_optional_string_value(value: &Value, label: &str) -> Option<String> {
    match value {
        Value::Unit => None,
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "None" =>
        {
            None
        }
        Value::EnumVariant(variant)
            if variant.enum_name == "Option" && variant.variant_name == "Some" =>
        {
            match variant.payloads.as_slice() {
                [text] => Some(expect_string_value(text, label)),
                _ => runtime_error(format!(
                    "`{}` expects `Option[String]`, found malformed option payload",
                    label
                )),
            }
        }
        other => runtime_error(format!(
            "`{}` expects `Option[String]`, found `{}`",
            label,
            value_type_name(other)
        )),
    }
}

fn process_error_from_io(error: io::Error) -> Value {
    match error.kind() {
        io::ErrorKind::TimedOut => process_error_timed_out(),
        io::ErrorKind::Interrupted => process_error_cancelled(),
        _ => process_error_io(error),
    }
}

fn await_process_capture_task(task: Option<TaskValue>, label: &str) -> Vec<u8> {
    let Some(task) = task else {
        return Vec::new();
    };
    match task.wait_result_with_cancellation_observed(None, Some(&current_cancellation())) {
        TaskWaitStatus::Ready(Ok(Value::Vec(vector)))
            if vector.element_type == Type::named("uint8") =>
        {
            vector
                .elements
                .into_iter()
                .map(|value| match value {
                    Value::Int(value) => value
                        .as_i128()
                        .and_then(|value| u8::try_from(value).ok())
                        .unwrap_or_else(|| {
                            runtime_error(format!(
                                "process {} capture returned a non-byte integer",
                                label
                            ))
                        }),
                    other => runtime_error(format!(
                        "process {} capture returned `{}` inside `Vec[uint8]`",
                        label,
                        other.render()
                    )),
                })
                .collect()
        }
        TaskWaitStatus::Ready(Ok(other)) => runtime_error(format!(
            "process {} capture returned `{}` instead of `Vec[uint8]`",
            label,
            other.render()
        )),
        TaskWaitStatus::Ready(Err(error)) => runtime_diagnostic_error(error),
        TaskWaitStatus::TimedOut => {
            runtime_error(format!("process {} capture timed out unexpectedly", label))
        }
        TaskWaitStatus::Cancelled => runtime_error(format!(
            "process {} capture was cancelled unexpectedly",
            label
        )),
    }
}

fn render_runtime_diagnostic(diagnostic: Diagnostic) -> String {
    if let Some(context) = DIRECT_PROGRAM_SOURCE.get() {
        diagnostic.render_with_source(&context.path, &context.source)
    } else {
        format!("error: {}", diagnostic.message)
    }
}

unsafe fn release_direct_cleanup_args(args: *mut i64, arg_count: usize) {
    if args.is_null() {
        return;
    }
    let values = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(args, arg_count)) };
    for value in values.iter().copied() {
        if value != 0 {
            unsafe {
                aurora_direct_release_value(value as *mut OpaqueValue);
            }
        }
    }
}

fn drain_direct_cleanup_stack() {
    let already_draining = DIRECT_CLEANUP_DRAINING.with(|draining| {
        if draining.get() {
            true
        } else {
            draining.set(true);
            false
        }
    });
    if already_draining {
        return;
    }
    let _guard = DirectCleanupDrainGuard;
    let previous_depth = DIRECT_CALL_DEPTH.with(|depth| {
        let previous = depth.get();
        depth.set(0);
        previous
    });
    let _depth_guard = DirectCallDepthGuard {
        previous: previous_depth,
    };
    let cleanup_key = direct_cleanup_key();
    let registrations = DIRECT_CLEANUP_STACKS
        .with(|stacks| stacks.borrow_mut().remove(&cleanup_key).unwrap_or_default());
    let skip_max_depth_cleanup = direct_primary_runtime_diagnostic()
        .as_ref()
        .is_some_and(is_call_depth_diagnostic);
    for registration in registrations.into_iter().rev() {
        // Match the interpreter: a cleanup call captured at the saturated Aurora
        // call depth cannot enter its `close` method during recursion unwinding.
        if skip_max_depth_cleanup && registration.call_depth >= DIRECT_MAX_CALL_DEPTH {
            unsafe {
                release_direct_cleanup_args(registration.args, registration.arg_count);
            }
            continue;
        }
        if registration.thunk_ptr != 0 {
            let thunk: NativeThunk =
                unsafe { std::mem::transmute(registration.thunk_ptr as usize) };
            let result = unsafe { thunk(registration.args as *const i64, registration.arg_count) };
            unsafe {
                aurora_direct_release_value(result);
            }
        }
        unsafe {
            release_direct_cleanup_args(registration.args, registration.arg_count);
        }
    }
}

fn emit_runtime_diagnostic_error(diagnostic: Diagnostic) -> ! {
    if TASK_RUNTIME_ERROR_CAPTURE.with(|capture| capture.get()) {
        std::panic::panic_any(LightweightTaskFailureSignal(diagnostic));
    }
    let _ = writeln!(
        io::stderr().lock(),
        "{}",
        render_runtime_diagnostic(diagnostic)
    );
    process::exit(1);
}

fn runtime_diagnostic_error(diagnostic: Diagnostic) -> ! {
    if DIRECT_CLEANUP_DRAINING.with(|draining| draining.get()) {
        emit_runtime_diagnostic_error(direct_primary_runtime_diagnostic().unwrap_or(diagnostic));
    }
    let _primary_guard = DirectPrimaryDiagnosticGuard::install(diagnostic.clone());
    drain_direct_cleanup_stack();
    emit_runtime_diagnostic_error(diagnostic);
}

fn runtime_error(message: impl AsRef<str>) -> ! {
    runtime_diagnostic_error(Diagnostic::new(message.as_ref()))
}

fn runtime_error_at(span: Span, message: impl AsRef<str>) -> ! {
    runtime_diagnostic_error(Diagnostic::at(span, message.as_ref()))
}

fn with_task_runtime_error_capture<T>(f: impl FnOnce() -> T) -> T {
    struct CaptureGuard {
        previous: bool,
    }

    impl Drop for CaptureGuard {
        fn drop(&mut self) {
            TASK_RUNTIME_ERROR_CAPTURE.with(|capture| capture.set(self.previous));
        }
    }

    TASK_RUNTIME_ERROR_CAPTURE.with(|capture| {
        let previous = capture.replace(true);
        let _guard = CaptureGuard { previous };
        f()
    })
}

#[track_caller]
fn task_runtime_boundary<T>(f: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) if payload.is::<TaskCancelledSignal>() => {
            cancel_current_lightweight_task_boundary()
        }
        Err(payload) => match payload.downcast::<LightweightTaskFailureSignal>() {
            Ok(signal) => fail_current_lightweight_task(signal.0),
            Err(payload) => std::panic::resume_unwind(payload),
        },
    }
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
        Value::Channel(_) => "Queue".to_string(),
        Value::Task(_) => "Task".to_string(),
        Value::TaskGroup(_) => "TaskGroup".to_string(),
        Value::File(_) => "fs.File".to_string(),
        Value::TcpListener(_) => "net.TcpListener".to_string(),
        Value::TcpStream(_) => "net.TcpStream".to_string(),
        Value::UdpSocket(_) => "net.UdpSocket".to_string(),
        Value::UdpDatagram(_) => "net.UdpDatagram".to_string(),
        Value::HttpListener(_) => "net.HttpListener".to_string(),
        Value::HttpExchange(_) => "net.HttpExchange".to_string(),
        Value::HttpResponse(_) => "net.HttpResponse".to_string(),
        Value::WebSocketListener(_) => "net.WebSocketListener".to_string(),
        Value::WebSocket(_) => "net.WebSocket".to_string(),
        Value::UnixListener(_) => "net.UnixListener".to_string(),
        Value::UnixStream(_) => "net.UnixStream".to_string(),
        Value::TlsListener(_) => "net.TlsListener".to_string(),
        Value::TlsStream(_) => "net.TlsStream".to_string(),
        Value::ProcessChild(_) => "process.Child".to_string(),
        Value::ProcessPipe(_) => "process.Pipe".to_string(),
        Value::ProcessCompleted(_) => "process.Completed".to_string(),
        Value::ProcessSupervisor(_) => "process.Supervisor".to_string(),
    }
}

fn inferred_collection_type(value: &Value) -> Type {
    match value {
        Value::String(_) => Type::named("String"),
        Value::Bool(_) => Type::named("bool"),
        Value::Float(_) => Type::named("float64"),
        Value::Vec(vector) => Type::Named("Vec".to_string(), vec![vector.element_type.clone()]),
        Value::Set(set) => Type::Named("Set".to_string(), vec![set.element_type.clone()]),
        Value::Map(map) => Type::Named(
            "Map".to_string(),
            vec![map.key_type.clone(), map.value_type.clone()],
        ),
        Value::Duration(_) => Type::named("Duration"),
        Value::Range(_) => Type::named("Range"),
        Value::Instance(instance) => Type::named(instance.class_name.clone()),
        Value::EnumVariant(variant) => Type::named(variant.enum_name.clone()),
        Value::Channel(_) => Type::named("Queue"),
        Value::Task(_) => Type::named("Task"),
        Value::TaskGroup(_) => Type::named("TaskGroup"),
        Value::File(_) => Type::named("fs.File"),
        Value::TcpListener(_) => Type::named("net.TcpListener"),
        Value::TcpStream(_) => Type::named("net.TcpStream"),
        Value::UdpSocket(_) => Type::named("net.UdpSocket"),
        Value::UdpDatagram(_) => Type::named("net.UdpDatagram"),
        Value::HttpListener(_) => Type::named("net.HttpListener"),
        Value::HttpExchange(_) => Type::named("net.HttpExchange"),
        Value::HttpResponse(_) => Type::named("net.HttpResponse"),
        Value::WebSocketListener(_) => Type::named("net.WebSocketListener"),
        Value::WebSocket(_) => Type::named("net.WebSocket"),
        Value::UnixListener(_) => Type::named("net.UnixListener"),
        Value::UnixStream(_) => Type::named("net.UnixStream"),
        Value::TlsListener(_) => Type::named("net.TlsListener"),
        Value::TlsStream(_) => Type::named("net.TlsStream"),
        Value::ProcessChild(_) => Type::named("process.Child"),
        Value::ProcessPipe(_) => Type::named("process.Pipe"),
        Value::ProcessCompleted(_) => Type::named("process.Completed"),
        Value::ProcessSupervisor(_) => Type::named("process.Supervisor"),
        Value::Int(_) | Value::ModuleNamespace(_) | Value::Unit => Type::named("Unknown"),
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
            (Value::Int(left), Value::Int(right)) => Ok(Value::Int(
                left.checked_div(right)
                    .expect("non-zero integer division is total"),
            )),
            (Value::Float(_), Value::Float(0.0)) => Err(Diagnostic::new("division by zero")),
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
            (Value::Int(left), Value::Int(right)) => Ok(Value::Int(
                left.checked_rem(right)
                    .expect("non-zero integer remainder is total"),
            )),
            (Value::Float(_), Value::Float(0.0)) => Err(Diagnostic::new("division by zero")),
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

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_runtime_init(
    path_ptr: *const u8,
    path_len: usize,
    source_ptr: *const u8,
    source_len: usize,
) {
    task_runtime_boundary(|| {
        DIRECT_CLEANUP_STACKS.with(|stacks| stacks.borrow_mut().clear());
        DIRECT_NEXT_CLEANUP_ID.with(|next| next.set(1));
        DIRECT_CLEANUP_DRAINING.with(|draining| draining.set(false));
        let _ = DIRECT_PROGRAM_SOURCE.set(ProgramSourceContext {
            path: decode_bytes(path_ptr, path_len),
            source: decode_bytes(source_ptr, source_len),
        });
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aurora_direct_run_root(thunk_ptr: i64) -> i32 {
    task_runtime_boundary(|| {
        if thunk_ptr == 0 {
            runtime_error("invalid direct root thunk pointer");
        }
        let thunk: NativeThunk = unsafe { std::mem::transmute(thunk_ptr as usize) };
        let result = std::thread::Builder::new()
            .stack_size(DIRECT_RUNTIME_STACK_SIZE)
            .spawn(move || {
                run_lightweight_root_task(move || {
                    with_cancellation_scope(CancellationContext::default(), || {
                        let result_ptr = unsafe { thunk(std::ptr::null(), 0) };
                        Ok(unsafe { consume_value(result_ptr) })
                    })
                })
            })
            .unwrap_or_else(|error| {
                runtime_error(format!("failed to start direct runtime thread: {}", error))
            })
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
        match result {
            Ok(Value::Int(value)) => value.as_i128().unwrap_or_default() as i32,
            Ok(Value::Unit) => 0,
            Ok(other) => runtime_error(format!(
                "direct main entry must return `int32` or `None`, found `{}`",
                value_type_name(&other)
            )),
            Err(error) => runtime_diagnostic_error(error),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aurora_direct_enter_call(
    line: i64,
    column: i64,
    function_ptr: *const u8,
    function_len: usize,
) {
    task_runtime_boundary(|| {
        DIRECT_CALL_DEPTH.with(|slot| {
            let depth = slot.get();
            if depth >= DIRECT_MAX_CALL_DEPTH {
                let function = decode_bytes(function_ptr, function_len);
                let message = format!(
                    "maximum call depth of {} exceeded while calling `{}`",
                    DIRECT_MAX_CALL_DEPTH, function
                );
                if line > 0 && column > 0 {
                    runtime_error_at(Span::new(line as usize, column as usize), message);
                }
                runtime_error(message);
            }
            slot.set(depth + 1);
        });
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aurora_direct_exit_call() {
    task_runtime_boundary(|| {
        DIRECT_CALL_DEPTH.with(|slot| {
            let depth = slot.get();
            if depth > 0 {
                slot.set(depth - 1);
            }
        });
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_print_i64(value: i64) {
    task_runtime_boundary(|| {
        write_stdout(&format!("{}\n", value));
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_print_f64(value: f64) {
    task_runtime_boundary(|| {
        write_stdout(&render_float(value));
        write_stdout("\n");
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_print_bool(value: i64) {
    task_runtime_boundary(|| {
        write_stdout(render_bool(value));
        write_stdout("\n");
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_box_i64(value: i64) -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(Value::Int(IntegerValue::from_signed(value as i128))))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_box_uint_literal(
    ptr: *const u8,
    len: usize,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let text = decode_bytes(ptr, len);
        let value = match text.parse::<u128>() {
            Ok(value) => value,
            Err(_) => runtime_error(format!("invalid embedded uint literal `{}`", text)),
        };
        boxed_value(Value::Int(IntegerValue::from_literal(value)))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_box_f64(value: f64) -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(Value::Float(value)))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_box_bool(value: i64) -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(Value::Bool(value != 0)))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_box_unit() -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(Value::Unit))
}

#[cfg_attr(not(coverage), no_mangle)]
/// # Safety
///
/// `value` must be either null or a live `OpaqueValue` pointer allocated by the Aurora direct
/// runtime. Callers must only retain pointers whose storage is still owned by the current process.
pub unsafe extern "C-unwind" fn aurora_direct_retain_value(
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        if !value.is_null() {
            let opaque = unsafe {
                value.as_ref().unwrap_or_else(|| {
                    runtime_error("direct runtime received a null opaque value pointer")
                })
            };
            if let Err(message) = retain_ref_count(&opaque.ref_count) {
                runtime_error(message);
            }
        }
        value
    })
}

#[cfg_attr(not(coverage), no_mangle)]
/// # Safety
///
/// `value` must be either null or a live `OpaqueValue` pointer allocated by the Aurora direct
/// runtime. Each successful retain/release pair must be balanced according to the direct-runtime
/// ownership contract.
pub unsafe extern "C-unwind" fn aurora_direct_release_value(value: *mut OpaqueValue) {
    task_runtime_boundary(|| {
        if !value.is_null() {
            unsafe {
                let opaque = value.as_ref().unwrap_or_else(|| {
                    runtime_error("direct runtime received a null opaque value pointer")
                });
                if release_ref_count(&opaque.ref_count)
                    .unwrap_or_else(|message| runtime_error(message))
                {
                    drop(Box::from_raw(value));
                }
            }
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_literal(
    ptr: *const u8,
    len: usize,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(Value::String(decode_bytes(ptr, len))))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_stringify_value(
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let rendered = unsafe { value_ref(value) }.render();
        boxed_value(Value::String(rendered))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_duration_literal(value: i64) -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(Value::Duration(value as i128)))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_len(value: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| match unsafe { value_ref(value) } {
        Value::String(text) => match i64::try_from(text.len()) {
            Ok(length) => length,
            Err(_) => runtime_error("string length does not fit in the direct runtime range"),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_contains(
    value: *mut OpaqueValue,
    needle: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_starts_with(
    value: *mut OpaqueValue,
    prefix: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_ends_with(
    value: *mut OpaqueValue,
    suffix: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_split(
    value: *mut OpaqueValue,
    separator: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_replace(
    value: *mut OpaqueValue,
    from: *mut OpaqueValue,
    to: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_to_lower(
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(value) } {
        Value::String(text) => boxed_value(Value::String(text.to_lowercase())),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_to_upper(
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(value) } {
        Value::String(text) => boxed_value(Value::String(text.to_uppercase())),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_strip_prefix(
    value: *mut OpaqueValue,
    prefix: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_strip_suffix(
    value: *mut OpaqueValue,
    suffix: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_trim(value: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(value) } {
        Value::String(text) => boxed_value(Value::String(text.trim().to_string())),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_string_join(
    separator: *mut OpaqueValue,
    parts: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_abs(value: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_min(
    left: *mut OpaqueValue,
    right: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_max(
    left: *mut OpaqueValue,
    right: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_sqrt(value: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let value = unsafe { take_value(value) };
        match value {
            Value::Float(value) => boxed_value(Value::Float(value.sqrt())),
            other => runtime_error(format!(
                "`sqrt(...)` expects `float32` or `float64`, found `{}`",
                value_type_name(&other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_parse_int32(value: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_parse_int64(value: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_parse_float64(value: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let value = unsafe { take_value(value) };
        match value {
            Value::String(text) => match text.parse::<f64>() {
                Ok(value) if value.is_finite() => boxed_value(result_ok(Value::Float(value))),
                Ok(_) => boxed_value(result_err(Value::String(
                    "float must be finite".to_string(),
                ))),
                Err(error) => boxed_value(result_err(Value::String(error.to_string()))),
            },
            other => runtime_error(format!(
                "`parse_float64(...)` expects `String`, found `{}`",
                value_type_name(&other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_range_new(start: i64, end: i64) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        boxed_value(Value::Range(RangeValue {
            start: start as i128,
            end: end as i128,
        }))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_range_current(range: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| match unsafe { value_ref(range) } {
        Value::Range(range) => match i64::try_from(range.start) {
            Ok(start) => start,
            Err(_) => runtime_error("range start is outside host i64 bounds"),
        },
        other => runtime_error(format!(
            "expected `Range`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_range_end(range: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| match unsafe { value_ref(range) } {
        Value::Range(range) => match i64::try_from(range.end) {
            Ok(end) => end,
            Err(_) => runtime_error("range end is outside host i64 bounds"),
        },
        other => runtime_error(format!(
            "expected `Range`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_range_advance(range: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(range) } {
        Value::Range(range) => boxed_value(Value::Range(RangeValue {
            start: range.start + 1,
            end: range.end,
        })),
        other => runtime_error(format!(
            "expected `Range`, found `{}`",
            value_type_name(other)
        )),
    })
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

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_empty() -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        boxed_value(Value::Vec(VecValue {
            element_type: Type::named("Unknown"),
            elements: Vec::new(),
        }))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_len(vec: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| {
        match i64::try_from(with_vector(vec, |vector| vector.elements.len())) {
            Ok(length) => length,
            Err(_) => runtime_error("vector length does not fit in the direct runtime range"),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_is_empty(vec: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| i64::from(with_vector(vec, |vector| vector.elements.is_empty())))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_push_in_place(
    vec: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let value = unsafe { take_value(value) };
        let inferred = inferred_collection_type(&value);
        with_vector_mut(vec, |vector| {
            if vector.element_type == Type::named("Unknown") && inferred != Type::named("Unknown") {
                vector.element_type = inferred;
            }
            vector.elements.push(value);
        });
        boxed_value(Value::Unit)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_pop_in_place(vec: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let value = with_vector_mut(vec, |vector| vector.elements.pop());
        match value {
            Some(value) => boxed_value(option_some(value)),
            None => boxed_value(option_none()),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_get(
    vec: *mut OpaqueValue,
    index: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let index = checked_vec_index(index);
        let value = with_vector(vec, |vector| vector.elements.get(index).cloned());
        boxed_value(value.map(option_some).unwrap_or_else(option_none))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_set_in_place(
    vec: *mut OpaqueValue,
    index: i64,
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let index = checked_vec_index(index);
        let value = unsafe { take_value(value) };
        let previous = with_vector_mut(vec, |vector| {
            if index >= vector.elements.len() {
                runtime_error(format!(
                    "vector set index `{}` is out of bounds for length `{}`",
                    index,
                    vector.elements.len()
                ));
            }
            std::mem::replace(&mut vector.elements[index], value)
        });
        boxed_value(option_some(previous))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_remove_in_place(
    vec: *mut OpaqueValue,
    index: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let index = checked_vec_index(index);
        let previous = with_vector_mut(vec, |vector| {
            if index >= vector.elements.len() {
                runtime_error(format!(
                    "vector remove index `{}` is out of bounds for length `{}`",
                    index,
                    vector.elements.len()
                ));
            }
            vector.elements.remove(index)
        });
        boxed_value(option_some(previous))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_swap_in_place(
    vec: *mut OpaqueValue,
    first: i64,
    second: i64,
) -> i64 {
    task_runtime_boundary(|| {
        let first = checked_vec_index(first);
        let second = checked_vec_index(second);
        with_vector_mut(vec, |vector| {
            if first >= vector.elements.len() || second >= vector.elements.len() {
                runtime_error(format!(
                    "vector swap indices `{}` and `{}` are out of bounds for length `{}`",
                    first,
                    second,
                    vector.elements.len()
                ));
            }
            vector.elements.swap(first, second);
        });
        1
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_contains(
    vec: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| {
        let needle = unsafe { take_value(value) };
        i64::from(with_vector(vec, |vector| vector.elements.contains(&needle)))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_insert_in_place(
    vec: *mut OpaqueValue,
    index: i64,
    value: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| {
        let index = checked_vec_index(index);
        let value = unsafe { take_value(value) };
        with_vector_mut(vec, |vector| {
            if index > vector.elements.len() {
                runtime_error(format!(
                    "vector insert index `{}` is out of bounds for length `{}`",
                    index,
                    vector.elements.len()
                ));
            }
            vector.elements.insert(index, value);
        });
        1
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_clear_in_place(
    vec: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        with_vector_mut(vec, |vector| vector.elements.clear());
        boxed_value(Value::Unit)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_reverse_in_place(
    vec: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        with_vector_mut(vec, |vector| vector.elements.reverse());
        boxed_value(Value::Unit)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_extend_in_place(
    vec: *mut OpaqueValue,
    other: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let other = unsafe { take_value(other) };
        let Value::Vec(other) = other else {
            runtime_error("`extend` requires another `Vec[T]` value");
        };
        with_vector_mut(vec, |vector| vector.elements.extend(other.elements));
        boxed_value(Value::Unit)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_index(
    vec: *mut OpaqueValue,
    index: i64,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_index_option(
    vec: *mut OpaqueValue,
    index: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let index = checked_vec_index(index);
        let value = with_vector(vec, |vector| vector.elements.get(index).cloned());
        boxed_value(value.map(option_some).unwrap_or_else(option_none))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_vec_set_index_in_place(
    vec: *mut OpaqueValue,
    index: i64,
    value: *mut OpaqueValue,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_empty() -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        boxed_value(Value::Map(MapValue {
            key_type: Type::named("Unknown"),
            value_type: Type::named("Unknown"),
            entries: Vec::new(),
        }))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_len(map: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(
        || match i64::try_from(with_map(map, |map| map.entries.len())) {
            Ok(length) => length,
            Err(_) => runtime_error("map length does not fit in the direct runtime range"),
        },
    )
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_is_empty(map: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| i64::from(with_map(map, |map| map.entries.is_empty())))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_get(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let key = unsafe { take_value(key) };
        let value = with_map(map, |map| {
            map.entries
                .iter()
                .find(|(candidate_key, _)| *candidate_key == key)
                .map(|(_, value)| value.clone())
        });
        boxed_value(value.map(option_some).unwrap_or_else(option_none))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_set_in_place(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let key = unsafe { take_value(key) };
        let value = unsafe { take_value(value) };
        let inferred_key_type = inferred_collection_type(&key);
        let inferred_value_type = inferred_collection_type(&value);
        let previous = with_map_mut(map, |map| {
            if map.key_type == Type::named("Unknown") && inferred_key_type != Type::named("Unknown")
            {
                map.key_type = inferred_key_type.clone();
            }
            if map.value_type == Type::named("Unknown")
                && inferred_value_type != Type::named("Unknown")
            {
                map.value_type = inferred_value_type.clone();
            }
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_remove_in_place(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_contains_key(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| {
        let key = unsafe { take_value(key) };
        i64::from(with_map(map, |map| {
            map.entries
                .iter()
                .any(|(candidate_key, _)| *candidate_key == key)
        }))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_keys(map: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_values(map: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_items(map: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_entries(map: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| aurora_direct_map_items(map))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_index(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_set_index_in_place(
    map: *mut OpaqueValue,
    key: *mut OpaqueValue,
    value: *mut OpaqueValue,
    _line: i64,
    _column: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_clear_in_place(
    map: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        with_map_mut(map, |map| map.entries.clear());
        boxed_value(Value::Unit)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_map_extend_in_place(
    map: *mut OpaqueValue,
    other: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_set_empty() -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        boxed_value(Value::Set(SetValue {
            element_type: Type::named("Unknown"),
            elements: Vec::new(),
        }))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_set_len(set: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(
        || match i64::try_from(with_set(set, |set| set.elements.len())) {
            Ok(length) => length,
            Err(_) => runtime_error("set length does not fit in the direct runtime range"),
        },
    )
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_set_is_empty(set: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| i64::from(with_set(set, |set| set.elements.is_empty())))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_set_contains(
    set: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| {
        let needle = unsafe { take_value(value) };
        i64::from(with_set(set, |set| set.elements.contains(&needle)))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_set_insert_in_place(
    set: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| {
        let value = unsafe { take_value(value) };
        let inferred = inferred_collection_type(&value);
        let inserted = with_set_mut(set, |set| {
            if set.element_type == Type::named("Unknown") && inferred != Type::named("Unknown") {
                set.element_type = inferred.clone();
            }
            if set.elements.contains(&value) {
                false
            } else {
                set.elements.push(value);
                true
            }
        });
        i64::from(inserted)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_set_remove_in_place(
    set: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_set_index_option(
    set: *mut OpaqueValue,
    index: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let index = checked_vec_index(index);
        let value = with_set(set, |set| set.elements.get(index).cloned());
        boxed_value(value.map(option_some).unwrap_or_else(option_none))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_clone_value(value: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(unsafe { value_ref(value) }))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unbox_i64(value: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| match unsafe { value_ref(value) } {
        Value::Int(value) => match value.as_i128().and_then(|value| i64::try_from(value).ok()) {
            Some(value) => value,
            None => runtime_error("direct backend expected an integer that fits in host i64"),
        },
        other => runtime_error(format!(
            "direct backend expected `int32`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unbox_f64(value: *mut OpaqueValue) -> f64 {
    task_runtime_boundary(|| match unsafe { value_ref(value) } {
        Value::Float(value) => value,
        other => runtime_error(format!(
            "direct backend expected `float64`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unbox_bool(value: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| match unsafe { value_ref(value) } {
        Value::Bool(value) => i64::from(value),
        other => runtime_error(format!(
            "direct backend expected `bool`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_print_value(value: *mut OpaqueValue) {
    task_runtime_boundary(|| {
        write_stdout(unsafe { value_ref(value) }.render().as_str());
        write_stdout("\n");
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_value_as_condition(value: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| match unsafe { value_ref(value) } {
        Value::Bool(value) => i64::from(value),
        Value::Int(value) => i64::from(!value.is_zero()),
        Value::Unit => 0,
        other => runtime_error(format!(
            "direct backend cannot use `{}` as a branch condition",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unary_value(
    op: i32,
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let op = match op {
            0 => UnaryOp::Neg,
            1 => UnaryOp::Not,
            other => runtime_error(format!("unknown unary opcode `{}`", other)),
        };
        match eval_unary_value(unsafe { take_value(value) }, op) {
            Ok(value) => boxed_value(value),
            Err(error) => runtime_error(error.message),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unary_value_at(
    op: i32,
    value: *mut OpaqueValue,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_binary_value(
    op: i32,
    left: *mut OpaqueValue,
    right: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_binary_value_at(
    op: i32,
    left: *mut OpaqueValue,
    right: *mut OpaqueValue,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_cast_value(
    value: *mut OpaqueValue,
    target_ptr: *const u8,
    target_len: usize,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let target = Type::named(decode_bytes(target_ptr, target_len));
        match cast_numeric_value(unsafe { take_value(value) }, &target, None) {
            Ok(value) => boxed_value(value),
            Err(error) => runtime_error(error.message),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_cast_value_at(
    value: *mut OpaqueValue,
    target_ptr: *const u8,
    target_len: usize,
    line: i64,
    column: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let target = Type::named(decode_bytes(target_ptr, target_len));
        match cast_numeric_value(unsafe { take_value(value) }, &target, None) {
            Ok(value) => boxed_value(value),
            Err(error) => match runtime_span(line, column) {
                Some(span) => runtime_error_at(span, error.message),
                None => runtime_error(error.message),
            },
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_value_type_matches(
    value: *mut OpaqueValue,
    type_ptr: *const u8,
    type_len: usize,
) -> i64 {
    task_runtime_boundary(|| {
        let expected = decode_bytes(type_ptr, type_len);
        let actual = unsafe { value_ref(value) };
        let matches = match &actual {
            Value::Instance(instance) => instance.class_name == expected,
            Value::EnumVariant(variant) => variant.enum_name == expected,
            Value::String(_) => expected == "String",
            Value::Vec(_) => expected == "Vec",
            Value::Set(_) => expected == "Set",
            Value::Map(_) => expected == "Map",
            Value::Channel(_) => expected == "Queue",
            Value::Task(_) => expected == "Task",
            Value::TaskGroup(_) => expected == "TaskGroup",
            Value::File(_) => expected == "fs.File",
            Value::TcpListener(_) => expected == "net.TcpListener",
            Value::TcpStream(_) => expected == "net.TcpStream",
            Value::UdpSocket(_) => expected == "net.UdpSocket",
            Value::UdpDatagram(_) => expected == "net.UdpDatagram",
            Value::HttpListener(_) => expected == "net.HttpListener",
            Value::HttpExchange(_) => expected == "net.HttpExchange",
            Value::HttpResponse(_) => expected == "net.HttpResponse",
            Value::WebSocketListener(_) => expected == "net.WebSocketListener",
            Value::WebSocket(_) => expected == "net.WebSocket",
            Value::UnixListener(_) => expected == "net.UnixListener",
            Value::UnixStream(_) => expected == "net.UnixStream",
            Value::TlsListener(_) => expected == "net.TlsListener",
            Value::TlsStream(_) => expected == "net.TlsStream",
            Value::ProcessChild(_) => expected == "process.Child",
            Value::ProcessPipe(_) => expected == "process.Pipe",
            Value::ProcessCompleted(_) => expected == "process.Completed",
            Value::ProcessSupervisor(_) => expected == "process.Supervisor",
            Value::Duration(_) => expected == "Duration",
            Value::Range(_) => expected == "Range",
            Value::Bool(_) => expected == "bool",
            Value::Float(_) => expected == "float64" || expected == "float32",
            Value::Int(_) => expected.starts_with("int") || expected.starts_with("uint"),
            Value::Unit => expected == "None",
            Value::ModuleNamespace(_) => expected.starts_with("module "),
        };
        i64::from(matches)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_enum_variant(
    enum_ptr: *const u8,
    enum_len: usize,
    variant_ptr: *const u8,
    variant_len: usize,
    payloads_ptr: *mut i64,
    payload_count: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let payload_count = usize::try_from(payload_count)
            .unwrap_or_else(|_| runtime_error("invalid enum payload count"));
        boxed_value(Value::EnumVariant(EnumVariantValue {
            enum_name: decode_bytes(enum_ptr, enum_len),
            variant_name: decode_bytes(variant_ptr, variant_len),
            payloads: if payload_count == 0 {
                Vec::new()
            } else {
                unsafe { consume_opaque_buffer(payloads_ptr, payload_count) }
            },
        }))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_variant_matches(
    value: *mut OpaqueValue,
    enum_ptr: *const u8,
    enum_len: usize,
    variant_ptr: *const u8,
    variant_len: usize,
) -> i64 {
    task_runtime_boundary(|| {
        let expected_enum = decode_bytes(enum_ptr, enum_len);
        let expected_variant = decode_bytes(variant_ptr, variant_len);
        match unsafe { value_ref(value) } {
            Value::EnumVariant(variant) => i64::from(
                variant.enum_name == expected_enum && variant.variant_name == expected_variant,
            ),
            _ => 0,
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_variant_payload(
    value: *mut OpaqueValue,
    index: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(value) } {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_instance_new(
    class_ptr: *const u8,
    class_len: usize,
    names_ptr: *const *const u8,
    lens_ptr: *const usize,
    values_ptr: *const *mut OpaqueValue,
    count: usize,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_instance_empty(
    class_ptr: *const u8,
    class_len: usize,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        boxed_value(Value::Instance(InstanceValue {
            class_name: decode_bytes(class_ptr, class_len),
            fields: BTreeMap::new(),
        }))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_instance_get_field(
    value: *mut OpaqueValue,
    field_ptr: *const u8,
    field_len: usize,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_instance_set_field(
    value: *mut OpaqueValue,
    field_ptr: *const u8,
    field_len: usize,
    new_value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_arg_buffer_new(count: i64) -> *mut i64 {
    task_runtime_boundary(|| {
        let count = match usize::try_from(count) {
            Ok(count) => count,
            Err(_) => runtime_error("invalid arg buffer size"),
        };
        let mut values = vec![0i64; count].into_boxed_slice();
        let ptr = values.as_mut_ptr();
        Box::leak(values);
        ptr
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_host_builtin(
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *mut i64,
    arg_count: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let name = decode_bytes(name_ptr, name_len);
        let arg_count = usize::try_from(arg_count)
            .unwrap_or_else(|_| runtime_error("invalid host builtin argument count"));
        let args = unsafe { consume_opaque_buffer(args_ptr, arg_count) };
        match evaluate_host_builtin(&name, args) {
            Ok(value) => boxed_value(value),
            Err(error) => runtime_diagnostic_error(error),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_arg_buffer_store(buffer: *mut i64, index: i64, value: i64) {
    task_runtime_boundary(|| {
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
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_register_cleanup(
    thunk_ptr: i64,
    args: *mut i64,
    arg_count: i64,
) -> i64 {
    task_runtime_boundary(|| {
        let arg_count = match usize::try_from(arg_count) {
            Ok(arg_count) => arg_count,
            Err(_) => runtime_error("invalid cleanup arg count"),
        };
        if thunk_ptr == 0 {
            runtime_error("invalid cleanup thunk pointer");
        }
        push_direct_cleanup_registration(thunk_ptr, args, arg_count)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unregister_cleanup(id: i64) {
    task_runtime_boundary(|| {
        if let Some(registration) = take_direct_cleanup_registration(id) {
            unsafe {
                release_direct_cleanup_args(registration.args, registration.arg_count);
            }
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_refresh_cleanup(
    active: i64,
    id: i64,
    thunk_ptr: i64,
    args: *mut i64,
    arg_count: i64,
) -> i64 {
    task_runtime_boundary(|| {
        let arg_count = match usize::try_from(arg_count) {
            Ok(arg_count) => arg_count,
            Err(_) => runtime_error("invalid cleanup arg count"),
        };
        if active == 0 {
            if let Some(registration) = take_direct_cleanup_registration(id) {
                unsafe {
                    release_direct_cleanup_args(registration.args, registration.arg_count);
                }
            }
            unsafe {
                release_direct_cleanup_args(args, arg_count);
            }
            return 0;
        }
        if thunk_ptr == 0 {
            unsafe {
                release_direct_cleanup_args(args, arg_count);
            }
            if let Some(registration) = take_direct_cleanup_registration(id) {
                unsafe {
                    release_direct_cleanup_args(registration.args, registration.arg_count);
                }
            }
            runtime_error("invalid cleanup thunk pointer");
        }
        if let Some(registration) = take_direct_cleanup_registration(id) {
            unsafe {
                release_direct_cleanup_args(registration.args, registration.arg_count);
            }
        }
        push_direct_cleanup_registration(thunk_ptr, args, arg_count)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_new(capacity: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        if capacity.is_null() {
            return boxed_value(Value::Channel(ChannelValue::new()));
        }
        let capacity = expect_i32_value(
            unsafe { value_ref(capacity) }.borrow(),
            "queue(capacity=...)",
        );
        if capacity <= 0 {
            runtime_error("`queue(capacity=...)` expects a positive `int32`");
        }
        boxed_value(Value::Channel(ChannelValue::with_capacity(
            capacity as usize,
        )))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_task_group_new() -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        boxed_value(Value::TaskGroup(TaskGroupValue::new(
            &current_cancellation(),
        )))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_cancelled() -> i64 {
    task_runtime_boundary(|| {
        if poll_cancellation(&current_cancellation()) {
            1
        } else {
            0
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_send(
    channel: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(channel) } {
        Value::Channel(channel) => {
            match channel
                .send_with_cancellation(unsafe { take_value(value) }, Some(&current_cancellation()))
            {
                Ok(()) => boxed_value(result_ok(Value::Unit)),
                Err(SendValueError::Closed(value)) => {
                    boxed_value(result_err(send_error_closed(*value)))
                }
                Err(SendValueError::Cancelled(value)) => {
                    boxed_value(result_err(send_error_cancelled(*value)))
                }
                Err(SendValueError::TimedOut(value)) => {
                    boxed_value(result_err(send_error_timed_out(*value)))
                }
                Err(SendValueError::Full(value)) => {
                    boxed_value(result_err(send_error_full(*value)))
                }
            }
        }
        other => runtime_error(format!(
            "expected `Queue`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_send_timeout_value(
    channel: *mut OpaqueValue,
    value: *mut OpaqueValue,
    duration: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let millis = extract_duration_millis(unsafe { value_ref(duration) });
        let millis = match u64::try_from(millis) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid queue timeout duration"),
        };
        match unsafe { value_ref(channel) } {
            Value::Channel(channel) => match channel.send_with_timeout(
                unsafe { take_value(value) },
                Some(StdDuration::from_millis(millis)),
                Some(&current_cancellation()),
            ) {
                Ok(()) => boxed_value(result_ok(Value::Unit)),
                Err(SendValueError::Closed(value)) => {
                    boxed_value(result_err(send_error_closed(*value)))
                }
                Err(SendValueError::Cancelled(value)) => {
                    boxed_value(result_err(send_error_cancelled(*value)))
                }
                Err(SendValueError::TimedOut(value)) => {
                    boxed_value(result_err(send_error_timed_out(*value)))
                }
                Err(SendValueError::Full(value)) => {
                    boxed_value(result_err(send_error_full(*value)))
                }
            },
            other => runtime_error(format!(
                "expected `Queue`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_try_send(
    channel: *mut OpaqueValue,
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(channel) } {
        Value::Channel(channel) => match channel.try_send_result(unsafe { take_value(value) }) {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(SendValueError::Closed(value)) => {
                boxed_value(result_err(send_error_closed(*value)))
            }
            Err(SendValueError::TimedOut(value)) => {
                boxed_value(result_err(send_error_timed_out(*value)))
            }
            Err(SendValueError::Cancelled(value)) => {
                boxed_value(result_err(send_error_cancelled(*value)))
            }
            Err(SendValueError::Full(value)) => boxed_value(result_err(send_error_full(*value))),
        },
        other => runtime_error(format!(
            "expected `Queue`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_recv(channel: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(channel) } {
        Value::Channel(channel) => boxed_value(
            match channel.recv_result_with_cancellation(None, Some(&current_cancellation())) {
                RecvValueResult::Value(value) => queue_receive_item(value),
                RecvValueResult::Closed => queue_receive_closed(),
                RecvValueResult::TimedOut => queue_receive_timed_out(),
                RecvValueResult::Cancelled => queue_receive_cancelled(),
            },
        ),
        other => runtime_error(format!(
            "expected `Queue`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_recv_in_task_group(
    channel: *mut OpaqueValue,
    task_group: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let channel_value = unsafe { value_ref(channel) };
        let task_group_value = unsafe { value_ref(task_group) };
        match (channel_value, task_group_value) {
            (Value::Channel(channel), Value::TaskGroup(group)) => boxed_value(
                match recv_for_task_group_iteration(&channel, &current_cancellation(), &group) {
                    RecvValueResult::Value(value) => queue_receive_item(value),
                    RecvValueResult::Closed => queue_receive_closed(),
                    RecvValueResult::TimedOut => queue_receive_timed_out(),
                    RecvValueResult::Cancelled => queue_receive_cancelled(),
                },
            ),
            (Value::Channel(_), other) => runtime_error(format!(
                "expected `TaskGroup`, found `{}`",
                value_type_name(other)
            )),
            (other, _) => runtime_error(format!(
                "expected `Queue`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_recv_with_registered_producers(
    channel: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(channel) } {
        Value::Channel(channel) => boxed_value(
            match recv_for_registered_producers_iteration(&channel, &current_cancellation()) {
                RecvValueResult::Value(value) => queue_receive_item(value),
                RecvValueResult::Closed => queue_receive_closed(),
                RecvValueResult::TimedOut => queue_receive_timed_out(),
                RecvValueResult::Cancelled => queue_receive_cancelled(),
            },
        ),
        other => runtime_error(format!(
            "expected `Queue`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_recv_timeout_value(
    channel: *mut OpaqueValue,
    duration: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let millis = extract_duration_millis(unsafe { value_ref(duration) });
        let millis = match u64::try_from(millis) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid queue timeout duration"),
        };
        match unsafe { value_ref(channel) } {
            Value::Channel(channel) => boxed_value(
                match channel.recv_result_with_cancellation(
                    Some(StdDuration::from_millis(millis)),
                    Some(&current_cancellation()),
                ) {
                    RecvValueResult::Value(value) => queue_receive_item(value),
                    RecvValueResult::Closed => queue_receive_closed(),
                    RecvValueResult::TimedOut => queue_receive_timed_out(),
                    RecvValueResult::Cancelled => queue_receive_cancelled(),
                },
            ),
            other => runtime_error(format!(
                "expected `Queue`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_recv_or_none(
    channel: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let cancellation = current_cancellation();
        match unsafe { value_ref(channel) } {
            Value::Channel(channel) => boxed_value(
                match if cancellation.is_cancelled() {
                    RecvValueResult::Cancelled
                } else {
                    match channel.try_recv() {
                        crate::runtime_value::TryRecvResult::Value(value) => {
                            RecvValueResult::Value(value)
                        }
                        crate::runtime_value::TryRecvResult::Closed => RecvValueResult::Closed,
                        crate::runtime_value::TryRecvResult::Empty => RecvValueResult::TimedOut,
                    }
                } {
                    RecvValueResult::Value(value) => option_some(value),
                    RecvValueResult::Closed
                    | RecvValueResult::TimedOut
                    | RecvValueResult::Cancelled => option_none(),
                },
            ),
            other => runtime_error(format!(
                "expected `Queue`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_recv_or_none_timeout_value(
    channel: *mut OpaqueValue,
    duration: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let millis = extract_duration_millis(unsafe { value_ref(duration) });
        let millis = match u64::try_from(millis) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid queue timeout duration"),
        };
        match unsafe { value_ref(channel) } {
            Value::Channel(channel) => boxed_value(
                match channel.recv_result_with_cancellation(
                    Some(StdDuration::from_millis(millis)),
                    Some(&current_cancellation()),
                ) {
                    RecvValueResult::Value(value) => option_some(value),
                    RecvValueResult::Closed
                    | RecvValueResult::TimedOut
                    | RecvValueResult::Cancelled => option_none(),
                },
            ),
            other => runtime_error(format!(
                "expected `Queue`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_recv_or_value(
    channel: *mut OpaqueValue,
    default: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let cancellation = current_cancellation();
        let default = unsafe { value_ref(default) }.clone();
        match unsafe { value_ref(channel) } {
            Value::Channel(channel) => boxed_value(
                match if cancellation.is_cancelled() {
                    RecvValueResult::Cancelled
                } else {
                    match channel.try_recv() {
                        crate::runtime_value::TryRecvResult::Value(value) => {
                            RecvValueResult::Value(value)
                        }
                        crate::runtime_value::TryRecvResult::Closed => RecvValueResult::Closed,
                        crate::runtime_value::TryRecvResult::Empty => RecvValueResult::TimedOut,
                    }
                } {
                    RecvValueResult::Value(value) => value,
                    RecvValueResult::Closed
                    | RecvValueResult::TimedOut
                    | RecvValueResult::Cancelled => default,
                },
            ),
            other => runtime_error(format!(
                "expected `Queue`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_recv_or_value_timeout_value(
    channel: *mut OpaqueValue,
    default: *mut OpaqueValue,
    duration: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let default = unsafe { value_ref(default) }.clone();
        let millis = extract_duration_millis(unsafe { value_ref(duration) });
        let millis = match u64::try_from(millis) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid queue timeout duration"),
        };
        match unsafe { value_ref(channel) } {
            Value::Channel(channel) => boxed_value(
                match channel.recv_result_with_cancellation(
                    Some(StdDuration::from_millis(millis)),
                    Some(&current_cancellation()),
                ) {
                    RecvValueResult::Value(value) => value,
                    RecvValueResult::Closed
                    | RecvValueResult::TimedOut
                    | RecvValueResult::Cancelled => default,
                },
            ),
            other => runtime_error(format!(
                "expected `Queue`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_channel_close(
    channel: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(channel) } {
        Value::Channel(channel) => {
            channel.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `Queue`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_task_join(task: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(task) } {
        Value::Task(task) => {
            match task.wait_result_with_cancellation_observed(None, Some(&current_cancellation())) {
                TaskWaitStatus::Ready(result) => match result {
                    Ok(value) => boxed_value(task_result_ready(value)),
                    Err(error) => boxed_value(task_result_error(error.message)),
                },
                TaskWaitStatus::TimedOut => boxed_value(task_result_timed_out()),
                TaskWaitStatus::Cancelled => boxed_value(task_result_cancelled()),
            }
        }
        other => runtime_error(format!(
            "expected `Task`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_task_join_timeout_value(
    task: *mut OpaqueValue,
    duration: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let millis = extract_duration_millis(unsafe { value_ref(duration) });
        let millis = match u64::try_from(millis) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid task result timeout duration"),
        };
        match unsafe { value_ref(task) } {
            Value::Task(task) => match task.wait_result_with_cancellation_observed(
                Some(StdDuration::from_millis(millis)),
                Some(&current_cancellation()),
            ) {
                TaskWaitStatus::Ready(result) => match result {
                    Ok(value) => boxed_value(task_result_ready(value)),
                    Err(error) => boxed_value(task_result_error(error.message)),
                },
                TaskWaitStatus::TimedOut => boxed_value(task_result_timed_out()),
                TaskWaitStatus::Cancelled => boxed_value(task_result_cancelled()),
            },
            other => runtime_error(format!(
                "expected `Task`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_task_join_or_none(
    task: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let cancellation = current_cancellation();
        match unsafe { value_ref(task) } {
            Value::Task(task) => match if cancellation.is_cancelled() {
                TaskWaitStatus::Cancelled
            } else if let Some(result) = task.completed_result_observed() {
                match result {
                    crate::runtime_value::TaskExecutionResult::Ready(result) => {
                        TaskWaitStatus::Ready(result)
                    }
                    crate::runtime_value::TaskExecutionResult::Cancelled => {
                        TaskWaitStatus::Cancelled
                    }
                }
            } else {
                TaskWaitStatus::TimedOut
            } {
                TaskWaitStatus::Ready(result) => match result {
                    Ok(value) => boxed_value(option_some(value)),
                    Err(_) => boxed_value(option_none()),
                },
                TaskWaitStatus::TimedOut | TaskWaitStatus::Cancelled => boxed_value(option_none()),
            },
            other => runtime_error(format!(
                "expected `Task`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_task_join_or_none_timeout_value(
    task: *mut OpaqueValue,
    duration: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let millis = extract_duration_millis(unsafe { value_ref(duration) });
        let millis = match u64::try_from(millis) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid task result timeout duration"),
        };
        match unsafe { value_ref(task) } {
            Value::Task(task) => match task.wait_result_with_cancellation_observed(
                Some(StdDuration::from_millis(millis)),
                Some(&current_cancellation()),
            ) {
                TaskWaitStatus::Ready(result) => match result {
                    Ok(value) => boxed_value(option_some(value)),
                    Err(_) => boxed_value(option_none()),
                },
                TaskWaitStatus::TimedOut | TaskWaitStatus::Cancelled => boxed_value(option_none()),
            },
            other => runtime_error(format!(
                "expected `Task`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_task_join_or_value(
    task: *mut OpaqueValue,
    default: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let cancellation = current_cancellation();
        let default = unsafe { value_ref(default) }.clone();
        match unsafe { value_ref(task) } {
            Value::Task(task) => match if cancellation.is_cancelled() {
                TaskWaitStatus::Cancelled
            } else if let Some(result) = task.completed_result_observed() {
                match result {
                    crate::runtime_value::TaskExecutionResult::Ready(result) => {
                        TaskWaitStatus::Ready(result)
                    }
                    crate::runtime_value::TaskExecutionResult::Cancelled => {
                        TaskWaitStatus::Cancelled
                    }
                }
            } else {
                TaskWaitStatus::TimedOut
            } {
                TaskWaitStatus::Ready(result) => match result {
                    Ok(value) => boxed_value(value),
                    Err(_) => boxed_value(default),
                },
                TaskWaitStatus::TimedOut | TaskWaitStatus::Cancelled => boxed_value(default),
            },
            other => runtime_error(format!(
                "expected `Task`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_task_join_or_value_timeout_value(
    task: *mut OpaqueValue,
    default: *mut OpaqueValue,
    duration: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let default = unsafe { value_ref(default) }.clone();
        let millis = extract_duration_millis(unsafe { value_ref(duration) });
        let millis = match u64::try_from(millis) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid task result timeout duration"),
        };
        match unsafe { value_ref(task) } {
            Value::Task(task) => match task.wait_result_with_cancellation_observed(
                Some(StdDuration::from_millis(millis)),
                Some(&current_cancellation()),
            ) {
                TaskWaitStatus::Ready(result) => match result {
                    Ok(value) => boxed_value(value),
                    Err(_) => boxed_value(default),
                },
                TaskWaitStatus::TimedOut | TaskWaitStatus::Cancelled => boxed_value(default),
            },
            other => runtime_error(format!(
                "expected `Task`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

fn expect_task_vec(value: &Value, context: &str) -> Vec<TaskValue> {
    match value {
        Value::Vec(vector) => vector
            .elements
            .iter()
            .map(|value| match value {
                Value::Task(task) => task.clone(),
                other => runtime_error(format!(
                    "expected `{}` tasks to be `Task`, found `{}`",
                    context,
                    value_type_name(other)
                )),
            })
            .collect(),
        other => runtime_error(format!(
            "expected `{}` to receive `Vec[Task]`, found `{}`",
            context,
            value_type_name(other)
        )),
    }
}

fn wait_any_tasks(
    tasks: Vec<TaskValue>,
    timeout: Option<StdDuration>,
) -> Result<Value, Diagnostic> {
    let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));
    let cancellation = current_cancellation();
    if tasks.is_empty() {
        return if poll_cancellation(&cancellation) {
            Ok(wait_any_cancelled())
        } else {
            Ok(wait_any_timed_out())
        };
    }
    loop {
        for (index, task) in tasks.iter().enumerate() {
            if let Some(result) = task.completed_result_observed() {
                let index = i32::try_from(index)
                    .map_err(|_| Diagnostic::new("wait_any result index exceeds int32 range"))?;
                return match result {
                    crate::runtime_value::TaskExecutionResult::Ready(result) => match result {
                        Ok(value) => Ok(wait_any_ready(index, value)),
                        Err(error) => Ok(wait_any_error(index, error.message)),
                    },
                    crate::runtime_value::TaskExecutionResult::Cancelled => {
                        Ok(wait_any_cancelled())
                    }
                };
            }
        }

        match wait_for_runtime_scheduler(
            Vec::new(),
            false,
            Vec::new(),
            tasks.clone(),
            deadline,
            Some(&cancellation),
        ) {
            RuntimeSchedulerWakeReason::Ready => {}
            RuntimeSchedulerWakeReason::TimedOut => return Ok(wait_any_timed_out()),
            RuntimeSchedulerWakeReason::Cancelled => return Ok(wait_any_cancelled()),
        }
    }
}

fn wait_all_tasks(
    tasks: Vec<TaskValue>,
    timeout: Option<StdDuration>,
) -> Result<Value, Diagnostic> {
    let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));
    let cancellation = current_cancellation();
    let mut results = Vec::with_capacity(tasks.len());
    for (index, task) in tasks.into_iter().enumerate() {
        let remaining = deadline.and_then(|deadline| {
            deadline
                .checked_duration_since(Instant::now())
                .or(Some(StdDuration::from_millis(0)))
        });
        match task.wait_result_with_cancellation_observed(remaining, Some(&cancellation)) {
            TaskWaitStatus::Ready(result) => match result {
                Ok(value) => results.push(value),
                Err(error) => {
                    let index = i32::try_from(index).map_err(|_| {
                        Diagnostic::new("wait_all result index exceeds int32 range")
                    })?;
                    return Ok(wait_all_error(index, error.message));
                }
            },
            TaskWaitStatus::TimedOut => return Ok(wait_all_timed_out()),
            TaskWaitStatus::Cancelled => return Ok(wait_all_cancelled()),
        }
    }
    Ok(wait_all_ready(results))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_wait_any(tasks: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        match wait_any_tasks(
            expect_task_vec(unsafe { &value_ref(tasks) }, "wait_any"),
            None,
        ) {
            Ok(value) => boxed_value(value),
            Err(error) => runtime_diagnostic_error(error),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_wait_any_timeout_value(
    tasks: *mut OpaqueValue,
    duration: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let millis = extract_duration_millis(unsafe { value_ref(duration) });
        let millis = match u64::try_from(millis) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid wait_any timeout duration"),
        };
        match wait_any_tasks(
            expect_task_vec(unsafe { &value_ref(tasks) }, "wait_any"),
            Some(StdDuration::from_millis(millis)),
        ) {
            Ok(value) => boxed_value(value),
            Err(error) => runtime_diagnostic_error(error),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_wait_all(tasks: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        match wait_all_tasks(
            expect_task_vec(unsafe { &value_ref(tasks) }, "wait_all"),
            None,
        ) {
            Ok(value) => boxed_value(value),
            Err(error) => runtime_diagnostic_error(error),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_wait_all_timeout_value(
    tasks: *mut OpaqueValue,
    duration: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let millis = extract_duration_millis(unsafe { value_ref(duration) });
        let millis = match u64::try_from(millis) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid wait_all timeout duration"),
        };
        match wait_all_tasks(
            expect_task_vec(unsafe { &value_ref(tasks) }, "wait_all"),
            Some(StdDuration::from_millis(millis)),
        ) {
            Ok(value) => boxed_value(value),
            Err(error) => runtime_diagnostic_error(error),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_task_group_cancel(
    group: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(group) } {
        Value::TaskGroup(group) => {
            group.cancel();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `TaskGroup`, found `{}`",
            value_type_name(other)
        )),
    })
}

fn close_task_group(group: &TaskGroupValue, cancel_before: bool) {
    let tasks = group.drain_tasks();
    let cancellation = current_cancellation();
    let mut cancel_group = cancel_before;
    if !cancel_group && task_group_cleanup_should_cancel(&tasks, &cancellation) {
        cancel_group = true;
    }
    if cancel_group {
        group.cancel();
    }
    for task in tasks {
        match task.wait_result_with_cancellation(None, Some(&cancellation)) {
            TaskWaitStatus::Ready(_) => {
                if let Some(error) = task.unobserved_error() {
                    runtime_diagnostic_error(error);
                }
            }
            TaskWaitStatus::TimedOut | TaskWaitStatus::Cancelled => {}
        }
    }
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_task_group_close(
    group: *mut OpaqueValue,
    cancel_before: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(group) } {
        Value::TaskGroup(group) => {
            close_task_group(&group, cancel_before != 0);
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `TaskGroup`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_close_value(
    value: *mut OpaqueValue,
    cancel_before: i64,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        match unsafe { value_ref(value) } {
            Value::Channel(channel) => channel.close(),
            Value::TaskGroup(group) => close_task_group(&group, cancel_before != 0),
            Value::File(file) => file.close(),
            Value::TcpListener(listener) => listener.close(),
            Value::TcpStream(stream) => stream.close(),
            Value::UdpSocket(socket) => socket.close(),
            Value::HttpListener(listener) => listener.close(),
            Value::WebSocket(socket) => {
                let _ = socket.close();
            }
            Value::ProcessChild(child) => child.close(),
            Value::ProcessPipe(pipe) => pipe.close(),
            Value::ProcessSupervisor(supervisor) => supervisor.close(),
            Value::UnixListener(listener) => listener.close(),
            Value::UnixStream(stream) => stream.close(),
            Value::TlsListener(listener) => listener.close(),
            Value::TlsStream(stream) => stream.close(),
            _ => {}
        }
        boxed_value(Value::Unit)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_io_write(text: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(text) } {
        Value::String(text) => match write_stdout_result(&text) {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_io_flush() -> *mut OpaqueValue {
    task_runtime_boundary(|| match flush_stdout_result() {
        Ok(()) => boxed_value(result_ok(Value::Unit)),
        Err(error) => boxed_value(result_err(io_error(error))),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_io_read_line() -> *mut OpaqueValue {
    task_runtime_boundary(|| match io_read_line() {
        Ok(Some(line)) => boxed_value(result_ok(option_some(Value::String(line)))),
        Ok(None) => boxed_value(result_ok(option_none())),
        Err(error) => boxed_value(result_err(io_error(error))),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_exists(path: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => boxed_value(Value::Bool(std::path::Path::new(&path).exists())),
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_read_to_string(
    path: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => match run_blocking_io(
            move || {
                let bytes = read_file_limited(&path, "fs.read_to_string")?;
                String::from_utf8(bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
            },
            Some(&current_cancellation()),
        ) {
            Ok(text) => boxed_value(result_ok(Value::String(text))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_read_bytes(path: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => match run_blocking_io(
            move || read_file_limited(&path, "fs.read_bytes"),
            Some(&current_cancellation()),
        ) {
            Ok(bytes) => boxed_value(result_ok(bytes_vec_value(bytes))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_write_string(
    path: *mut OpaqueValue,
    text: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let path = match unsafe { value_ref(path) } {
            Value::String(path) => path.clone(),
            other => runtime_error(format!(
                "expected `String`, found `{}`",
                value_type_name(other)
            )),
        };
        let text = match unsafe { value_ref(text) } {
            Value::String(text) => text.clone(),
            other => runtime_error(format!(
                "expected `String`, found `{}`",
                value_type_name(other)
            )),
        };
        match run_blocking_io(
            move || std::fs::write(path, text),
            Some(&current_cancellation()),
        ) {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_write_bytes(
    path: *mut OpaqueValue,
    bytes: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let path = expect_string_value(&unsafe { value_ref(path) }, "fs.write_bytes(...)");
        let bytes = expect_bytes_value(&unsafe { value_ref(bytes) }, "fs.write_bytes(...)");
        match run_blocking_io(
            move || std::fs::write(path, bytes),
            Some(&current_cancellation()),
        ) {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_append_string(
    path: *mut OpaqueValue,
    text: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let path = match unsafe { value_ref(path) } {
            Value::String(path) => path.clone(),
            other => runtime_error(format!(
                "expected `String`, found `{}`",
                value_type_name(other)
            )),
        };
        let text = match unsafe { value_ref(text) } {
            Value::String(text) => text.clone(),
            other => runtime_error(format!(
                "expected `String`, found `{}`",
                value_type_name(other)
            )),
        };
        match run_blocking_io(
            move || {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut file| file.write_all(text.as_bytes()))
            },
            Some(&current_cancellation()),
        ) {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_append_bytes(
    path: *mut OpaqueValue,
    bytes: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let path = expect_string_value(&unsafe { value_ref(path) }, "fs.append_bytes(...)");
        let bytes = expect_bytes_value(&unsafe { value_ref(bytes) }, "fs.append_bytes(...)");
        match run_blocking_io(
            move || {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut file| file.write_all(&bytes))
            },
            Some(&current_cancellation()),
        ) {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_create_dir(path: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => match run_blocking_io(
            move || crate::runtime_value::create_dir_once(path),
            Some(&current_cancellation()),
        ) {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_read_dir(path: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => match run_blocking_io(
            move || {
                let mut names = std::fs::read_dir(path)?
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.file_name().to_string_lossy().to_string())
                    .collect::<Vec<_>>();
                names.sort();
                Ok(names)
            },
            Some(&current_cancellation()),
        ) {
            Ok(names) => boxed_value(result_ok(Value::Vec(VecValue {
                element_type: Type::named("String"),
                elements: names.into_iter().map(Value::String).collect(),
            }))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_remove_file(path: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => match run_blocking_io(
            move || crate::runtime_value::remove_file_checked(path),
            Some(&current_cancellation()),
        ) {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_open(path: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => match FileValue::open(&path) {
            Ok(file) => boxed_value(result_ok(Value::File(file))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_create(path: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => match FileValue::create(&path) {
            Ok(file) => boxed_value(result_ok(Value::File(file))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fs_append(path: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => match FileValue::append(&path) {
            Ok(file) => boxed_value(result_ok(Value::File(file))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_file_read_all(file: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(file) } {
        Value::File(file) => match file.read_all() {
            Ok(text) => boxed_value(result_ok(Value::String(text))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `fs.File`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_file_read_bytes(file: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(file) } {
        Value::File(file) => match file.read_bytes() {
            Ok(bytes) => boxed_value(result_ok(bytes_vec_value(bytes))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `fs.File`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_file_write_all(
    file: *mut OpaqueValue,
    text: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let text = match unsafe { value_ref(text) } {
            Value::String(text) => text.clone(),
            other => runtime_error(format!(
                "expected `String`, found `{}`",
                value_type_name(other)
            )),
        };
        match unsafe { value_ref(file) } {
            Value::File(file) => match file.write_all(&text) {
                Ok(()) => boxed_value(result_ok(Value::Unit)),
                Err(error) => boxed_value(result_err(io_error(error))),
            },
            other => runtime_error(format!(
                "expected `fs.File`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_file_write_bytes(
    file: *mut OpaqueValue,
    bytes: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let bytes = expect_bytes_value(&unsafe { value_ref(bytes) }, "write_bytes(...)");
        match unsafe { value_ref(file) } {
            Value::File(file) => match file.write_bytes(&bytes) {
                Ok(()) => boxed_value(result_ok(Value::Unit)),
                Err(error) => boxed_value(result_err(io_error(error))),
            },
            other => runtime_error(format!(
                "expected `fs.File`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_file_flush(file: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(file) } {
        Value::File(file) => match file.flush() {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `fs.File`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_file_close(file: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(file) } {
        Value::File(file) => {
            file.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `fs.File`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_inherit() -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(process_stdio_inherit()))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_null() -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(process_stdio_null()))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_pipe() -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(process_stdio_pipe()))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_supervisor() -> *mut OpaqueValue {
    task_runtime_boundary(|| boxed_value(Value::ProcessSupervisor(ProcessSupervisorValue::new())))
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_start(
    command: *mut OpaqueValue,
    cwd: *mut OpaqueValue,
    env: *mut OpaqueValue,
    stdin: *mut OpaqueValue,
    stdout: *mut OpaqueValue,
    stderr: *mut OpaqueValue,
    group: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let command = expect_command_vec(&unsafe { value_ref(command) }, "process.start(...)");
        if command.is_empty() {
            return boxed_value(result_err(process_error_no_command()));
        }
        let cwd = expect_optional_string_value(&unsafe { value_ref(cwd) }, "process.start(...)");
        let env = expect_headers_map(&unsafe { value_ref(env) }, "process.start(...)");
        let stdin = decode_process_stdio(&unsafe { value_ref(stdin) }, "process.start(...)")
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let stdout = decode_process_stdio(&unsafe { value_ref(stdout) }, "process.start(...)")
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let stderr = decode_process_stdio(&unsafe { value_ref(stderr) }, "process.start(...)")
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let group = expect_bool_value(&unsafe { value_ref(group) }, "process.start(...)");
        match ProcessChildValue::spawn(command, cwd, env, stdin, stdout, stderr, group) {
            Ok(child) => boxed_value(result_ok(Value::ProcessChild(child))),
            Err(error) => boxed_value(result_err(process_error_spawn(error.to_string()))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_run(
    command: *mut OpaqueValue,
    cwd: *mut OpaqueValue,
    env: *mut OpaqueValue,
    stdin: *mut OpaqueValue,
    stdout: *mut OpaqueValue,
    stderr: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
    group: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let command = expect_command_vec(&unsafe { value_ref(command) }, "process.run(...)");
        if command.is_empty() {
            return boxed_value(result_err(process_error_no_command()));
        }
        let cwd = expect_optional_string_value(&unsafe { value_ref(cwd) }, "process.run(...)");
        let env = expect_headers_map(&unsafe { value_ref(env) }, "process.run(...)");
        let stdin = decode_process_stdio(&unsafe { value_ref(stdin) }, "process.run(...)")
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let stdout = decode_process_stdio(&unsafe { value_ref(stdout) }, "process.run(...)")
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let stderr = decode_process_stdio(&unsafe { value_ref(stderr) }, "process.run(...)")
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let timeout = process_optional_timeout_from_ptr(timeout, "process.run(...)");
        let group = expect_bool_value(&unsafe { value_ref(group) }, "process.run(...)");

        let child = match ProcessChildValue::spawn(command, cwd, env, stdin, stdout, stderr, group)
        {
            Ok(child) => child,
            Err(error) => return boxed_value(result_err(process_error_spawn(error.to_string()))),
        };

        let cancellation = current_cancellation();
        let stdout_task = child
            .stdout()
            .map(|pipe| {
                let capture_cancellation = cancellation.clone();
                spawn_lightweight_task_with_cancellation(capture_cancellation.clone(), move || {
                    match pipe.read_all_bytes(Some(&capture_cancellation)) {
                        Ok(bytes) => Ok(bytes_vec_value(bytes)),
                        Err(error) => Err(Diagnostic::new(format!(
                            "process stdout capture failed: {}",
                            error
                        ))),
                    }
                })
            })
            .transpose()
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let stderr_task = child
            .stderr()
            .map(|pipe| {
                let capture_cancellation = cancellation.clone();
                spawn_lightweight_task_with_cancellation(capture_cancellation.clone(), move || {
                    match pipe.read_all_bytes(Some(&capture_cancellation)) {
                        Ok(bytes) => Ok(bytes_vec_value(bytes)),
                        Err(error) => Err(Diagnostic::new(format!(
                            "process stderr capture failed: {}",
                            error
                        ))),
                    }
                })
            })
            .transpose()
            .unwrap_or_else(|error| runtime_diagnostic_error(error));

        let status = match child.wait(timeout, Some(&cancellation)) {
            ProcessChildWaitStatus::Exited(status) => status,
            ProcessChildWaitStatus::TimedOut => {
                child.close();
                return boxed_value(result_err(process_error_timed_out()));
            }
            ProcessChildWaitStatus::Cancelled => {
                child.close();
                return boxed_value(result_err(process_error_cancelled()));
            }
            ProcessChildWaitStatus::Failed(error) => {
                child.close();
                return boxed_value(result_err(process_error_from_io(error)));
            }
        };
        let stdout = await_process_capture_task(stdout_task, "stdout");
        let stderr = await_process_capture_task(stderr_task, "stderr");
        boxed_value(result_ok(Value::ProcessCompleted(
            ProcessCompletedValue::new(
                crate::runtime_value::process_exit_status(status),
                stdout,
                stderr,
            ),
        )))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_child_stdin(
    child: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(child) } {
        Value::ProcessChild(child) => boxed_value(
            child
                .stdin()
                .map(Value::ProcessPipe)
                .map(option_some)
                .unwrap_or_else(option_none),
        ),
        other => runtime_error(format!(
            "expected `process.Child`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_child_stdout(
    child: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(child) } {
        Value::ProcessChild(child) => boxed_value(
            child
                .stdout()
                .map(Value::ProcessPipe)
                .map(option_some)
                .unwrap_or_else(option_none),
        ),
        other => runtime_error(format!(
            "expected `process.Child`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_child_stderr(
    child: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(child) } {
        Value::ProcessChild(child) => boxed_value(
            child
                .stderr()
                .map(Value::ProcessPipe)
                .map(option_some)
                .unwrap_or_else(option_none),
        ),
        other => runtime_error(format!(
            "expected `process.Child`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_child_wait(
    child: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = process_optional_timeout_from_ptr(timeout, "wait(timeout=...)");
        match unsafe { value_ref(child) } {
            Value::ProcessChild(child) => {
                boxed_value(match child.wait(timeout, Some(&current_cancellation())) {
                    ProcessChildWaitStatus::Exited(status) => process_wait_exited(status),
                    ProcessChildWaitStatus::TimedOut => process_wait_timed_out(),
                    ProcessChildWaitStatus::Cancelled => process_wait_cancelled(),
                    ProcessChildWaitStatus::Failed(error) => {
                        process_wait_failed(process_error_from_io(error))
                    }
                })
            }
            other => runtime_error(format!(
                "expected `process.Child`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_child_wait_or_none(
    child: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = process_optional_timeout_from_ptr(timeout, "wait_or_none(timeout=...)");
        match unsafe { value_ref(child) } {
            Value::ProcessChild(child) => {
                match child.wait_or_none(timeout, Some(&current_cancellation())) {
                    Ok(Some(status)) => {
                        boxed_value(result_ok(option_some(process_exit_status(status))))
                    }
                    Ok(None) => boxed_value(result_ok(option_none())),
                    Err(error) => boxed_value(result_err(error)),
                }
            }
            other => runtime_error(format!(
                "expected `process.Child`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_child_wait_ok(
    child: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = process_optional_timeout_from_ptr(timeout, "wait_ok(timeout=...)");
        match unsafe { value_ref(child) } {
            Value::ProcessChild(child) => {
                match child.wait_ok(timeout, Some(&current_cancellation())) {
                    Ok(status) => boxed_value(result_ok(process_exit_status(status))),
                    Err(error) => boxed_value(result_err(error)),
                }
            }
            other => runtime_error(format!(
                "expected `process.Child`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_child_kill(
    child: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(child) } {
        Value::ProcessChild(child) => match child.kill() {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(process_error_from_io(error))),
        },
        other => runtime_error(format!(
            "expected `process.Child`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_child_terminate(
    child: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(child) } {
        Value::ProcessChild(child) => match child.terminate() {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(process_error_from_io(error))),
        },
        other => runtime_error(format!(
            "expected `process.Child`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_child_close(
    child: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(child) } {
        Value::ProcessChild(child) => {
            child.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `process.Child`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_pipe_read_all(
    pipe: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(pipe) } {
        Value::ProcessPipe(pipe) => match pipe.read_all(Some(&current_cancellation())) {
            Ok(text) => boxed_value(result_ok(Value::String(text))),
            Err(error) => boxed_value(result_err(process_error_from_io(error))),
        },
        other => runtime_error(format!(
            "expected `process.Pipe`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_pipe_read_line(
    pipe: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = process_optional_timeout_from_ptr(timeout, "read_line(timeout=...)");
        match unsafe { value_ref(pipe) } {
            Value::ProcessPipe(pipe) => {
                match pipe.read_line(timeout, Some(&current_cancellation())) {
                    Ok(Some(text)) => boxed_value(result_ok(option_some(Value::String(text)))),
                    Ok(None) => boxed_value(result_ok(option_none())),
                    Err(error) => boxed_value(result_err(process_error_from_io(error))),
                }
            }
            other => runtime_error(format!(
                "expected `process.Pipe`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_pipe_read_bytes(
    pipe: *mut OpaqueValue,
    max_bytes: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let count = expect_i32_value(&unsafe { value_ref(max_bytes) }, "read_bytes(...)");
        let count = usize::try_from(count).unwrap_or_else(|_| {
            runtime_error("`read_bytes(...)` expects a non-negative `max_bytes`")
        });
        let timeout = process_optional_timeout_from_ptr(timeout, "read_bytes(timeout=...)");
        match unsafe { value_ref(pipe) } {
            Value::ProcessPipe(pipe) => {
                match pipe.read_bytes(count, timeout, Some(&current_cancellation())) {
                    Ok(Some(bytes)) => boxed_value(result_ok(option_some(bytes_vec_value(bytes)))),
                    Ok(None) => boxed_value(result_ok(option_none())),
                    Err(error) => boxed_value(result_err(process_error_from_io(error))),
                }
            }
            other => runtime_error(format!(
                "expected `process.Pipe`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_pipe_write_all(
    pipe: *mut OpaqueValue,
    text: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let text = expect_string_value(&unsafe { value_ref(text) }, "write_all(...)");
        let timeout = process_optional_timeout_from_ptr(timeout, "write_all(timeout=...)");
        match unsafe { value_ref(pipe) } {
            Value::ProcessPipe(pipe) => {
                match pipe.write_all(&text, timeout, Some(&current_cancellation())) {
                    Ok(()) => boxed_value(result_ok(Value::Unit)),
                    Err(error) => boxed_value(result_err(process_error_from_io(error))),
                }
            }
            other => runtime_error(format!(
                "expected `process.Pipe`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_pipe_write_bytes(
    pipe: *mut OpaqueValue,
    bytes: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let bytes = expect_bytes_value(&unsafe { value_ref(bytes) }, "write_bytes(...)");
        let timeout = process_optional_timeout_from_ptr(timeout, "write_bytes(timeout=...)");
        match unsafe { value_ref(pipe) } {
            Value::ProcessPipe(pipe) => {
                match pipe.write_bytes(&bytes, timeout, Some(&current_cancellation())) {
                    Ok(()) => boxed_value(result_ok(Value::Unit)),
                    Err(error) => boxed_value(result_err(process_error_from_io(error))),
                }
            }
            other => runtime_error(format!(
                "expected `process.Pipe`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_pipe_flush(
    pipe: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(pipe) } {
        Value::ProcessPipe(pipe) => match pipe.flush() {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(process_error_from_io(error))),
        },
        other => runtime_error(format!(
            "expected `process.Pipe`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_pipe_close(
    pipe: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(pipe) } {
        Value::ProcessPipe(pipe) => {
            pipe.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `process.Pipe`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_completed_status(
    completed: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(completed) } {
        Value::ProcessCompleted(completed) => boxed_value(completed.status()),
        other => runtime_error(format!(
            "expected `process.Completed`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_completed_success(
    completed: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| match unsafe { value_ref(completed) } {
        Value::ProcessCompleted(completed) => i64::from(completed.success()),
        other => runtime_error(format!(
            "expected `process.Completed`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_completed_stdout(
    completed: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(completed) } {
        Value::ProcessCompleted(completed) => match completed.stdout() {
            Ok(stdout) => boxed_value(Value::String(stdout)),
            Err(error) => runtime_error(error.to_string()),
        },
        other => runtime_error(format!(
            "expected `process.Completed`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_completed_stderr(
    completed: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(completed) } {
        Value::ProcessCompleted(completed) => match completed.stderr() {
            Ok(stderr) => boxed_value(Value::String(stderr)),
            Err(error) => runtime_error(error.to_string()),
        },
        other => runtime_error(format!(
            "expected `process.Completed`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_completed_stdout_bytes(
    completed: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(completed) } {
        Value::ProcessCompleted(completed) => {
            boxed_value(bytes_vec_value(completed.stdout_bytes()))
        }
        other => runtime_error(format!(
            "expected `process.Completed`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_completed_stderr_bytes(
    completed: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(completed) } {
        Value::ProcessCompleted(completed) => {
            boxed_value(bytes_vec_value(completed.stderr_bytes()))
        }
        other => runtime_error(format!(
            "expected `process.Completed`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_completed_check(
    completed: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(completed) } {
        Value::ProcessCompleted(completed) => match completed.check() {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(error)),
        },
        other => runtime_error(format!(
            "expected `process.Completed`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_supervisor_start(
    supervisor: *mut OpaqueValue,
    name: *mut OpaqueValue,
    command: *mut OpaqueValue,
    cwd: *mut OpaqueValue,
    env: *mut OpaqueValue,
    stdin: *mut OpaqueValue,
    stdout: *mut OpaqueValue,
    stderr: *mut OpaqueValue,
    restart: *mut OpaqueValue,
    backoff: *mut OpaqueValue,
    max_restarts: *mut OpaqueValue,
    group: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let name = expect_string_value(&unsafe { value_ref(name) }, "start(...)");
        let command = expect_command_vec(&unsafe { value_ref(command) }, "start(...)");
        let cwd = expect_optional_string_value(&unsafe { value_ref(cwd) }, "start(...)");
        let env = expect_headers_map(&unsafe { value_ref(env) }, "start(...)");
        let stdin = decode_process_stdio(&unsafe { value_ref(stdin) }, "start(...)")
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let stdout = decode_process_stdio(&unsafe { value_ref(stdout) }, "start(...)")
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let stderr = decode_process_stdio(&unsafe { value_ref(stderr) }, "start(...)")
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let restart = decode_process_restart_policy(&unsafe { value_ref(restart) }, "start(...)")
            .unwrap_or_else(|error| runtime_diagnostic_error(error));
        let backoff = duration_from_ptr(backoff, "start(...)");
        let max_restarts = supervisor_max_restarts_from_ptr(max_restarts, "start(...)");
        let group = expect_bool_value(&unsafe { value_ref(group) }, "start(...)");
        match unsafe { value_ref(supervisor) } {
            Value::ProcessSupervisor(supervisor) => match supervisor.start(
                name,
                command,
                cwd,
                env,
                stdin,
                stdout,
                stderr,
                restart,
                backoff,
                max_restarts,
                group,
            ) {
                Ok(()) => boxed_value(result_ok(Value::Unit)),
                Err(error) => boxed_value(result_err(error)),
            },
            other => runtime_error(format!(
                "expected `process.Supervisor`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_supervisor_wait(
    supervisor: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = process_optional_timeout_from_ptr(timeout, "wait(timeout=...)");
        match unsafe { value_ref(supervisor) } {
            Value::ProcessSupervisor(supervisor) => boxed_value(
                match supervisor.wait(timeout, Some(&current_cancellation())) {
                    ProcessSupervisorWaitStatus::Event(event) => {
                        process_supervisor_wait_event(event)
                    }
                    ProcessSupervisorWaitStatus::TimedOut => process_supervisor_wait_timed_out(),
                    ProcessSupervisorWaitStatus::Cancelled => process_supervisor_wait_cancelled(),
                },
            ),
            other => runtime_error(format!(
                "expected `process.Supervisor`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_supervisor_wait_or_none(
    supervisor: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = process_optional_timeout_from_ptr(timeout, "wait_or_none(timeout=...)");
        match unsafe { value_ref(supervisor) } {
            Value::ProcessSupervisor(supervisor) => {
                match supervisor.wait_or_none(timeout, Some(&current_cancellation())) {
                    Ok(Some(event)) => boxed_value(result_ok(option_some(event))),
                    Ok(None) => boxed_value(result_ok(option_none())),
                    Err(error) => boxed_value(result_err(error)),
                }
            }
            other => runtime_error(format!(
                "expected `process.Supervisor`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_supervisor_stop(
    supervisor: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(supervisor) } {
        Value::ProcessSupervisor(supervisor) => match supervisor.stop() {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(error)),
        },
        other => runtime_error(format!(
            "expected `process.Supervisor`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_supervisor_is_empty(
    supervisor: *mut OpaqueValue,
) -> i64 {
    task_runtime_boundary(|| match unsafe { value_ref(supervisor) } {
        Value::ProcessSupervisor(supervisor) => i64::from(supervisor.is_empty()),
        other => runtime_error(format!(
            "expected `process.Supervisor`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_process_supervisor_close(
    supervisor: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(supervisor) } {
        Value::ProcessSupervisor(supervisor) => {
            supervisor.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `process.Supervisor`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_connect(address: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(address) } {
        Value::String(address) => {
            match TcpStreamValue::connect(&address, None, Some(&current_cancellation())) {
                Ok(stream) => boxed_value(result_ok(Value::TcpStream(stream))),
                Err(error) => boxed_value(result_err(io_error(error))),
            }
        }
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_connect_timeout(
    address: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "net.connect_timeout(...)");
        match unsafe { value_ref(address) } {
            Value::String(address) => {
                match TcpStreamValue::connect(&address, timeout, Some(&current_cancellation())) {
                    Ok(stream) => boxed_value(result_ok(Value::TcpStream(stream))),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `String`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_listen(address: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(address) } {
        Value::String(address) => match TcpListenerValue::bind(&address) {
            Ok(listener) => boxed_value(result_ok(Value::TcpListener(listener))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_udp_bind(address: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(address) } {
        Value::String(address) => match UdpSocketValue::bind(&address) {
            Ok(socket) => boxed_value(result_ok(Value::UdpSocket(socket))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_unix_listen(path: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => match UnixListenerValue::bind(&path) {
            Ok(listener) => boxed_value(result_ok(Value::UnixListener(listener))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_unix_connect(
    path: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(path) } {
        Value::String(path) => {
            match UnixStreamValue::connect(&path, None, Some(&current_cancellation())) {
                Ok(stream) => boxed_value(result_ok(Value::UnixStream(stream))),
                Err(error) => boxed_value(result_err(io_error(error))),
            }
        }
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_unix_connect_timeout(
    path: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "net.unix_connect_timeout(...)");
        match unsafe { value_ref(path) } {
            Value::String(path) => {
                match UnixStreamValue::connect(&path, timeout, Some(&current_cancellation())) {
                    Ok(stream) => boxed_value(result_ok(Value::UnixStream(stream))),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `String`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_tls_listen(
    address: *mut OpaqueValue,
    cert_pem_path: *mut OpaqueValue,
    key_pem_path: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let address = expect_string_value(&unsafe { value_ref(address) }, "net.tls_listen(...)");
        let cert_pem_path =
            expect_string_value(&unsafe { value_ref(cert_pem_path) }, "net.tls_listen(...)");
        let key_pem_path =
            expect_string_value(&unsafe { value_ref(key_pem_path) }, "net.tls_listen(...)");
        match TlsListenerValue::bind(&address, &cert_pem_path, &key_pem_path) {
            Ok(listener) => boxed_value(result_ok(Value::TlsListener(listener))),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_tls_connect(
    address: *mut OpaqueValue,
    server_name: *mut OpaqueValue,
    ca_pem_path: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let address = expect_string_value(&unsafe { value_ref(address) }, "net.tls_connect(...)");
        let server_name =
            expect_string_value(&unsafe { value_ref(server_name) }, "net.tls_connect(...)");
        let ca_pem_path =
            expect_string_value(&unsafe { value_ref(ca_pem_path) }, "net.tls_connect(...)");
        match TlsStreamValue::connect(
            &address,
            &server_name,
            Some(&ca_pem_path),
            None,
            Some(&current_cancellation()),
        ) {
            Ok(stream) => boxed_value(result_ok(Value::TlsStream(stream))),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_tls_connect_timeout(
    address: *mut OpaqueValue,
    server_name: *mut OpaqueValue,
    ca_pem_path: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let address = expect_string_value(
            &unsafe { value_ref(address) },
            "net.tls_connect_timeout(...)",
        );
        let server_name = expect_string_value(
            &unsafe { value_ref(server_name) },
            "net.tls_connect_timeout(...)",
        );
        let ca_pem_path = expect_string_value(
            &unsafe { value_ref(ca_pem_path) },
            "net.tls_connect_timeout(...)",
        );
        let timeout = optional_timeout_from_ptr(timeout, "net.tls_connect_timeout(...)");
        match TlsStreamValue::connect(
            &address,
            &server_name,
            Some(&ca_pem_path),
            timeout,
            Some(&current_cancellation()),
        ) {
            Ok(stream) => boxed_value(result_ok(Value::TlsStream(stream))),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_http_listen(
    address: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(address) } {
        Value::String(address) => match HttpListenerValue::bind(&address) {
            Ok(listener) => boxed_value(result_ok(Value::HttpListener(listener))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_http_request_text(
    method: *mut OpaqueValue,
    url: *mut OpaqueValue,
    body: *mut OpaqueValue,
    headers: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let method =
            expect_string_value(&unsafe { value_ref(method) }, "net.http_request_text(...)");
        let url = expect_string_value(&unsafe { value_ref(url) }, "net.http_request_text(...)");
        let body = expect_string_value(&unsafe { value_ref(body) }, "net.http_request_text(...)");
        let headers =
            expect_headers_map(&unsafe { value_ref(headers) }, "net.http_request_text(...)");
        match HttpResponseValue::request_text(
            &method,
            &url,
            &body,
            headers,
            None,
            Some(&current_cancellation()),
        ) {
            Ok(response) => boxed_value(result_ok(Value::HttpResponse(response))),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_http_request_text_timeout(
    method: *mut OpaqueValue,
    url: *mut OpaqueValue,
    body: *mut OpaqueValue,
    headers: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let method = expect_string_value(
            &unsafe { value_ref(method) },
            "net.http_request_text_timeout(...)",
        );
        let url = expect_string_value(
            &unsafe { value_ref(url) },
            "net.http_request_text_timeout(...)",
        );
        let body = expect_string_value(
            &unsafe { value_ref(body) },
            "net.http_request_text_timeout(...)",
        );
        let headers = expect_headers_map(
            &unsafe { value_ref(headers) },
            "net.http_request_text_timeout(...)",
        );
        let timeout = optional_timeout_from_ptr(timeout, "net.http_request_text_timeout(...)");
        match HttpResponseValue::request_text(
            &method,
            &url,
            &body,
            headers,
            timeout,
            Some(&current_cancellation()),
        ) {
            Ok(response) => boxed_value(result_ok(Value::HttpResponse(response))),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_http_request_bytes(
    method: *mut OpaqueValue,
    url: *mut OpaqueValue,
    bytes: *mut OpaqueValue,
    headers: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let method =
            expect_string_value(&unsafe { value_ref(method) }, "net.http_request_bytes(...)");
        let url = expect_string_value(&unsafe { value_ref(url) }, "net.http_request_bytes(...)");
        let bytes = expect_bytes_value(&unsafe { value_ref(bytes) }, "net.http_request_bytes(...)");
        let headers = expect_headers_map(
            &unsafe { value_ref(headers) },
            "net.http_request_bytes(...)",
        );
        match HttpResponseValue::request_bytes(
            &method,
            &url,
            &bytes,
            headers,
            None,
            Some(&current_cancellation()),
        ) {
            Ok(response) => boxed_value(result_ok(Value::HttpResponse(response))),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_http_request_bytes_timeout(
    method: *mut OpaqueValue,
    url: *mut OpaqueValue,
    bytes: *mut OpaqueValue,
    headers: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let method = expect_string_value(
            &unsafe { value_ref(method) },
            "net.http_request_bytes_timeout(...)",
        );
        let url = expect_string_value(
            &unsafe { value_ref(url) },
            "net.http_request_bytes_timeout(...)",
        );
        let bytes = expect_bytes_value(
            &unsafe { value_ref(bytes) },
            "net.http_request_bytes_timeout(...)",
        );
        let headers = expect_headers_map(
            &unsafe { value_ref(headers) },
            "net.http_request_bytes_timeout(...)",
        );
        let timeout = optional_timeout_from_ptr(timeout, "net.http_request_bytes_timeout(...)");
        match HttpResponseValue::request_bytes(
            &method,
            &url,
            &bytes,
            headers,
            timeout,
            Some(&current_cancellation()),
        ) {
            Ok(response) => boxed_value(result_ok(Value::HttpResponse(response))),
            Err(error) => boxed_value(result_err(io_error(error))),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_websocket_listen(
    address: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(address) } {
        Value::String(address) => match WebSocketListenerValue::bind(&address) {
            Ok(listener) => boxed_value(result_ok(Value::WebSocketListener(listener))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_websocket_connect(
    url: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(url) } {
        Value::String(url) => match WebSocketValue::connect(&url, None) {
            Ok(socket) => boxed_value(result_ok(Value::WebSocket(socket))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `String`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_net_websocket_connect_timeout(
    url: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "net.websocket_connect_timeout(...)");
        match unsafe { value_ref(url) } {
            Value::String(url) => match WebSocketValue::connect(&url, timeout) {
                Ok(socket) => boxed_value(result_ok(Value::WebSocket(socket))),
                Err(error) => boxed_value(result_err(io_error(error))),
            },
            other => runtime_error(format!(
                "expected `String`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_listener_accept(
    listener: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "accept(timeout=...)");
        match unsafe { value_ref(listener) } {
            Value::TcpListener(listener) => {
                match listener.accept(timeout, Some(&current_cancellation())) {
                    Ok(stream) => boxed_value(result_ok(Value::TcpStream(stream))),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TcpListener`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_listener_local_addr(
    listener: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(listener) } {
        Value::TcpListener(listener) => match listener.local_addr() {
            Ok(address) => boxed_value(result_ok(Value::String(address))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.TcpListener`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_listener_close(
    listener: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(listener) } {
        Value::TcpListener(listener) => {
            listener.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `net.TcpListener`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_read_all(
    stream: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "read_all(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::TcpStream(stream) => {
                match stream.read_all(timeout, Some(&current_cancellation())) {
                    Ok(text) => boxed_value(result_ok(Value::String(text))),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TcpStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_read_line(
    stream: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "read_line(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::TcpStream(stream) => {
                match stream.read_line(timeout, Some(&current_cancellation())) {
                    Ok(Some(line)) => boxed_value(result_ok(option_some(Value::String(line)))),
                    Ok(None) => boxed_value(result_ok(option_none())),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TcpStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_read_bytes(
    stream: *mut OpaqueValue,
    max_bytes: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let max_bytes = expect_i32_value(&unsafe { value_ref(max_bytes) }, "read_bytes(...)");
        let max_bytes = usize::try_from(max_bytes).unwrap_or_else(|_| {
            runtime_error("`read_bytes(...)` requires a non-negative max_bytes")
        });
        let timeout = optional_timeout_from_ptr(timeout, "read_bytes(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::TcpStream(stream) => {
                match stream.read_bytes(max_bytes, timeout, Some(&current_cancellation())) {
                    Ok(Some(bytes)) => boxed_value(result_ok(option_some(bytes_vec_value(bytes)))),
                    Ok(None) => boxed_value(result_ok(option_none())),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TcpStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_read_exact(
    stream: *mut OpaqueValue,
    count: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let count = expect_i32_value(&unsafe { value_ref(count) }, "read_exact(...)");
        let count = usize::try_from(count)
            .unwrap_or_else(|_| runtime_error("`read_exact(...)` requires a non-negative count"));
        let timeout = optional_timeout_from_ptr(timeout, "read_exact(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::TcpStream(stream) => {
                match stream.read_exact(count, timeout, Some(&current_cancellation())) {
                    Ok(bytes) => boxed_value(result_ok(bytes_vec_value(bytes))),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TcpStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_write_all(
    stream: *mut OpaqueValue,
    text: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let text = match unsafe { value_ref(text) } {
            Value::String(text) => text.clone(),
            other => runtime_error(format!(
                "expected `String`, found `{}`",
                value_type_name(other)
            )),
        };
        let timeout = optional_timeout_from_ptr(timeout, "write_all(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::TcpStream(stream) => {
                match stream.write_all(&text, timeout, Some(&current_cancellation())) {
                    Ok(()) => boxed_value(result_ok(Value::Unit)),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TcpStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_write_bytes(
    stream: *mut OpaqueValue,
    bytes: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let bytes = expect_bytes_value(&unsafe { value_ref(bytes) }, "write_bytes(...)");
        let timeout = optional_timeout_from_ptr(timeout, "write_bytes(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::TcpStream(stream) => {
                match stream.write_bytes(&bytes, timeout, Some(&current_cancellation())) {
                    Ok(()) => boxed_value(result_ok(Value::Unit)),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TcpStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_shutdown_read(
    stream: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(stream) } {
        Value::TcpStream(stream) => match stream.shutdown_read() {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.TcpStream`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_shutdown_write(
    stream: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(stream) } {
        Value::TcpStream(stream) => match stream.shutdown_write() {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.TcpStream`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_shutdown_both(
    stream: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(stream) } {
        Value::TcpStream(stream) => match stream.shutdown_both() {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.TcpStream`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_flush(
    stream: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(stream) } {
        Value::TcpStream(stream) => match stream.flush() {
            Ok(()) => boxed_value(result_ok(Value::Unit)),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.TcpStream`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_local_addr(
    stream: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(stream) } {
        Value::TcpStream(stream) => match stream.local_addr() {
            Ok(address) => boxed_value(result_ok(Value::String(address))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.TcpStream`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_peer_addr(
    stream: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(stream) } {
        Value::TcpStream(stream) => match stream.peer_addr() {
            Ok(address) => boxed_value(result_ok(Value::String(address))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.TcpStream`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tcp_stream_close(
    stream: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(stream) } {
        Value::TcpStream(stream) => {
            stream.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `net.TcpStream`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_udp_socket_send_text(
    socket: *mut OpaqueValue,
    address: *mut OpaqueValue,
    text: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let address = expect_string_value(&unsafe { value_ref(address) }, "send_text(...)");
        let text = expect_string_value(&unsafe { value_ref(text) }, "send_text(...)");
        let timeout = optional_timeout_from_ptr(timeout, "send_text(timeout=...)");
        match unsafe { value_ref(socket) } {
            Value::UdpSocket(socket) => {
                match socket.send_to_text(&address, &text, timeout, Some(&current_cancellation())) {
                    Ok(()) => boxed_value(result_ok(Value::Unit)),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.UdpSocket`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_udp_socket_send_bytes(
    socket: *mut OpaqueValue,
    address: *mut OpaqueValue,
    bytes: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let address = expect_string_value(&unsafe { value_ref(address) }, "send_bytes(...)");
        let bytes = expect_bytes_value(&unsafe { value_ref(bytes) }, "send_bytes(...)");
        let timeout = optional_timeout_from_ptr(timeout, "send_bytes(timeout=...)");
        match unsafe { value_ref(socket) } {
            Value::UdpSocket(socket) => {
                match socket.send_to_bytes(&address, &bytes, timeout, Some(&current_cancellation()))
                {
                    Ok(()) => boxed_value(result_ok(Value::Unit)),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.UdpSocket`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_udp_socket_recv(
    socket: *mut OpaqueValue,
    max_bytes: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let max_bytes = expect_i32_value(&unsafe { value_ref(max_bytes) }, "recv(...)");
        let max_bytes = usize::try_from(max_bytes)
            .unwrap_or_else(|_| runtime_error("`recv(...)` requires a non-negative max_bytes"));
        let timeout = optional_timeout_from_ptr(timeout, "recv(timeout=...)");
        match unsafe { value_ref(socket) } {
            Value::UdpSocket(socket) => {
                match socket.recv(max_bytes, timeout, Some(&current_cancellation())) {
                    Ok(Some(bytes)) => boxed_value(result_ok(option_some(bytes_vec_value(bytes)))),
                    Ok(None) => boxed_value(result_ok(option_none())),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.UdpSocket`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_udp_socket_recv_from(
    socket: *mut OpaqueValue,
    max_bytes: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let max_bytes = expect_i32_value(&unsafe { value_ref(max_bytes) }, "recv_from(...)");
        let max_bytes = usize::try_from(max_bytes).unwrap_or_else(|_| {
            runtime_error("`recv_from(...)` requires a non-negative max_bytes")
        });
        let timeout = optional_timeout_from_ptr(timeout, "recv_from(timeout=...)");
        match unsafe { value_ref(socket) } {
            Value::UdpSocket(socket) => {
                match socket.recv_from(max_bytes, timeout, Some(&current_cancellation())) {
                    Ok(Some(datagram)) => {
                        boxed_value(result_ok(option_some(Value::UdpDatagram(datagram))))
                    }
                    Ok(None) => boxed_value(result_ok(option_none())),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.UdpSocket`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_udp_socket_local_addr(
    socket: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(socket) } {
        Value::UdpSocket(socket) => match socket.local_addr() {
            Ok(address) => boxed_value(result_ok(Value::String(address))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.UdpSocket`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_udp_socket_peer_addr(
    socket: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(socket) } {
        Value::UdpSocket(socket) => match socket.peer_addr() {
            Ok(address) => boxed_value(result_ok(Value::String(address))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.UdpSocket`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_udp_socket_close(
    socket: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(socket) } {
        Value::UdpSocket(socket) => {
            socket.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `net.UdpSocket`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_udp_datagram_address(
    datagram: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(datagram) } {
        Value::UdpDatagram(datagram) => boxed_value(Value::String(datagram.address())),
        other => runtime_error(format!(
            "expected `net.UdpDatagram`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_udp_datagram_bytes(
    datagram: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(datagram) } {
        Value::UdpDatagram(datagram) => boxed_value(bytes_vec_value(datagram.bytes())),
        other => runtime_error(format!(
            "expected `net.UdpDatagram`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_udp_datagram_text(
    datagram: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(datagram) } {
        Value::UdpDatagram(datagram) => match datagram.text() {
            Ok(text) => boxed_value(result_ok(Value::String(text))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.UdpDatagram`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_listener_accept(
    listener: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "accept(timeout=...)");
        match unsafe { value_ref(listener) } {
            Value::HttpListener(listener) => {
                match listener.accept(timeout, Some(&current_cancellation())) {
                    Ok(exchange) => boxed_value(result_ok(Value::HttpExchange(exchange))),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.HttpListener`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_listener_local_addr(
    listener: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(listener) } {
        Value::HttpListener(listener) => match listener.local_addr() {
            Ok(address) => boxed_value(result_ok(Value::String(address))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.HttpListener`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_listener_close(
    listener: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(listener) } {
        Value::HttpListener(listener) => {
            listener.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `net.HttpListener`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_exchange_method(
    exchange: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(exchange) } {
        Value::HttpExchange(exchange) => boxed_value(Value::String(exchange.method())),
        other => runtime_error(format!(
            "expected `net.HttpExchange`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_exchange_path(
    exchange: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(exchange) } {
        Value::HttpExchange(exchange) => boxed_value(Value::String(exchange.path())),
        other => runtime_error(format!(
            "expected `net.HttpExchange`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_exchange_headers(
    exchange: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(exchange) } {
        Value::HttpExchange(exchange) => boxed_value(headers_map_value(exchange.headers())),
        other => runtime_error(format!(
            "expected `net.HttpExchange`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_exchange_body_text(
    exchange: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(exchange) } {
        Value::HttpExchange(exchange) => match exchange.body_text() {
            Ok(text) => boxed_value(result_ok(Value::String(text))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.HttpExchange`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_exchange_body_bytes(
    exchange: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(exchange) } {
        Value::HttpExchange(exchange) => boxed_value(bytes_vec_value(exchange.body_bytes())),
        other => runtime_error(format!(
            "expected `net.HttpExchange`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_exchange_respond_text(
    exchange: *mut OpaqueValue,
    status: *mut OpaqueValue,
    text: *mut OpaqueValue,
    headers: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let status = expect_i32_value(&unsafe { value_ref(status) }, "respond_text(...)");
        let text = expect_string_value(&unsafe { value_ref(text) }, "respond_text(...)");
        let headers = expect_headers_map(&unsafe { value_ref(headers) }, "respond_text(...)");
        match unsafe { value_ref(exchange) } {
            Value::HttpExchange(exchange) => match exchange.respond_text(status, &text, headers) {
                Ok(()) => boxed_value(result_ok(Value::Unit)),
                Err(error) => boxed_value(result_err(io_error(error))),
            },
            other => runtime_error(format!(
                "expected `net.HttpExchange`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_exchange_respond_bytes(
    exchange: *mut OpaqueValue,
    status: *mut OpaqueValue,
    bytes: *mut OpaqueValue,
    headers: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let status = expect_i32_value(&unsafe { value_ref(status) }, "respond_bytes(...)");
        let bytes = expect_bytes_value(&unsafe { value_ref(bytes) }, "respond_bytes(...)");
        let headers = expect_headers_map(&unsafe { value_ref(headers) }, "respond_bytes(...)");
        match unsafe { value_ref(exchange) } {
            Value::HttpExchange(exchange) => {
                match exchange.respond_bytes(status, &bytes, headers) {
                    Ok(()) => boxed_value(result_ok(Value::Unit)),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.HttpExchange`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_response_status(response: *mut OpaqueValue) -> i64 {
    task_runtime_boundary(|| match unsafe { value_ref(response) } {
        Value::HttpResponse(response) => i64::from(response.status()),
        other => runtime_error(format!(
            "expected `net.HttpResponse`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_response_reason(
    response: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(response) } {
        Value::HttpResponse(response) => boxed_value(Value::String(response.reason())),
        other => runtime_error(format!(
            "expected `net.HttpResponse`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_response_headers(
    response: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(response) } {
        Value::HttpResponse(response) => boxed_value(headers_map_value(response.headers())),
        other => runtime_error(format!(
            "expected `net.HttpResponse`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_response_text(
    response: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(response) } {
        Value::HttpResponse(response) => match response.text() {
            Ok(text) => boxed_value(result_ok(Value::String(text))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.HttpResponse`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_http_response_bytes(
    response: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(response) } {
        Value::HttpResponse(response) => boxed_value(bytes_vec_value(response.bytes())),
        other => runtime_error(format!(
            "expected `net.HttpResponse`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_websocket_listener_accept(
    listener: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "accept(timeout=...)");
        match unsafe { value_ref(listener) } {
            Value::WebSocketListener(listener) => match listener.accept(timeout) {
                Ok(socket) => boxed_value(result_ok(Value::WebSocket(socket))),
                Err(error) => boxed_value(result_err(io_error(error))),
            },
            other => runtime_error(format!(
                "expected `net.WebSocketListener`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_websocket_listener_local_addr(
    listener: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(listener) } {
        Value::WebSocketListener(listener) => match listener.local_addr() {
            Ok(address) => boxed_value(result_ok(Value::String(address))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.WebSocketListener`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_websocket_send_text(
    socket: *mut OpaqueValue,
    text: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let text = expect_string_value(&unsafe { value_ref(text) }, "send_text(...)");
        let timeout = optional_timeout_from_ptr(timeout, "send_text(timeout=...)");
        match unsafe { value_ref(socket) } {
            Value::WebSocket(socket) => match socket.send_text(&text, timeout) {
                Ok(()) => boxed_value(result_ok(Value::Unit)),
                Err(error) => boxed_value(result_err(io_error(error))),
            },
            other => runtime_error(format!(
                "expected `net.WebSocket`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_websocket_send_bytes(
    socket: *mut OpaqueValue,
    bytes: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let bytes = expect_bytes_value(&unsafe { value_ref(bytes) }, "send_bytes(...)");
        let timeout = optional_timeout_from_ptr(timeout, "send_bytes(timeout=...)");
        match unsafe { value_ref(socket) } {
            Value::WebSocket(socket) => match socket.send_bytes(&bytes, timeout) {
                Ok(()) => boxed_value(result_ok(Value::Unit)),
                Err(error) => boxed_value(result_err(io_error(error))),
            },
            other => runtime_error(format!(
                "expected `net.WebSocket`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_websocket_recv_text(
    socket: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "recv_text(timeout=...)");
        match unsafe { value_ref(socket) } {
            Value::WebSocket(socket) => match socket.recv_text(timeout) {
                Ok(Some(text)) => boxed_value(result_ok(option_some(Value::String(text)))),
                Ok(None) => boxed_value(result_ok(option_none())),
                Err(error) => boxed_value(result_err(io_error(error))),
            },
            other => runtime_error(format!(
                "expected `net.WebSocket`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_websocket_recv_bytes(
    socket: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "recv_bytes(timeout=...)");
        match unsafe { value_ref(socket) } {
            Value::WebSocket(socket) => match socket.recv_bytes(timeout) {
                Ok(Some(bytes)) => boxed_value(result_ok(option_some(bytes_vec_value(bytes)))),
                Ok(None) => boxed_value(result_ok(option_none())),
                Err(error) => boxed_value(result_err(io_error(error))),
            },
            other => runtime_error(format!(
                "expected `net.WebSocket`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_websocket_close(
    socket: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(socket) } {
        Value::WebSocket(socket) => {
            let _ = socket.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `net.WebSocket`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unix_listener_accept(
    listener: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "accept(timeout=...)");
        match unsafe { value_ref(listener) } {
            Value::UnixListener(listener) => {
                match listener.accept(timeout, Some(&current_cancellation())) {
                    Ok(stream) => boxed_value(result_ok(Value::UnixStream(stream))),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.UnixListener`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unix_listener_close(
    listener: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(listener) } {
        Value::UnixListener(listener) => {
            listener.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `net.UnixListener`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unix_stream_read_line(
    stream: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "read_line(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::UnixStream(stream) => {
                match stream.read_line(timeout, Some(&current_cancellation())) {
                    Ok(Some(text)) => boxed_value(result_ok(option_some(Value::String(text)))),
                    Ok(None) => boxed_value(result_ok(option_none())),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.UnixStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unix_stream_read_exact(
    stream: *mut OpaqueValue,
    count: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let count = expect_i32_value(&unsafe { value_ref(count) }, "read_exact(...)");
        let count = usize::try_from(count)
            .unwrap_or_else(|_| runtime_error("`read_exact(...)` requires a non-negative count"));
        let timeout = optional_timeout_from_ptr(timeout, "read_exact(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::UnixStream(stream) => {
                match stream.read_exact(count, timeout, Some(&current_cancellation())) {
                    Ok(bytes) => boxed_value(result_ok(bytes_vec_value(bytes))),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.UnixStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unix_stream_write_all(
    stream: *mut OpaqueValue,
    text: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let text = expect_string_value(&unsafe { value_ref(text) }, "write_all(...)");
        let timeout = optional_timeout_from_ptr(timeout, "write_all(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::UnixStream(stream) => {
                match stream.write_all(&text, timeout, Some(&current_cancellation())) {
                    Ok(()) => boxed_value(result_ok(Value::Unit)),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.UnixStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_unix_stream_close(
    stream: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(stream) } {
        Value::UnixStream(stream) => {
            stream.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `net.UnixStream`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tls_listener_accept(
    listener: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "accept(timeout=...)");
        match unsafe { value_ref(listener) } {
            Value::TlsListener(listener) => {
                match listener.accept(timeout, Some(&current_cancellation())) {
                    Ok(stream) => boxed_value(result_ok(Value::TlsStream(stream))),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TlsListener`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tls_listener_local_addr(
    listener: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(listener) } {
        Value::TlsListener(listener) => match listener.local_addr() {
            Ok(address) => boxed_value(result_ok(Value::String(address))),
            Err(error) => boxed_value(result_err(io_error(error))),
        },
        other => runtime_error(format!(
            "expected `net.TlsListener`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tls_listener_close(
    listener: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(listener) } {
        Value::TlsListener(listener) => {
            listener.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `net.TlsListener`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tls_stream_read_line(
    stream: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let timeout = optional_timeout_from_ptr(timeout, "read_line(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::TlsStream(stream) => {
                match stream.read_line(timeout, Some(&current_cancellation())) {
                    Ok(Some(text)) => boxed_value(result_ok(option_some(Value::String(text)))),
                    Ok(None) => boxed_value(result_ok(option_none())),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TlsStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tls_stream_read_exact(
    stream: *mut OpaqueValue,
    count: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let count = expect_i32_value(&unsafe { value_ref(count) }, "read_exact(...)");
        let count = usize::try_from(count)
            .unwrap_or_else(|_| runtime_error("`read_exact(...)` requires a non-negative count"));
        let timeout = optional_timeout_from_ptr(timeout, "read_exact(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::TlsStream(stream) => {
                match stream.read_exact(count, timeout, Some(&current_cancellation())) {
                    Ok(bytes) => boxed_value(result_ok(bytes_vec_value(bytes))),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TlsStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tls_stream_write_all(
    stream: *mut OpaqueValue,
    text: *mut OpaqueValue,
    timeout: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let text = expect_string_value(&unsafe { value_ref(text) }, "write_all(...)");
        let timeout = optional_timeout_from_ptr(timeout, "write_all(timeout=...)");
        match unsafe { value_ref(stream) } {
            Value::TlsStream(stream) => {
                match stream.write_all(&text, timeout, Some(&current_cancellation())) {
                    Ok(()) => boxed_value(result_ok(Value::Unit)),
                    Err(error) => boxed_value(result_err(io_error(error))),
                }
            }
            other => runtime_error(format!(
                "expected `net.TlsStream`, found `{}`",
                value_type_name(other)
            )),
        }
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_tls_stream_close(
    stream: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| match unsafe { value_ref(stream) } {
        Value::TlsStream(stream) => {
            stream.close();
            boxed_value(Value::Unit)
        }
        other => runtime_error(format!(
            "expected `net.TlsStream`, found `{}`",
            value_type_name(other)
        )),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_sleep_ms(duration: i64) {
    task_runtime_boundary(|| {
        let millis = match u64::try_from(duration) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid sleep duration"),
        };
        let _ = sleep_with_runtime_scheduler(
            StdDuration::from_millis(millis),
            Some(&current_cancellation()),
        );
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_sleep_value(duration: *mut OpaqueValue) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let millis = extract_duration_millis(unsafe { value_ref(duration) });
        let millis = match u64::try_from(millis) {
            Ok(millis) => millis,
            Err(_) => runtime_error("invalid sleep duration"),
        };
        let _ = sleep_with_runtime_scheduler(
            StdDuration::from_millis(millis),
            Some(&current_cancellation()),
        );
        boxed_value(Value::Unit)
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aurora_direct_start_task_call(
    thunk_ptr: i64,
    args_ptr: *const i64,
    arg_count: i64,
    returns_handle: i64,
    task_group: *mut OpaqueValue,
) -> *mut OpaqueValue {
    task_runtime_boundary(|| {
        let thunk: NativeThunk = unsafe { std::mem::transmute(thunk_ptr as usize) };
        let arg_count = match usize::try_from(arg_count) {
            Ok(arg_count) => arg_count,
            Err(_) => runtime_error("invalid task-start arg count"),
        };
        let args = unsafe {
            let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                args_ptr as *mut i64,
                arg_count,
            ));
            boxed.into_vec()
        };
        let queue_producer_args = args
            .iter()
            .copied()
            .filter(|arg| *arg != 0)
            .map(|arg| unsafe { value_ref(arg as *mut OpaqueValue).clone() })
            .collect::<Vec<_>>();
        let group = if task_group.is_null() {
            runtime_error("task starting requires a `TaskGroup`")
        } else {
            match unsafe { value_ref(task_group) } {
                Value::TaskGroup(group) => group.clone(),
                other => runtime_error(format!(
                    "expected `TaskGroup`, found `{}`",
                    value_type_name(other)
                )),
            }
        };
        let cancellation = group.child_cancellation();
        let task = spawn_lightweight_task_with_cancellation(cancellation, move || {
            Ok(with_task_runtime_error_capture(|| {
                let result_ptr = unsafe { thunk(args.as_ptr(), args.len()) };
                unsafe { consume_value(result_ptr) }
            }))
        })
        .unwrap_or_else(|error| runtime_diagnostic_error(error));

        group.register_task(task.clone());
        register_task_as_queue_producer_for_values(queue_producer_args.iter(), &task);

        if returns_handle == 0 {
            return boxed_value(Value::Unit);
        }
        boxed_value(Value::Task(task))
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_sqrt_f64(value: f64) -> f64 {
    task_runtime_boundary(|| value.sqrt())
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fail_division_by_zero(line: i64, column: i64) -> ! {
    task_runtime_boundary(|| match runtime_span(line, column) {
        Some(span) => runtime_error_at(span, "division by zero"),
        None => runtime_error("division by zero"),
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aurora_direct_fail_int32_overflow(
    value: i64,
    line: i64,
    column: i64,
) -> ! {
    task_runtime_boundary(|| {
        let message = int32_overflow_message(value);
        match runtime_span(line, column) {
            Some(span) => runtime_error_at(span, message),
            None => runtime_error(message),
        }
    })
}

#[path = "native_runtime_tests.rs"]
mod tests;
