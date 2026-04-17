use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use crate::diag::{Diagnostic, Result, Span};
use crate::integer::{IntegerBounds, IntegerValue};
use crate::sema::Type;

#[derive(Clone, Debug)]
pub enum Value {
    Int(IntegerValue),
    Float(f64),
    Bool(bool),
    String(String),
    Vec(VecValue),
    Set(SetValue),
    Map(MapValue),
    Duration(i128),
    Range(RangeValue),
    ModuleNamespace(ModuleNamespaceValue),
    Unit,
    Instance(InstanceValue),
    EnumVariant(EnumVariantValue),
    Channel(ChannelValue),
    Task(TaskValue),
    TaskGroup(TaskGroupValue),
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstanceValue {
    pub class_name: String,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumVariantValue {
    pub enum_name: String,
    pub variant_name: String,
    pub payloads: Vec<Value>,
}

impl EnumVariantValue {
    pub(crate) fn single_payload(&self) -> Option<&Value> {
        match self.payloads.as_slice() {
            [payload] => Some(payload),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VecValue {
    pub element_type: Type,
    pub elements: Vec<Value>,
}

#[derive(Clone, Debug)]
pub struct SetValue {
    pub element_type: Type,
    pub elements: Vec<Value>,
}

#[derive(Clone, Debug)]
pub struct MapValue {
    pub key_type: Type,
    pub value_type: Type,
    pub entries: Vec<(Value, Value)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RangeValue {
    pub start: i128,
    pub end: i128,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModuleNamespaceValue {
    pub path: String,
}

#[derive(Clone)]
pub struct ChannelValue {
    inner: Arc<ChannelState>,
}

struct ChannelState {
    state: Mutex<ChannelInner>,
    ready: Condvar,
}

struct ChannelInner {
    queue: VecDeque<Value>,
    closed: bool,
}

#[derive(Clone)]
pub struct TaskValue {
    inner: Arc<TaskState>,
}

#[derive(Clone)]
pub struct TaskGroupValue {
    inner: Arc<TaskGroupState>,
}

struct TaskState {
    handle: Mutex<TaskHandle>,
}

struct TaskGroupState {
    tasks: Mutex<Vec<TaskValue>>,
    cancel_flag: Arc<AtomicBool>,
    parent_flags: Vec<Arc<AtomicBool>>,
}

enum TaskHandle {
    Running(Option<JoinHandle<std::result::Result<Value, String>>>),
    Completed(std::result::Result<Value, String>),
}

#[derive(Clone, Default)]
pub(crate) struct CancellationContext {
    flags: Vec<Arc<AtomicBool>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunOutput {
    pub value: Value,
    pub stdout: String,
}

pub(crate) fn cast_numeric_value(value: Value, target: &Type, span: Option<Span>) -> Result<Value> {
    fn render_target_error(span: Option<Span>, message: String) -> Diagnostic {
        match span {
            Some(span) => Diagnostic::at(span, message),
            None => Diagnostic::new(message),
        }
    }

    fn render_source_type(value: &Value) -> String {
        match value {
            Value::Int(_) | Value::Float(_) => {
                unreachable!("numeric source types are handled before render_source_type")
            }
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

    match value {
        Value::Int(value) => {
            if let Some(bounds) = crate::sema::integer_type_bounds(target) {
                if !value.fits_bounds(bounds) {
                    return Err(render_target_error(
                        span,
                        format!("integer value `{}` does not fit in `{}`", value, target),
                    ));
                }
                return Ok(Value::Int(value));
            }
            match target {
                Type::Named(name, args) if args.is_empty() && name == "float32" => {
                    let float = value.to_exact_f32().ok_or_else(|| {
                        render_target_error(
                            span,
                            format!(
                                "integer value `{}` cannot be represented exactly as `float32`",
                                value
                            ),
                        )
                    })?;
                    Ok(Value::Float(float as f64))
                }
                Type::Named(name, args) if args.is_empty() && name == "float64" => {
                    let float = value.to_exact_f64().ok_or_else(|| {
                        render_target_error(
                            span,
                            format!(
                                "integer value `{}` cannot be represented exactly as `float64`",
                                value
                            ),
                        )
                    })?;
                    Ok(Value::Float(float))
                }
                _ => Err(render_target_error(
                    span,
                    format!(
                        "casts are only supported between numeric types, found `float64` and `{}`",
                        target
                    ),
                )),
            }
        }
        Value::Float(value) => {
            if let Some(bounds) = crate::sema::integer_type_bounds(target) {
                if !value.is_finite() {
                    return Err(render_target_error(
                        span,
                        format!("cannot cast non-finite float to `{}`", target),
                    ));
                }
                let truncated = value.trunc();
                let coerced = match bounds {
                    IntegerBounds::Signed { min, max } => {
                        if truncated < min as f64 || truncated > max as f64 {
                            return Err(render_target_error(
                                span,
                                format!(
                                    "integer value `{}` does not fit in `{}`",
                                    truncated, target
                                ),
                            ));
                        }
                        IntegerValue::from_signed(truncated as i128)
                    }
                    IntegerBounds::Unsigned { max } => {
                        if truncated < 0.0 || truncated > max as f64 {
                            return Err(render_target_error(
                                span,
                                format!(
                                    "integer value `{}` does not fit in `{}`",
                                    truncated, target
                                ),
                            ));
                        }
                        IntegerValue::from_literal(truncated as u128)
                    }
                };
                if !coerced.fits_bounds(bounds) {
                    return Err(render_target_error(
                        span,
                        format!("integer value `{}` does not fit in `{}`", coerced, target),
                    ));
                }
                return Ok(Value::Int(coerced));
            }
            match target {
                Type::Named(name, args) if args.is_empty() && name == "float32" => {
                    Ok(Value::Float((value as f32) as f64))
                }
                Type::Named(name, args) if args.is_empty() && name == "float64" => {
                    Ok(Value::Float(value))
                }
                _ => Err(render_target_error(
                    span,
                    format!(
                        "casts are only supported between numeric types, found `float64` and `{}`",
                        target
                    ),
                )),
            }
        }
        other => Err(render_target_error(
            span,
            format!(
                "casts are only supported between numeric types, found `{}` and `{}`",
                render_source_type(&other),
                target
            ),
        )),
    }
}

impl fmt::Debug for ChannelValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ChannelValue(..)")
    }
}

impl fmt::Debug for TaskValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TaskValue(..)")
    }
}

impl fmt::Debug for TaskGroupValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TaskGroupValue(..)")
    }
}

impl PartialEq for ChannelValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for TaskValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for TaskGroupValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn wait_condvar<'a, T>(condvar: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    condvar
        .wait(guard)
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl PartialEq for VecValue {
    fn eq(&self, other: &Self) -> bool {
        self.elements == other.elements
    }
}

impl PartialEq for SetValue {
    fn eq(&self, other: &Self) -> bool {
        if self.elements.len() != other.elements.len() {
            return false;
        }
        for element in &self.elements {
            let mut found = false;
            for candidate in &other.elements {
                if candidate == element {
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }
        true
    }
}

impl PartialEq for MapValue {
    fn eq(&self, other: &Self) -> bool {
        if self.entries.len() != other.entries.len() {
            return false;
        }
        for (key, value) in &self.entries {
            let mut matched = false;
            for (candidate_key, candidate_value) in &other.entries {
                if candidate_key == key {
                    matched = candidate_value == value;
                    break;
                }
            }
            if !matched {
                return false;
            }
        }
        true
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(left), Value::Int(right)) => left == right,
            (Value::Duration(left), Value::Duration(right)) => left == right,
            (Value::Float(left), Value::Float(right)) => left == right,
            (Value::Bool(left), Value::Bool(right)) => left == right,
            (Value::String(left), Value::String(right)) => left == right,
            (Value::Vec(left), Value::Vec(right)) => left == right,
            (Value::Set(left), Value::Set(right)) => left == right,
            (Value::Map(left), Value::Map(right)) => left == right,
            (Value::Range(left), Value::Range(right)) => left == right,
            (Value::ModuleNamespace(left), Value::ModuleNamespace(right)) => left == right,
            (Value::Unit, Value::Unit) => true,
            (Value::Instance(left), Value::Instance(right)) => left == right,
            (Value::EnumVariant(left), Value::EnumVariant(right)) => left == right,
            (Value::Channel(left), Value::Channel(right)) => left == right,
            (Value::Task(left), Value::Task(right)) => left == right,
            (Value::TaskGroup(left), Value::TaskGroup(right)) => left == right,
            _ => false,
        }
    }
}

impl Value {
    pub fn render(&self) -> String {
        match self {
            Value::Int(value) => value.to_string(),
            Value::Float(value) => render_float(*value),
            Value::Bool(value) => value.to_string(),
            Value::String(value) => value.clone(),
            Value::Vec(values) => {
                let mut rendered = String::from("[");
                for (index, value) in values.elements.iter().enumerate() {
                    if index > 0 {
                        rendered.push_str(", ");
                    }
                    rendered.push_str(&value.render());
                }
                rendered.push(']');
                rendered
            }
            Value::Set(values) => {
                let mut rendered = String::from("Set{");
                for (index, value) in values.elements.iter().enumerate() {
                    if index > 0 {
                        rendered.push_str(", ");
                    }
                    rendered.push_str(&value.render());
                }
                rendered.push('}');
                rendered
            }
            Value::Map(map) => {
                let mut rendered = String::from("{");
                for (index, (key, value)) in map.entries.iter().enumerate() {
                    if index > 0 {
                        rendered.push_str(", ");
                    }
                    rendered.push_str(&key.render());
                    rendered.push_str(": ");
                    rendered.push_str(&value.render());
                }
                rendered.push('}');
                rendered
            }
            Value::Duration(value) => format!("{}ms", value),
            Value::Range(range) => format!("range({}, {})", range.start, range.end),
            Value::ModuleNamespace(namespace) => format!("<module {}>", namespace.path),
            Value::Unit => String::new(),
            Value::Channel(_) => "<channel>".to_string(),
            Value::Task(_) => "<task>".to_string(),
            Value::TaskGroup(_) => "<task_group>".to_string(),
            Value::Instance(instance) => {
                let mut rendered = format!("{}(", instance.class_name);
                for (index, (name, value)) in instance.fields.iter().enumerate() {
                    if index > 0 {
                        rendered.push_str(", ");
                    }
                    rendered.push_str(name);
                    rendered.push('=');
                    rendered.push_str(&value.render());
                }
                rendered.push(')');
                rendered
            }
            Value::EnumVariant(variant) => {
                let mut rendered = format!("{}.{}", variant.enum_name, variant.variant_name);
                if !variant.payloads.is_empty() {
                    rendered.push('(');
                    for (index, payload) in variant.payloads.iter().enumerate() {
                        if index > 0 {
                            rendered.push_str(", ");
                        }
                        rendered.push_str(&payload.render());
                    }
                    rendered.push(')');
                }
                rendered
            }
        }
    }
}

pub(crate) fn render_float(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let roundtripped_f32 = (value as f32) as f64;
    let mut rendered = if value == roundtripped_f32 {
        (value as f32).to_string()
    } else {
        value.to_string()
    };
    if !rendered.contains(['.', 'e', 'E']) {
        rendered.push_str(".0");
    }
    rendered
}

impl ChannelValue {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(ChannelState {
                state: Mutex::new(ChannelInner {
                    queue: VecDeque::new(),
                    closed: false,
                }),
                ready: Condvar::new(),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TryRecvResult {
    Value(Value),
    Closed,
    Empty,
}

impl ChannelValue {
    pub(crate) fn try_recv(&self) -> TryRecvResult {
        let mut state = lock_mutex(&self.inner.state);
        if let Some(value) = state.queue.pop_front() {
            return TryRecvResult::Value(value);
        }
        if state.closed {
            return TryRecvResult::Closed;
        }
        TryRecvResult::Empty
    }

    pub(crate) fn send(&self, value: Value) -> std::result::Result<(), Value> {
        let mut state = lock_mutex(&self.inner.state);
        if state.closed {
            return Err(value);
        }
        state.queue.push_back(value);
        drop(state);
        self.inner.ready.notify_one();
        Ok(())
    }

    pub(crate) fn recv_blocking(&self) -> Option<Value> {
        let mut state = lock_mutex(&self.inner.state);
        loop {
            if let Some(value) = state.queue.pop_front() {
                return Some(value);
            }
            if state.closed {
                return None;
            }
            state = wait_condvar(&self.inner.ready, state);
        }
    }

    pub(crate) fn close(&self) {
        let mut state = lock_mutex(&self.inner.state);
        state.closed = true;
        drop(state);
        self.inner.ready.notify_all();
    }
}

impl CancellationContext {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.flags.iter().any(|flag| flag.load(Ordering::SeqCst))
    }
}

impl TaskGroupValue {
    pub(crate) fn new(parent: &CancellationContext) -> Self {
        Self {
            inner: Arc::new(TaskGroupState {
                tasks: Mutex::new(Vec::new()),
                cancel_flag: Arc::new(AtomicBool::new(false)),
                parent_flags: parent.flags.clone(),
            }),
        }
    }

    pub(crate) fn child_cancellation(&self) -> CancellationContext {
        let mut flags = self.inner.parent_flags.clone();
        flags.push(self.inner.cancel_flag.clone());
        CancellationContext { flags }
    }

    // Invariant: every task must be registered before its worker thread is spawned so a later
    // drain sees the complete task set.
    pub(crate) fn register_task(&self, task: TaskValue) {
        lock_mutex(&self.inner.tasks).push(task);
    }

    pub(crate) fn cancel(&self) {
        self.inner.cancel_flag.store(true, Ordering::SeqCst);
    }

    // Invariant: callers drain only after they have finished registering tasks for the group.
    pub(crate) fn drain_tasks(&self) -> Vec<TaskValue> {
        let mut tasks = lock_mutex(&self.inner.tasks);
        std::mem::take(&mut *tasks)
    }
}

impl TaskValue {
    pub(crate) fn from_handle(handle: JoinHandle<std::result::Result<Value, String>>) -> Self {
        Self {
            inner: Arc::new(TaskState {
                handle: Mutex::new(TaskHandle::Running(Some(handle))),
            }),
        }
    }

    pub(crate) fn join_result(&self) -> std::result::Result<Value, String> {
        let handle = {
            let mut state = lock_mutex(&self.inner.handle);
            match &mut *state {
                TaskHandle::Completed(result) => return result.clone(),
                TaskHandle::Running(handle) => handle.take(),
            }
        };

        let Some(handle) = handle else {
            return Err("task join handle was not available".to_string());
        };

        let result = handle
            .join()
            .map_err(|_| "spawned task panicked".to_string())?;
        let mut state = lock_mutex(&self.inner.handle);
        *state = TaskHandle::Completed(result.clone());
        result
    }
}

pub(crate) fn option_some(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Option".to_string(),
        variant_name: "Some".to_string(),
        payloads: vec![value],
    })
}

pub(crate) fn option_none() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Option".to_string(),
        variant_name: "None".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn result_ok(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Result".to_string(),
        variant_name: "Ok".to_string(),
        payloads: vec![value],
    })
}

pub(crate) fn result_err(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Result".to_string(),
        variant_name: "Err".to_string(),
        payloads: vec![value],
    })
}

pub(crate) fn send_error_closed(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SendError".to_string(),
        variant_name: "Closed".to_string(),
        payloads: vec![value],
    })
}

#[cfg(test)]
#[path = "runtime_value_tests.rs"]
mod tests;
