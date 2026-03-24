use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration as StdDuration, Instant};

use crate::ast::{
    Argument, AssignTarget, BinaryOp, Expr, ExprKind, FunctionDecl, Param, Pattern, ReceiverKind,
    SelectArm, Stmt, UnaryOp,
};
use crate::call::{
    bind_call_arguments, callable_params_from_decl, BuiltinFunction, BuiltinMember, CallConvention,
};
use crate::diag::{Diagnostic, Result, Span};
use crate::sema::{ModuleNamespace, Program, Type};

#[derive(Clone, Debug)]
pub enum Value {
    Int(i128),
    Float(f64),
    Bool(bool),
    String(String),
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
    pub payload: Option<Box<Value>>,
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

    let cast_float = |value: f64| match target {
        Type::Named(name, args) if args.is_empty() && name == "float32" => {
            Ok(Value::Float((value as f32) as f64))
        }
        Type::Named(name, args) if args.is_empty() && name == "float64" => Ok(Value::Float(value)),
        _ => Err(render_target_error(
            span,
            format!(
                "casts are only supported between numeric types, found `float64` and `{}`",
                target
            ),
        )),
    };

    match value {
        Value::Int(value) => {
            if let Some((min, max)) = crate::sema::integer_type_bounds(target) {
                if value < min || value > max {
                    return Err(render_target_error(
                        span,
                        format!("integer value `{}` does not fit in `{}`", value, target),
                    ));
                }
                return Ok(Value::Int(value));
            }
            cast_float(value as f64)
        }
        Value::Float(value) => {
            if crate::sema::integer_type_bounds(target).is_some() {
                if !value.is_finite() {
                    return Err(render_target_error(
                        span,
                        format!("cannot cast non-finite float to `{}`", target),
                    ));
                }
                let truncated = value.trunc();
                if truncated < i128::MIN as f64 || truncated > i128::MAX as f64 {
                    return Err(render_target_error(
                        span,
                        format!("integer value `{}` does not fit in `{}`", truncated, target),
                    ));
                }
                let coerced = truncated as i128;
                let (min, max) = crate::sema::integer_type_bounds(target).unwrap();
                if coerced < min || coerced > max {
                    return Err(render_target_error(
                        span,
                        format!("integer value `{}` does not fit in `{}`", coerced, target),
                    ));
                }
                return Ok(Value::Int(coerced));
            }
            cast_float(value)
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

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(left), Value::Int(right)) => left == right,
            (Value::Duration(left), Value::Duration(right)) => left == right,
            (Value::Float(left), Value::Float(right)) => left == right,
            (Value::Bool(left), Value::Bool(right)) => left == right,
            (Value::String(left), Value::String(right)) => left == right,
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
            Value::Float(value) => {
                if value.is_finite() && value.fract() == 0.0 {
                    format!("{value:.1}")
                } else {
                    value.to_string()
                }
            }
            Value::Bool(value) => value.to_string(),
            Value::String(value) => value.clone(),
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
                if let Some(payload) = &variant.payload {
                    rendered.push('(');
                    rendered.push_str(&payload.render());
                    rendered.push(')');
                }
                rendered
            }
        }
    }
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

pub(crate) enum TryRecvResult {
    Value(Value),
    Closed,
    Empty,
}

impl ChannelValue {
    pub(crate) fn try_recv(&self) -> TryRecvResult {
        let mut state = self.inner.state.lock().unwrap();
        if let Some(value) = state.queue.pop_front() {
            return TryRecvResult::Value(value);
        }
        if state.closed {
            return TryRecvResult::Closed;
        }
        TryRecvResult::Empty
    }

    pub(crate) fn send(&self, value: Value) -> std::result::Result<(), Value> {
        let mut state = self.inner.state.lock().unwrap();
        if state.closed {
            return Err(value);
        }
        state.queue.push_back(value);
        drop(state);
        self.inner.ready.notify_one();
        Ok(())
    }

    pub(crate) fn recv_blocking(&self) -> Option<Value> {
        let mut state = self.inner.state.lock().unwrap();
        loop {
            if let Some(value) = state.queue.pop_front() {
                return Some(value);
            }
            if state.closed {
                return None;
            }
            state = self.inner.ready.wait(state).unwrap();
        }
    }

    pub(crate) fn close(&self) {
        let mut state = self.inner.state.lock().unwrap();
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

    pub(crate) fn register_task(&self, task: TaskValue) {
        self.inner.tasks.lock().unwrap().push(task);
    }

    pub(crate) fn cancel(&self) {
        self.inner.cancel_flag.store(true, Ordering::SeqCst);
    }

    pub(crate) fn drain_tasks(&self) -> Vec<TaskValue> {
        let mut tasks = self.inner.tasks.lock().unwrap();
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
            let mut state = self.inner.handle.lock().unwrap();
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
        let mut state = self.inner.handle.lock().unwrap();
        *state = TaskHandle::Completed(result.clone());
        result
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

pub fn run(program: &Program) -> Result<RunOutput> {
    let stdout = Arc::new(Mutex::new(String::new()));
    let mut interpreter = Interpreter {
        program: Arc::new(program.clone()),
        stdout: stdout.clone(),
        cancellation: CancellationContext::default(),
    };
    let value = interpreter.run_main()?;
    let rendered_stdout = stdout.lock().unwrap().clone();
    Ok(RunOutput {
        value,
        stdout: rendered_stdout,
    })
}

struct Interpreter {
    program: Arc<Program>,
    stdout: Arc<Mutex<String>>,
    cancellation: CancellationContext,
}

struct CallOutcome {
    value: Value,
    updated_receiver: Option<Value>,
    updated_params: Vec<(usize, Value)>,
}

struct EvaluatedArg<'a> {
    argument: Option<&'a Argument>,
    value: Value,
}

enum EvalOutcome {
    Value(Value),
    Return(Value),
}

enum ExecFlow {
    Continue,
    Return(Value),
    Break,
    ContinueLoop,
}

#[derive(Default)]
struct Env {
    scopes: Vec<Scope>,
}

#[derive(Default)]
struct Scope {
    values: HashMap<String, Value>,
    types: HashMap<String, Type>,
}

impl Env {
    fn with_root() -> Self {
        Self {
            scopes: vec![Scope::default()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn get(&self, name: &str) -> Option<&Value> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.values.get(name))
    }

    fn get_type(&self, name: &str) -> Option<&Type> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.types.get(name))
    }

    fn set(&mut self, name: String, value: Value) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.values.contains_key(&name) {
                scope.values.insert(name, value);
                return;
            }
        }
        self.scopes.last_mut().unwrap().values.insert(name, value);
    }

    fn define(&mut self, name: String, value: Value) {
        self.scopes.last_mut().unwrap().values.insert(name, value);
    }

    fn define_typed(&mut self, name: String, ty: Type, value: Value) {
        let scope = self.scopes.last_mut().unwrap();
        scope.types.insert(name.clone(), ty);
        scope.values.insert(name, value);
    }
}

impl Interpreter {
    fn seed_imported_modules(&self, env: &mut Env) {
        for (name, namespace) in &self.program.imported_modules {
            env.define_typed(
                name.clone(),
                Type::Module(namespace.path.clone()),
                Value::ModuleNamespace(ModuleNamespaceValue {
                    path: namespace.path.clone(),
                }),
            );
        }
    }

    fn module_namespace(&self, path: &str) -> Option<&ModuleNamespace> {
        let mut segments = path.split('.');
        let first = segments.next()?;
        let mut namespace = self.program.imported_modules.get(first)?;
        for segment in segments {
            namespace = namespace.modules.get(segment)?;
        }
        Some(namespace)
    }

    fn run_main(&mut self) -> Result<Value> {
        let Some(main_fn) = self.program.functions.get("main").cloned() else {
            if self.program.top_level_stmts.is_empty() {
                return Err(Diagnostic::new(
                    "no `main` function or top-level script statements were found",
                ));
            }
            return self.run_top_level_script();
        };

        if !main_fn.signature.params.is_empty() {
            return Err(Diagnostic::at(
                main_fn.decl.span,
                "`main` must not take parameters in the bootstrap runtime",
            ));
        }

        Ok(self.call_function(&main_fn.decl, Vec::new())?.value)
    }

    fn lower_runtime_type(type_ref: &crate::ast::TypeRef) -> Type {
        if type_ref.name == "None" {
            return Type::Unit;
        }
        Type::Named(
            type_ref.name.clone(),
            type_ref.args.iter().map(Self::lower_runtime_type).collect(),
        )
    }

    fn infer_value_type(value: &Value) -> Option<Type> {
        match value {
            Value::Int(_) => Some(Type::named("int32")),
            Value::Float(_) => Some(Type::named("float64")),
            Value::Bool(_) => Some(Type::named("bool")),
            Value::String(_) => Some(Type::named("String")),
            Value::Duration(_) => Some(Type::named("Duration")),
            Value::Range(_) => Some(Type::named("Range")),
            Value::ModuleNamespace(namespace) => Some(Type::Module(namespace.path.clone())),
            Value::Unit => Some(Type::Unit),
            Value::Instance(instance) => Some(Type::named(&instance.class_name)),
            Value::EnumVariant(variant) => match variant.enum_name.as_str() {
                "Option" => match &variant.payload {
                    Some(payload) => Self::infer_value_type(payload)
                        .map(|inner| Type::Named("Option".to_string(), vec![inner])),
                    None => None,
                },
                "Result" => match (variant.variant_name.as_str(), &variant.payload) {
                    ("Ok", Some(payload)) => Self::infer_value_type(payload).map(|ok| {
                        Type::Named("Result".to_string(), vec![ok, Type::named("Unknown")])
                    }),
                    ("Err", Some(payload)) => Self::infer_value_type(payload).map(|err| {
                        Type::Named("Result".to_string(), vec![Type::named("Unknown"), err])
                    }),
                    _ => None,
                },
                "SendError" => variant
                    .payload
                    .as_ref()
                    .and_then(|payload| Self::infer_value_type(payload))
                    .map(|value| Type::Named("SendError".to_string(), vec![value])),
                other => Some(Type::named(other)),
            },
            Value::Channel(_) | Value::Task(_) | Value::TaskGroup(_) => None,
        }
    }

    fn infer_expr_type(&self, expr: &Expr, env: &Env) -> Option<Type> {
        match &expr.kind {
            ExprKind::Name(name) if name == "None" => Some(Type::Unit),
            ExprKind::Name(name) => env
                .get_type(name)
                .cloned()
                .or_else(|| env.get(name).and_then(Self::infer_value_type)),
            ExprKind::Int(_) => Some(Type::named("int32")),
            ExprKind::DurationMillis(_) => Some(Type::named("Duration")),
            ExprKind::Float(_) => Some(Type::named("float64")),
            ExprKind::Bool(_) => Some(Type::named("bool")),
            ExprKind::String(_) => Some(Type::named("String")),
            ExprKind::Group(inner) => self.infer_expr_type(inner, env),
            ExprKind::Cast { ty, .. } => Some(Self::lower_runtime_type(ty)),
            ExprKind::Unary { op, expr } => match op {
                UnaryOp::Not => Some(Type::named("bool")),
                UnaryOp::Neg => self.infer_expr_type(expr, env),
            },
            ExprKind::Spawn { detached, value } => {
                if *detached {
                    Some(Type::Unit)
                } else if let ExprKind::Call { callee, .. } = &value.kind {
                    self.infer_call_type(callee, env)
                        .map(|inner| Type::Named("Task".to_string(), vec![inner]))
                } else {
                    None
                }
            }
            ExprKind::Try(inner) => match self.infer_expr_type(inner, env) {
                Some(Type::Named(name, mut args)) if name == "Result" && args.len() == 2 => {
                    Some(args.remove(0))
                }
                _ => None,
            },
            ExprKind::Binary { op, left, right: _ } => match op {
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Less
                | BinaryOp::LessEq
                | BinaryOp::Greater
                | BinaryOp::GreaterEq
                | BinaryOp::And
                | BinaryOp::Or => Some(Type::named("bool")),
                BinaryOp::Add if self.infer_expr_type(left, env) == Some(Type::named("String")) => {
                    Some(Type::named("String"))
                }
                _ => self.infer_expr_type(left, env),
            },
            ExprKind::Member { object, field } => {
                if let Some(Type::Named(class_name, class_args)) = self.infer_expr_type(object, env)
                {
                    if class_name == "String" && class_args.is_empty() {
                        return match field.as_str() {
                            "clone" => Some(Type::named("String")),
                            _ => None,
                        };
                    }
                    if class_name == "Channel" && class_args.len() == 1 {
                        return match field.as_str() {
                            "clone" => Some(Type::Named("Channel".to_string(), class_args)),
                            "recv" => Some(Type::Named(
                                "Option".to_string(),
                                vec![class_args[0].clone()],
                            )),
                            "send" => Some(Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Unit,
                                    Type::Named(
                                        "SendError".to_string(),
                                        vec![class_args[0].clone()],
                                    ),
                                ],
                            )),
                            "close" => Some(Type::Unit),
                            _ => None,
                        };
                    }
                    if class_name == "Task" && class_args.len() == 1 {
                        return match field.as_str() {
                            "clone" => Some(Type::Named("Task".to_string(), class_args)),
                            "join" => Some(class_args[0].clone()),
                            _ => None,
                        };
                    }
                    if class_name == "TaskGroup" && class_args.is_empty() {
                        return match field.as_str() {
                            "spawn" => Some(Type::Named("Task".to_string(), vec![Type::Unit])),
                            "cancel" | "close" => Some(Type::Unit),
                            _ => None,
                        };
                    }
                    if let Some(class_info) = self.program.classes.get(&class_name) {
                        let substitutions = crate::sema::substitutions_from_decl_type_args(
                            &class_info.decl.type_params,
                            &class_args,
                        );
                        if let Some(field_info) = class_info.fields.get(field) {
                            return Some(crate::sema::substitute_type(
                                &field_info.ty,
                                &substitutions,
                            ));
                        }
                        if let Some(method) = class_info.methods.get(field) {
                            return Some(crate::sema::substitute_type(
                                &method.signature.return_type,
                                &substitutions,
                            ));
                        }
                    }
                    if let Some(method) = self.find_trait_impl_method(
                        &Type::Named(class_name.clone(), class_args.clone()),
                        field,
                    ) {
                        return Some(method.signature.return_type.clone());
                    }
                }
                None
            }
            ExprKind::Call { callee, .. } => self.infer_call_type(callee, env),
        }
    }

    fn infer_call_type(&self, callee: &Expr, env: &Env) -> Option<Type> {
        match &callee.kind {
            ExprKind::Name(name) => {
                if let Some(builtin) = BuiltinFunction::from_name(name) {
                    return match builtin {
                        BuiltinFunction::Print => Some(Type::Unit),
                        BuiltinFunction::Range => Some(Type::named("Range")),
                        BuiltinFunction::Channel => None,
                        BuiltinFunction::TaskGroup => Some(Type::named("TaskGroup")),
                        BuiltinFunction::Cancelled => Some(Type::named("bool")),
                        BuiltinFunction::After => Some(Type::named("Duration")),
                        BuiltinFunction::Sleep => Some(Type::Unit),
                    };
                }
                if let Some(function) = self.program.functions.get(name) {
                    return Some(function.signature.return_type.clone());
                }
                if self.program.classes.contains_key(name) {
                    return Some(Type::named(name));
                }
                None
            }
            ExprKind::Member { object, field } => self.infer_expr_type(
                &Expr {
                    kind: ExprKind::Member {
                        object: object.clone(),
                        field: field.clone(),
                    },
                    span: callee.span,
                },
                env,
            ),
            _ => None,
        }
    }

    fn find_trait_impl_method(
        &self,
        receiver_ty: &Type,
        field: &str,
    ) -> Option<&crate::sema::TraitImplMethodInfo> {
        self.program.trait_impls.iter().find_map(|trait_impl| {
            if &trait_impl.for_type != receiver_ty {
                return None;
            }
            trait_impl.methods.get(field)
        })
    }

    fn find_trait_impl_method_for_class_name(
        &self,
        class_name: &str,
        field: &str,
    ) -> Option<&crate::sema::TraitImplMethodInfo> {
        let mut matches =
            self.program
                .trait_impls
                .iter()
                .filter_map(|trait_impl| match &trait_impl.for_type {
                    Type::Named(name, _) if name == class_name => trait_impl.methods.get(field),
                    _ => None,
                });
        let first = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some(first)
    }

    fn validate_value_fits_type(
        &self,
        value: &Value,
        ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        if let Some((min, max)) = crate::sema::integer_type_bounds(ty) {
            let Value::Int(value) = value else {
                return Ok(());
            };
            let value = *value as i128;
            if value < min || value > max {
                return Err(Diagnostic::at(
                    span,
                    format!("integer value `{}` does not fit in `{}`", value, ty),
                ));
            }
        }
        Ok(())
    }

    fn run_top_level_script(&mut self) -> Result<Value> {
        let mut env = Env::with_root();
        self.seed_imported_modules(&mut env);
        let top_level_stmts = self.program.top_level_stmts.clone();
        match self.exec_block(&top_level_stmts, &mut env, false)? {
            ExecFlow::Continue => Ok(Value::Int(0)),
            ExecFlow::Return(_) => unreachable!("top-level return should be rejected in sema"),
            ExecFlow::Break | ExecFlow::ContinueLoop => {
                unreachable!("top-level loop control should be rejected in sema")
            }
        }
    }

    fn call_function(&mut self, function: &FunctionDecl, args: Vec<Value>) -> Result<CallOutcome> {
        let mut env = Env::with_root();
        self.seed_imported_modules(&mut env);
        let mut values = args.into_iter();
        if function.receiver.is_some() {
            let Some(receiver) = values.next() else {
                return Err(Diagnostic::at(
                    function.span,
                    format!("method `{}` is missing its receiver", function.name),
                ));
            };
            let receiver_ty =
                Self::infer_value_type(&receiver).unwrap_or_else(|| Type::named("Unknown"));
            env.define_typed("self".to_string(), receiver_ty, receiver);
        }
        for (param, value) in function.params.iter().zip(values) {
            let ty = Self::lower_runtime_type(&param.ty);
            self.validate_value_fits_type(&value, &ty, param.span)?;
            env.define_typed(param.name.clone(), ty, value);
        }

        let value = match self.exec_block(&function.body, &mut env, false)? {
            ExecFlow::Continue => Value::Unit,
            ExecFlow::Return(value) => value,
            ExecFlow::Break | ExecFlow::ContinueLoop => {
                unreachable!("loop control outside loop should be rejected in sema")
            }
        };
        let updated_receiver = if function.receiver == Some(ReceiverKind::BorrowMut) {
            env.get("self").cloned()
        } else {
            None
        };
        let mut updated_params = Vec::new();
        for (index, param) in function.params.iter().enumerate() {
            if param.passing == ReceiverKind::BorrowMut {
                let value = env.get(&param.name).cloned().ok_or_else(|| {
                    Diagnostic::at(
                        param.span,
                        format!(
                            "mutable borrowed parameter `{}` was not available after function execution",
                            param.name
                        ),
                    )
                })?;
                updated_params.push((index, value));
            }
        }

        Ok(CallOutcome {
            value,
            updated_receiver,
            updated_params,
        })
    }

    fn exec_block(&mut self, body: &[Stmt], env: &mut Env, scoped: bool) -> Result<ExecFlow> {
        if scoped {
            env.push_scope();
        }

        for stmt in body {
            let flow = self.exec_stmt(stmt, env)?;
            if !matches!(flow, ExecFlow::Continue) {
                if scoped {
                    env.pop_scope();
                }
                return Ok(flow);
            }
        }

        if scoped {
            env.pop_scope();
        }
        Ok(ExecFlow::Continue)
    }

    fn exec_stmt(&mut self, stmt: &Stmt, env: &mut Env) -> Result<ExecFlow> {
        match stmt {
            Stmt::Assign(assign) => {
                let value = match self.eval_expr(&assign.value, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(ExecFlow::Return(value)),
                };
                let final_value = if let Some(op) = assign.op {
                    let current = self.read_assign_target(&assign.target, env, assign.span)?;
                    self.eval_binary(assign.span, op, current, value)?
                } else {
                    value
                };
                if let AssignTarget::Name(name) = &assign.target {
                    if env.get(name).is_none() {
                        let binding_ty = assign
                            .annotation
                            .as_ref()
                            .map(Self::lower_runtime_type)
                            .or_else(|| self.infer_expr_type(&assign.value, env))
                            .or_else(|| Self::infer_value_type(&final_value));
                        if let Some(binding_ty) = binding_ty {
                            self.validate_value_fits_type(&final_value, &binding_ty, assign.span)?;
                            env.define_typed(name.clone(), binding_ty, final_value);
                        } else {
                            env.define(name.clone(), final_value);
                        }
                        return Ok(ExecFlow::Continue);
                    }
                }
                self.write_assign_target(&assign.target, env, final_value, assign.span)?;
                Ok(ExecFlow::Continue)
            }
            Stmt::Pass(_) => Ok(ExecFlow::Continue),
            Stmt::Return(return_stmt) => {
                let value = if let Some(value) = &return_stmt.value {
                    match self.eval_expr(value, env)? {
                        EvalOutcome::Value(value) | EvalOutcome::Return(value) => value,
                    }
                } else {
                    Value::Unit
                };
                Ok(ExecFlow::Return(value))
            }
            Stmt::If(if_stmt) => {
                for branch in &if_stmt.branches {
                    let condition = match self.eval_expr(&branch.condition, env)? {
                        EvalOutcome::Value(value) => value,
                        EvalOutcome::Return(value) => return Ok(ExecFlow::Return(value)),
                    };
                    match condition {
                        Value::Bool(true) => return self.exec_block(&branch.body, env, true),
                        Value::Bool(false) => {}
                        other => {
                            return Err(Diagnostic::at(
                                branch.span,
                                format!(
                                    "`if` condition must evaluate to `bool`, found `{}`",
                                    other.render()
                                ),
                            ));
                        }
                    }
                }

                if let Some(else_body) = &if_stmt.else_body {
                    return self.exec_block(else_body, env, true);
                }

                Ok(ExecFlow::Continue)
            }
            Stmt::Match(match_stmt) => {
                let scrutinee = match self.eval_expr(&match_stmt.scrutinee, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(ExecFlow::Return(value)),
                };
                for arm in &match_stmt.arms {
                    if let Some(binding) = self.match_pattern(&arm.pattern, &scrutinee)? {
                        env.push_scope();
                        if let Some((name, value)) = binding {
                            let binding_ty = Self::infer_value_type(&value)
                                .unwrap_or_else(|| Type::named("Unknown"));
                            env.define_typed(name, binding_ty, value);
                        }
                        let flow = self.exec_block(&arm.body, env, false)?;
                        env.pop_scope();
                        return Ok(flow);
                    }
                }
                Err(Diagnostic::at(
                    match_stmt.span,
                    "no `match` arm matched the scrutinee at runtime",
                ))
            }
            Stmt::For(for_stmt) => {
                let iterable = match self.eval_expr(&for_stmt.iterable, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(ExecFlow::Return(value)),
                };
                let Value::Range(range) = iterable else {
                    return Err(Diagnostic::at(
                        for_stmt.span,
                        format!(
                            "`for` currently requires a `Range` iterable, found `{}`",
                            iterable.render()
                        ),
                    ));
                };
                for current in range.start..range.end {
                    env.push_scope();
                    env.define_typed(
                        for_stmt.binding.clone(),
                        Type::named("int32"),
                        Value::Int(current),
                    );
                    let flow = self.exec_block(&for_stmt.body, env, false)?;
                    env.pop_scope();
                    match flow {
                        ExecFlow::Continue => {}
                        ExecFlow::Return(value) => return Ok(ExecFlow::Return(value)),
                        ExecFlow::Break => break,
                        ExecFlow::ContinueLoop => continue,
                    }
                }
                Ok(ExecFlow::Continue)
            }
            Stmt::With(with_stmt) => {
                let resource = match self.eval_expr(&with_stmt.value, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(ExecFlow::Return(value)),
                };
                env.push_scope();
                let resource_ty = self
                    .infer_expr_type(&with_stmt.value, env)
                    .or_else(|| Self::infer_value_type(&resource));
                if let Some(resource_ty) = resource_ty {
                    env.define_typed(with_stmt.binding.clone(), resource_ty, resource);
                } else {
                    env.define(with_stmt.binding.clone(), resource);
                }
                let flow = self.exec_block(&with_stmt.body, env, false)?;
                let cancel_on_exit = !matches!(flow, ExecFlow::Continue);
                self.run_with_cleanup(&with_stmt.binding, env, with_stmt.span, cancel_on_exit)?;
                env.pop_scope();
                Ok(flow)
            }
            Stmt::Select(select_stmt) => self.exec_select(&select_stmt.arms, env),
            Stmt::While(while_stmt) => {
                loop {
                    let condition = match self.eval_expr(&while_stmt.condition, env)? {
                        EvalOutcome::Value(value) => value,
                        EvalOutcome::Return(value) => return Ok(ExecFlow::Return(value)),
                    };
                    match condition {
                        Value::Bool(true) => match self.exec_block(&while_stmt.body, env, true)? {
                            ExecFlow::Continue => {}
                            ExecFlow::Return(value) => return Ok(ExecFlow::Return(value)),
                            ExecFlow::Break => break,
                            ExecFlow::ContinueLoop => continue,
                        },
                        Value::Bool(false) => break,
                        other => {
                            return Err(Diagnostic::at(
                                while_stmt.span,
                                format!(
                                    "`while` condition must evaluate to `bool`, found `{}`",
                                    other.render()
                                ),
                            ));
                        }
                    }
                }
                Ok(ExecFlow::Continue)
            }
            Stmt::Break(_) => Ok(ExecFlow::Break),
            Stmt::Continue(_) => Ok(ExecFlow::ContinueLoop),
            Stmt::Expr(expr_stmt) => match self.eval_expr(&expr_stmt.expr, env)? {
                EvalOutcome::Value(_) => Ok(ExecFlow::Continue),
                EvalOutcome::Return(value) => Ok(ExecFlow::Return(value)),
            },
        }
    }

    fn match_pattern(
        &self,
        pattern: &Pattern,
        value: &Value,
    ) -> Result<Option<Option<(String, Value)>>> {
        match pattern {
            Pattern::Wildcard(_) => Ok(Some(None)),
            Pattern::Variant(pattern) => {
                let Value::EnumVariant(variant) = value else {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "expected enum value for pattern, found `{}`",
                            value.render()
                        ),
                    ));
                };
                if variant.enum_name != pattern.enum_name
                    || variant.variant_name != pattern.variant_name
                {
                    return Ok(None);
                }

                match (&pattern.binding, &variant.payload) {
                    (Some(name), Some(payload)) => {
                        Ok(Some(Some((name.clone(), payload.as_ref().clone()))))
                    }
                    (None, None) => Ok(Some(None)),
                    (Some(_), None) | (None, Some(_)) => Err(Diagnostic::at(
                        pattern.span,
                        "pattern payload shape did not match enum variant payload",
                    )),
                }
            }
        }
    }

    fn exec_select(&mut self, arms: &[SelectArm], env: &mut Env) -> Result<ExecFlow> {
        let deadlines = arms
            .iter()
            .map(|arm| self.prepare_select_deadline(&arm.expr, env))
            .collect::<Result<Vec<_>>>()?;

        loop {
            for (index, arm) in arms.iter().enumerate() {
                if let Some(ready_value) = self.try_select_arm(&arm.expr, env, deadlines[index])? {
                    env.push_scope();
                    if let Some(binding) = &arm.binding {
                        env.define(binding.clone(), ready_value);
                    }
                    let flow = self.exec_block(&arm.body, env, false)?;
                    env.pop_scope();
                    return Ok(flow);
                }
            }
            thread::sleep(StdDuration::from_millis(1));
        }
    }

    fn prepare_select_deadline(&mut self, expr: &Expr, env: &mut Env) -> Result<Option<Instant>> {
        let ExprKind::Call { callee, args } = &expr.kind else {
            return Err(Diagnostic::at(
                expr.span,
                "`select` currently supports `recv()`, `send(...)`, and `after(...)` arms",
            ));
        };
        match &callee.kind {
            ExprKind::Name(name) if name == "after" => {
                let ordered_args = BuiltinFunction::After.bind_args(args, expr.span)?;
                let duration_arg = ordered_args[0].expect("`after` requires exactly one argument");
                let duration = match self.eval_expr(&duration_arg.value, env)? {
                    EvalOutcome::Value(Value::Duration(value)) => value,
                    EvalOutcome::Value(other) => {
                        return Err(Diagnostic::at(
                            duration_arg.span,
                            format!(
                                "`after(...)` expects a `Duration`, found `{}`",
                                other.render()
                            ),
                        ))
                    }
                    EvalOutcome::Return(value) => {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!(
                                "`select` timer preparation cannot return early with `{}`",
                                value.render()
                            ),
                        ))
                    }
                };
                let duration = u64::try_from(duration).map_err(|_| {
                    Diagnostic::at(
                        duration_arg.span,
                        format!(
                            "duration `{}ms` does not fit in the runtime timer range",
                            duration
                        ),
                    )
                })?;
                Ok(Some(
                    Instant::now()
                        .checked_add(StdDuration::from_millis(duration))
                        .unwrap_or_else(Instant::now),
                ))
            }
            _ => Ok(None),
        }
    }

    fn try_select_arm(
        &mut self,
        expr: &Expr,
        env: &mut Env,
        deadline: Option<Instant>,
    ) -> Result<Option<Value>> {
        let ExprKind::Call { callee, args } = &expr.kind else {
            return Err(Diagnostic::at(
                expr.span,
                "`select` currently supports `recv()`, `send(...)`, and `after(...)` arms",
            ));
        };

        match &callee.kind {
            ExprKind::Name(name) if name == "after" => {
                if !args.is_empty() && deadline.is_none() {
                    return Err(Diagnostic::at(
                        expr.span,
                        "`after(...)` timer was not prepared correctly",
                    ));
                }
                if let Some(deadline) = deadline {
                    if Instant::now() >= deadline {
                        return Ok(Some(Value::Unit));
                    }
                }
                Ok(None)
            }
            ExprKind::Member { object, field } if field == "recv" => {
                BuiltinMember::ChannelRecv.bind_args(args, expr.span)?;
                let receiver = match self.eval_expr(object, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!(
                                "`select` receive arm cannot return early with `{}`",
                                value.render()
                            ),
                        ))
                    }
                };
                let Value::Channel(channel) = receiver else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "`select` receive arms require a channel receiver",
                    ));
                };
                match channel.try_recv() {
                    TryRecvResult::Value(value) => Ok(Some(option_some(value))),
                    TryRecvResult::Closed => Ok(Some(option_none())),
                    TryRecvResult::Empty => Ok(None),
                }
            }
            ExprKind::Member { object, field } if field == "send" => {
                let ordered_args = BuiltinMember::ChannelSend.bind_args(args, expr.span)?;
                let send_arg = ordered_args[0].expect("`send` requires exactly one argument");
                let receiver = match self.eval_expr(object, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!(
                                "`select` send arm cannot return early with `{}`",
                                value.render()
                            ),
                        ))
                    }
                };
                let Value::Channel(channel) = receiver else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "`select` send arms require a channel receiver",
                    ));
                };
                let value = match self.eval_expr(&send_arg.value, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!(
                                "`select` send arm cannot return early with `{}`",
                                value.render()
                            ),
                        ))
                    }
                };
                let mut state = channel.inner.state.lock().unwrap();
                if state.closed {
                    return Ok(Some(result_err(send_error_closed(value))));
                }
                state.queue.push_back(value);
                drop(state);
                channel.inner.ready.notify_one();
                Ok(Some(result_ok(Value::Unit)))
            }
            _ => Err(Diagnostic::at(
                expr.span,
                "`select` currently supports `recv()`, `send(...)`, and `after(...)` arms",
            )),
        }
    }

    fn eval_expr(&mut self, expr: &Expr, env: &mut Env) -> Result<EvalOutcome> {
        match &expr.kind {
            ExprKind::Name(name) if name == "None" => Ok(EvalOutcome::Value(Value::Unit)),
            ExprKind::Name(name) => {
                Ok(EvalOutcome::Value(env.get(name).cloned().ok_or_else(
                    || Diagnostic::at(expr.span, format!("unknown name `{}`", name)),
                )?))
            }
            ExprKind::Int(value) => Ok(EvalOutcome::Value(Value::Int(*value))),
            ExprKind::DurationMillis(value) => Ok(EvalOutcome::Value(Value::Duration(*value))),
            ExprKind::Float(value) => Ok(EvalOutcome::Value(Value::Float(*value))),
            ExprKind::Bool(value) => Ok(EvalOutcome::Value(Value::Bool(*value))),
            ExprKind::String(value) => Ok(EvalOutcome::Value(Value::String(value.clone()))),
            ExprKind::Group(inner) => self.eval_expr(inner, env),
            ExprKind::Cast { expr: value, ty } => {
                let value = match self.eval_expr(value, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                };
                let target_ty = Self::lower_runtime_type(ty);
                Ok(EvalOutcome::Value(cast_numeric_value(
                    value,
                    &target_ty,
                    Some(expr.span),
                )?))
            }
            ExprKind::Unary { op, expr: value } => {
                let value = match self.eval_expr(value, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                };
                let result = match (op, value) {
                    (UnaryOp::Not, Value::Bool(value)) => Value::Bool(!value),
                    (UnaryOp::Neg, Value::Int(value)) => Value::Int(
                        value
                            .checked_neg()
                            .ok_or_else(|| Diagnostic::at(expr.span, "integer overflow"))?,
                    ),
                    (UnaryOp::Neg, Value::Float(value)) => Value::Float(-value),
                    (UnaryOp::Not, other) => {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!("`not` expects `bool`, found `{}`", other.render()),
                        ))
                    }
                    (UnaryOp::Neg, other) => {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!(
                                "unary `-` expects a numeric value, found `{}`",
                                other.render()
                            ),
                        ))
                    }
                };
                if let Some(result_ty) = self.infer_expr_type(expr, env) {
                    self.validate_value_fits_type(&result, &result_ty, expr.span)?;
                }
                Ok(EvalOutcome::Value(result))
            }
            ExprKind::Spawn { detached, value } => {
                self.eval_spawn(*detached, value, env, expr.span)
            }
            ExprKind::Try(inner) => {
                let value = match self.eval_expr(inner, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                };
                let Value::EnumVariant(variant) = value else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "`try` requires a `Result` value at runtime",
                    ));
                };
                if variant.enum_name != "Result" {
                    return Err(Diagnostic::at(
                        expr.span,
                        "`try` requires a `Result` value at runtime",
                    ));
                }
                match (variant.variant_name.as_str(), variant.payload) {
                    ("Ok", Some(payload)) => Ok(EvalOutcome::Value(*payload)),
                    ("Err", Some(payload)) => {
                        Ok(EvalOutcome::Return(Value::EnumVariant(EnumVariantValue {
                            enum_name: "Result".to_string(),
                            variant_name: "Err".to_string(),
                            payload: Some(payload),
                        })))
                    }
                    _ => Err(Diagnostic::at(
                        expr.span,
                        "`try` encountered an invalid `Result` payload at runtime",
                    )),
                }
            }
            ExprKind::Binary { op, left, right } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let left_value = match self.eval_expr(left, env)? {
                        EvalOutcome::Value(value) => value,
                        EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                    };
                    let Value::Bool(left_bool) = left_value else {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!(
                                "logical operator expects `bool`, found `{}`",
                                left_value.render()
                            ),
                        ));
                    };

                    if *op == BinaryOp::And && !left_bool {
                        return Ok(EvalOutcome::Value(Value::Bool(false)));
                    }
                    if *op == BinaryOp::Or && left_bool {
                        return Ok(EvalOutcome::Value(Value::Bool(true)));
                    }

                    let right_value = match self.eval_expr(right, env)? {
                        EvalOutcome::Value(value) => value,
                        EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                    };
                    let Value::Bool(right_bool) = right_value else {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!(
                                "logical operator expects `bool`, found `{}`",
                                right_value.render()
                            ),
                        ));
                    };
                    return Ok(EvalOutcome::Value(Value::Bool(right_bool)));
                }

                let left_value = match self.eval_expr(left, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                };
                let right_value = match self.eval_expr(right, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                };
                let result = self.eval_binary(expr.span, *op, left_value, right_value)?;
                if let Some(result_ty) = self.infer_expr_type(expr, env) {
                    self.validate_value_fits_type(&result, &result_ty, expr.span)?;
                }
                Ok(EvalOutcome::Value(result))
            }
            ExprKind::Member { object, field } => {
                if let ExprKind::Name(enum_name) = &object.kind {
                    if matches!((enum_name.as_str(), field.as_str()), ("Option", "None")) {
                        return Ok(EvalOutcome::Value(Value::EnumVariant(EnumVariantValue {
                            enum_name: enum_name.clone(),
                            variant_name: field.clone(),
                            payload: None,
                        })));
                    }
                    if let Some(enum_info) = self.program.enums.get(enum_name) {
                        let variant = enum_info.variants.get(field).ok_or_else(|| {
                            Diagnostic::at(
                                expr.span,
                                format!("enum `{}` has no variant `{}`", enum_name, field),
                            )
                        })?;
                        if variant.payload.is_some() {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "variant `{}` of enum `{}` requires a payload",
                                    field, enum_name
                                ),
                            ));
                        }
                        return Ok(EvalOutcome::Value(Value::EnumVariant(EnumVariantValue {
                            enum_name: enum_name.clone(),
                            variant_name: field.clone(),
                            payload: None,
                        })));
                    }
                }

                let value = match self.eval_expr(object, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                };
                match value {
                    Value::ModuleNamespace(namespace) => {
                        let module = self.module_namespace(&namespace.path).ok_or_else(|| {
                            Diagnostic::at(
                                expr.span,
                                format!("unknown module namespace `{}`", namespace.path),
                            )
                        })?;
                        if let Some(child) = module.modules.get(field) {
                            Ok(EvalOutcome::Value(Value::ModuleNamespace(
                                ModuleNamespaceValue {
                                    path: child.path.clone(),
                                },
                            )))
                        } else {
                            Err(Diagnostic::at(
                                expr.span,
                                format!("module `{}` has no member `{}`", namespace.path, field),
                            ))
                        }
                    }
                    Value::Instance(instance) => Ok(EvalOutcome::Value(
                        instance.fields.get(field).cloned().ok_or_else(|| {
                            Diagnostic::at(
                                expr.span,
                                format!("class `{}` has no field `{}`", instance.class_name, field),
                            )
                        })?,
                    )),
                    _ => Err(Diagnostic::at(
                        expr.span,
                        format!("cannot access field `{}` on non-instance value", field),
                    )),
                }
            }
            ExprKind::Call { callee, args } => self.eval_call(callee, args, env),
        }
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Argument],
        env: &mut Env,
    ) -> Result<EvalOutcome> {
        match &callee.kind {
            ExprKind::Name(name) if BuiltinFunction::from_name(name).is_some() => {
                let builtin = BuiltinFunction::from_name(name).unwrap();
                let ordered_args = builtin.bind_args(args, callee.span)?;
                match builtin {
                    BuiltinFunction::Print => {
                        let value = match self.eval_expr(
                            &ordered_args[0]
                                .expect("`print` requires exactly one argument")
                                .value,
                            env,
                        )? {
                            EvalOutcome::Value(value) => value,
                            EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                        };
                        let mut stdout = self.stdout.lock().unwrap();
                        stdout.push_str(&value.render());
                        stdout.push('\n');
                        Ok(EvalOutcome::Value(Value::Unit))
                    }
                    BuiltinFunction::Range => {
                        let mut values = Vec::new();
                        for argument in ordered_args.into_iter().flatten() {
                            values.push(match self.eval_expr(&argument.value, env)? {
                                EvalOutcome::Value(value) => value,
                                EvalOutcome::Return(value) => {
                                    return Ok(EvalOutcome::Return(value))
                                }
                            });
                        }
                        let (start, end) = match values.as_slice() {
                            [Value::Int(end)] => (0, *end),
                            [Value::Int(start), Value::Int(end)] => (*start, *end),
                            _ => {
                                return Err(Diagnostic::at(
                                    callee.span,
                                    "`range` arguments must be `int32` values",
                                ))
                            }
                        };
                        Ok(EvalOutcome::Value(Value::Range(RangeValue { start, end })))
                    }
                    BuiltinFunction::Channel => {
                        Ok(EvalOutcome::Value(Value::Channel(ChannelValue::new())))
                    }
                    BuiltinFunction::TaskGroup => Ok(EvalOutcome::Value(Value::TaskGroup(
                        TaskGroupValue::new(&self.cancellation),
                    ))),
                    BuiltinFunction::Cancelled => Ok(EvalOutcome::Value(Value::Bool(
                        self.cancellation.is_cancelled(),
                    ))),
                    BuiltinFunction::After => {
                        let duration = match self.eval_expr(
                            &ordered_args[0]
                                .expect("`after` requires exactly one argument")
                                .value,
                            env,
                        )? {
                            EvalOutcome::Value(Value::Duration(value)) => value,
                            EvalOutcome::Value(other) => {
                                return Err(Diagnostic::at(
                                    ordered_args[0]
                                        .expect("`after` requires exactly one argument")
                                        .span,
                                    format!(
                                        "`after(...)` expects a `Duration`, found `{}`",
                                        other.render()
                                    ),
                                ))
                            }
                            EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                        };
                        Ok(EvalOutcome::Value(Value::Duration(duration)))
                    }
                    BuiltinFunction::Sleep => {
                        let duration = match self.eval_expr(
                            &ordered_args[0]
                                .expect("`sleep` requires exactly one argument")
                                .value,
                            env,
                        )? {
                            EvalOutcome::Value(Value::Duration(value)) => value,
                            EvalOutcome::Value(other) => {
                                return Err(Diagnostic::at(
                                    ordered_args[0]
                                        .expect("`sleep` requires exactly one argument")
                                        .span,
                                    format!(
                                        "`sleep(...)` expects a `Duration`, found `{}`",
                                        other.render()
                                    ),
                                ))
                            }
                            EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                        };
                        let duration = u64::try_from(duration).map_err(|_| {
                            Diagnostic::at(
                                ordered_args[0]
                                    .expect("`sleep` requires exactly one argument")
                                    .span,
                                format!(
                                    "duration `{}ms` does not fit in the runtime timer range",
                                    duration
                                ),
                            )
                        })?;
                        std::thread::sleep(std::time::Duration::from_millis(duration));
                        Ok(EvalOutcome::Value(Value::Unit))
                    }
                }
            }
            ExprKind::Name(name) if self.program.functions.contains_key(name) => {
                let function = self.program.functions.get(name).unwrap().decl.clone();
                let evaluated_args = self.eval_callable_args(
                    &format!("function `{}`", name),
                    &function.params,
                    args,
                    env,
                    callee.span,
                )?;
                let values = evaluated_args
                    .iter()
                    .map(|argument| argument.value.clone())
                    .collect();
                let outcome = self.call_function(&function, values)?;
                self.apply_borrowed_param_writebacks(
                    &function.params,
                    &evaluated_args,
                    &outcome.updated_params,
                    env,
                )?;
                Ok(EvalOutcome::Value(outcome.value))
            }
            ExprKind::Name(name) if self.program.classes.contains_key(name) => {
                let class_decl = self.program.classes.get(name).unwrap().decl.clone();
                let mut values = BTreeMap::new();

                for argument in args {
                    let Some(field_name) = &argument.name else {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!("constructor `{}` requires keyword arguments", name),
                        ));
                    };
                    let value = match self.eval_expr(&argument.value, env)? {
                        EvalOutcome::Value(value) => value,
                        EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                    };
                    values.insert(field_name.clone(), value);
                }

                for field in &class_decl.fields {
                    if values.contains_key(&field.name) {
                        continue;
                    }
                    if let Some(default) = &field.default {
                        values.insert(
                            field.name.clone(),
                            match self.eval_expr(default, env)? {
                                EvalOutcome::Value(value) => value,
                                EvalOutcome::Return(value) => {
                                    return Ok(EvalOutcome::Return(value))
                                }
                            },
                        );
                    } else {
                        return Err(Diagnostic::at(
                            callee.span,
                            format!("missing required field `{}` for `{}`", field.name, name),
                        ));
                    }
                }

                Ok(EvalOutcome::Value(Value::Instance(InstanceValue {
                    class_name: name.clone(),
                    fields: values,
                })))
            }
            ExprKind::Member { object, field } => {
                if let ExprKind::Name(class_name) = &object.kind {
                    if let Some(method) = self
                        .program
                        .classes
                        .get(class_name)
                        .and_then(|class_info| class_info.methods.get(field))
                        .cloned()
                    {
                        if method.decl.receiver.is_none() {
                            let evaluated_args = self.eval_callable_args(
                                &format!("method `{}`", field),
                                &method.decl.params,
                                args,
                                env,
                                callee.span,
                            )?;
                            let values = evaluated_args
                                .iter()
                                .map(|argument| argument.value.clone())
                                .collect();
                            let outcome = self.call_function(&method.decl, values)?;
                            self.apply_borrowed_param_writebacks(
                                &method.decl.params,
                                &evaluated_args,
                                &outcome.updated_params,
                                env,
                            )?;
                            return Ok(EvalOutcome::Value(outcome.value));
                        }
                    }
                }

                if let ExprKind::Name(enum_name) = &object.kind {
                    if matches!(
                        (enum_name.as_str(), field.as_str()),
                        ("Option", "Some")
                            | ("Result", "Ok")
                            | ("Result", "Err")
                            | ("SendError", "Closed")
                    ) {
                        if args.len() != 1 {
                            return Err(Diagnostic::at(
                                callee.span,
                                format!(
                                    "variant `{}` of enum `{}` expects exactly one payload argument",
                                    field, enum_name
                                ),
                            ));
                        }
                        return Ok(EvalOutcome::Value(Value::EnumVariant(EnumVariantValue {
                            enum_name: enum_name.clone(),
                            variant_name: field.clone(),
                            payload: Some(Box::new(match self.eval_expr(&args[0].value, env)? {
                                EvalOutcome::Value(value) => value,
                                EvalOutcome::Return(value) => {
                                    return Ok(EvalOutcome::Return(value))
                                }
                            })),
                        })));
                    }
                    if let Some(enum_info) = self.program.enums.get(enum_name) {
                        let variant = enum_info.variants.get(field).ok_or_else(|| {
                            Diagnostic::at(
                                callee.span,
                                format!("enum `{}` has no variant `{}`", enum_name, field),
                            )
                        })?;
                        let payload = match &variant.payload {
                            Some(_) => {
                                if args.len() != 1 {
                                    return Err(Diagnostic::at(
                                        callee.span,
                                        format!(
                                            "variant `{}` of enum `{}` expects exactly one payload argument",
                                            field, enum_name
                                        ),
                                    ));
                                }
                                Some(Box::new(match self.eval_expr(&args[0].value, env)? {
                                    EvalOutcome::Value(value) => value,
                                    EvalOutcome::Return(value) => {
                                        return Ok(EvalOutcome::Return(value))
                                    }
                                }))
                            }
                            None => {
                                return Err(Diagnostic::at(
                                    callee.span,
                                    format!(
                                        "variant `{}` of enum `{}` does not take a payload",
                                        field, enum_name
                                    ),
                                ));
                            }
                        };
                        return Ok(EvalOutcome::Value(Value::EnumVariant(EnumVariantValue {
                            enum_name: enum_name.clone(),
                            variant_name: field.clone(),
                            payload,
                        })));
                    }
                }

                let receiver_value = match self.eval_expr(object, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                };

                if let Value::Channel(channel) = &receiver_value {
                    return self.eval_channel_method(
                        channel.clone(),
                        field,
                        args,
                        env,
                        callee.span,
                    );
                }

                if let Value::Task(task) = &receiver_value {
                    return self.eval_task_method(task.clone(), field, args, callee.span);
                }

                if let Value::TaskGroup(group) = &receiver_value {
                    return self.eval_task_group_method(
                        group.clone(),
                        field,
                        args,
                        env,
                        callee.span,
                    );
                }

                match receiver_value {
                    Value::ModuleNamespace(namespace) => {
                        let module = self.module_namespace(&namespace.path).ok_or_else(|| {
                            Diagnostic::at(
                                callee.span,
                                format!("unknown module namespace `{}`", namespace.path),
                            )
                        })?;
                        if let Some(function) = module.functions.get(field).cloned() {
                            let evaluated_args = self.eval_callable_args(
                                &format!("function `{}`", function.decl.name),
                                &function.decl.params,
                                args,
                                env,
                                callee.span,
                            )?;
                            let values = evaluated_args
                                .iter()
                                .map(|argument| argument.value.clone())
                                .collect();
                            let outcome = self.call_function(&function.decl, values)?;
                            self.apply_borrowed_param_writebacks(
                                &function.decl.params,
                                &evaluated_args,
                                &outcome.updated_params,
                                env,
                            )?;
                            return Ok(EvalOutcome::Value(outcome.value));
                        }
                        if let Some(class_info) = module.classes.get(field) {
                            let class_decl = class_info.decl.clone();
                            let mut values = BTreeMap::new();

                            for argument in args {
                                let Some(field_name) = &argument.name else {
                                    return Err(Diagnostic::at(
                                        argument.span,
                                        format!(
                                            "constructor `{}` requires keyword arguments",
                                            class_decl.name
                                        ),
                                    ));
                                };
                                let value = match self.eval_expr(&argument.value, env)? {
                                    EvalOutcome::Value(value) => value,
                                    EvalOutcome::Return(value) => {
                                        return Ok(EvalOutcome::Return(value))
                                    }
                                };
                                values.insert(field_name.clone(), value);
                            }

                            for field_decl in &class_decl.fields {
                                if values.contains_key(&field_decl.name) {
                                    continue;
                                }
                                if let Some(default) = &field_decl.default {
                                    values.insert(
                                        field_decl.name.clone(),
                                        match self.eval_expr(default, env)? {
                                            EvalOutcome::Value(value) => value,
                                            EvalOutcome::Return(value) => {
                                                return Ok(EvalOutcome::Return(value))
                                            }
                                        },
                                    );
                                } else {
                                    return Err(Diagnostic::at(
                                        callee.span,
                                        format!(
                                            "missing required field `{}` for `{}`",
                                            field_decl.name, class_decl.name
                                        ),
                                    ));
                                }
                            }

                            return Ok(EvalOutcome::Value(Value::Instance(InstanceValue {
                                class_name: class_decl.name,
                                fields: values,
                            })));
                        }
                        Err(Diagnostic::at(
                            callee.span,
                            format!(
                                "module `{}` has no callable member `{}`",
                                namespace.path, field
                            ),
                        ))
                    }
                    Value::Instance(instance) => {
                        if let Some(method) = self
                            .program
                            .classes
                            .get(&instance.class_name)
                            .and_then(|class_info| class_info.methods.get(field))
                            .cloned()
                        {
                            if method.decl.receiver.is_some() {
                                let mut values = vec![Value::Instance(instance)];
                                let evaluated_args = self.eval_callable_args(
                                    &format!("method `{}`", field),
                                    &method.decl.params,
                                    args,
                                    env,
                                    callee.span,
                                )?;
                                values.extend(
                                    evaluated_args.iter().map(|argument| argument.value.clone()),
                                );
                                let outcome = self.call_function(&method.decl, values)?;
                                if method.decl.receiver == Some(ReceiverKind::BorrowMut) {
                                    let updated_receiver =
                                            outcome.updated_receiver.ok_or_else(|| {
                                                Diagnostic::at(
                                                    callee.span,
                                                    format!(
                                                        "method `{}` did not produce an updated mutable receiver",
                                                        field
                                                    ),
                                                )
                                            })?;
                                    self.write_place_expr(object, env, updated_receiver)?;
                                }
                                self.apply_borrowed_param_writebacks(
                                    &method.decl.params,
                                    &evaluated_args,
                                    &outcome.updated_params,
                                    env,
                                )?;
                                return Ok(EvalOutcome::Value(outcome.value));
                            }
                        }
                        let resolved_receiver_ty = self
                            .infer_expr_type(object, env)
                            .filter(|ty| !matches!(ty, Type::TypeParam(_)))
                            .unwrap_or_else(|| Type::named(&instance.class_name));
                        if let Some(method) = self
                            .find_trait_impl_method(&resolved_receiver_ty, field)
                            .or_else(|| {
                                self.find_trait_impl_method_for_class_name(
                                    &instance.class_name,
                                    field,
                                )
                            })
                            .cloned()
                        {
                            let mut values = vec![Value::Instance(instance)];
                            let evaluated_args = self.eval_callable_args(
                                &format!("method `{}`", field),
                                &method.decl.params,
                                args,
                                env,
                                callee.span,
                            )?;
                            values.extend(
                                evaluated_args.iter().map(|argument| argument.value.clone()),
                            );
                            let outcome = self.call_function(&method.decl, values)?;
                            if method.decl.receiver == Some(ReceiverKind::BorrowMut) {
                                let updated_receiver =
                                    outcome.updated_receiver.ok_or_else(|| {
                                        Diagnostic::at(
                                            callee.span,
                                            format!(
                                                "method `{}` did not produce an updated mutable receiver",
                                                field
                                            ),
                                        )
                                    })?;
                                self.write_place_expr(object, env, updated_receiver)?;
                            }
                            self.apply_borrowed_param_writebacks(
                                &method.decl.params,
                                &evaluated_args,
                                &outcome.updated_params,
                                env,
                            )?;
                            return Ok(EvalOutcome::Value(outcome.value));
                        }
                        Err(Diagnostic::at(callee.span, "unsupported call target"))
                    }
                    Value::Float(value) if field == "sqrt" => {
                        BuiltinMember::FloatSqrt.bind_args(args, callee.span)?;
                        Ok(EvalOutcome::Value(Value::Float(value.sqrt())))
                    }
                    Value::String(value) if field == "clone" => {
                        BuiltinMember::StringClone.bind_args(args, callee.span)?;
                        Ok(EvalOutcome::Value(Value::String(value.clone())))
                    }
                    other if field == "sqrt" => {
                        BuiltinMember::FloatSqrt.bind_args(args, callee.span)?;
                        Err(Diagnostic::at(
                            callee.span,
                            format!(
                                "`sqrt` is only available on `float64`, found `{}`",
                                other.render()
                            ),
                        ))
                    }
                    _ => Err(Diagnostic::at(callee.span, "unsupported call target")),
                }
            }
            _ => Err(Diagnostic::at(callee.span, "unsupported call target")),
        }
    }

    fn eval_binary(
        &self,
        span: crate::diag::Span,
        op: BinaryOp,
        left: Value,
        right: Value,
    ) -> Result<Value> {
        match op {
            BinaryOp::And => match (left, right) {
                (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left && right)),
                _ => Err(Diagnostic::at(
                    span,
                    "logical operands must both have type `bool`",
                )),
            },
            BinaryOp::Or => match (left, right) {
                (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left || right)),
                _ => Err(Diagnostic::at(
                    span,
                    "logical operands must both have type `bool`",
                )),
            },
            BinaryOp::Eq => Ok(Value::Bool(left == right)),
            BinaryOp::NotEq => Ok(Value::Bool(left != right)),
            BinaryOp::Add => match (left, right) {
                (Value::Int(left), Value::Int(right)) => left
                    .checked_add(right)
                    .map(Value::Int)
                    .ok_or_else(|| Diagnostic::at(span, "integer overflow")),
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left + right)),
                (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
                _ => Err(Diagnostic::at(
                    span,
                    "binary operands must have matching supported types",
                )),
            },
            BinaryOp::Sub => match (left, right) {
                (Value::Int(left), Value::Int(right)) => left
                    .checked_sub(right)
                    .map(Value::Int)
                    .ok_or_else(|| Diagnostic::at(span, "integer overflow")),
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left - right)),
                _ => Err(Diagnostic::at(
                    span,
                    "binary operands must have matching numeric types",
                )),
            },
            BinaryOp::Mul => match (left, right) {
                (Value::Int(left), Value::Int(right)) => left
                    .checked_mul(right)
                    .map(Value::Int)
                    .ok_or_else(|| Diagnostic::at(span, "integer overflow")),
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left * right)),
                _ => Err(Diagnostic::at(
                    span,
                    "binary operands must have matching numeric types",
                )),
            },
            BinaryOp::Div => match (left, right) {
                (Value::Int(_left), Value::Int(0)) => Err(Diagnostic::at(span, "division by zero")),
                (Value::Int(left), Value::Int(right)) => left
                    .checked_div(right)
                    .map(Value::Int)
                    .ok_or_else(|| Diagnostic::at(span, "integer overflow")),
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left / right)),
                _ => Err(Diagnostic::at(
                    span,
                    "binary operands must have matching numeric types",
                )),
            },
            BinaryOp::Mod => match (left, right) {
                (Value::Int(_left), Value::Int(0)) => Err(Diagnostic::at(span, "division by zero")),
                (Value::Int(left), Value::Int(right)) => left
                    .checked_rem(right)
                    .map(Value::Int)
                    .ok_or_else(|| Diagnostic::at(span, "integer overflow")),
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left % right)),
                _ => Err(Diagnostic::at(
                    span,
                    "binary operands must have matching numeric types",
                )),
            },
            BinaryOp::Less => self.eval_ordering(
                span,
                left,
                right,
                |left, right| left < right,
                |left, right| left < right,
            ),
            BinaryOp::LessEq => self.eval_ordering(
                span,
                left,
                right,
                |left, right| left <= right,
                |left, right| left <= right,
            ),
            BinaryOp::Greater => self.eval_ordering(
                span,
                left,
                right,
                |left, right| left > right,
                |left, right| left > right,
            ),
            BinaryOp::GreaterEq => self.eval_ordering(
                span,
                left,
                right,
                |left, right| left >= right,
                |left, right| left >= right,
            ),
        }
    }

    fn eval_ordering(
        &self,
        span: crate::diag::Span,
        left: Value,
        right: Value,
        compare_int: impl FnOnce(i128, i128) -> bool + Copy,
        compare_float: impl FnOnce(f64, f64) -> bool + Copy,
    ) -> Result<Value> {
        match (left, right) {
            (Value::Int(left), Value::Int(right)) => Ok(Value::Bool(compare_int(left, right))),
            (Value::Float(left), Value::Float(right)) => {
                Ok(Value::Bool(compare_float(left, right)))
            }
            _ => Err(Diagnostic::at(
                span,
                "ordering comparisons require matching numeric operands",
            )),
        }
    }

    fn eval_spawn(
        &mut self,
        detached: bool,
        expr: &Expr,
        env: &mut Env,
        span: crate::diag::Span,
    ) -> Result<EvalOutcome> {
        let ExprKind::Call { callee, args } = &expr.kind else {
            return Err(Diagnostic::at(
                span,
                "`spawn` requires a function or method call expression",
            ));
        };

        let ExprKind::Name(function_name) = &callee.kind else {
            return Err(Diagnostic::at(
                span,
                "`spawn` currently supports named function calls only",
            ));
        };
        let function = self
            .program
            .functions
            .get(function_name)
            .ok_or_else(|| Diagnostic::at(span, "spawn target must be a named function"))?
            .decl
            .clone();

        self.require_spawnable_function(&function, span)?;
        let evaluated_args = self.eval_callable_args(
            &format!("function `{}`", function_name),
            &function.params,
            args,
            env,
            span,
        )?;
        let values = evaluated_args
            .iter()
            .map(|argument| argument.value.clone())
            .collect();

        let program = self.program.clone();
        let stdout = self.stdout.clone();
        let cancellation = if detached {
            CancellationContext::default()
        } else {
            self.cancellation.clone()
        };
        let handle = thread::spawn(move || {
            let mut interpreter = Interpreter {
                program,
                stdout,
                cancellation,
            };
            interpreter
                .call_function(&function, values)
                .map(|outcome| outcome.value)
                .map_err(|error| error.to_string())
        });

        let task = TaskValue {
            inner: Arc::new(TaskState {
                handle: Mutex::new(TaskHandle::Running(Some(handle))),
            }),
        };
        if detached {
            Ok(EvalOutcome::Value(Value::Unit))
        } else {
            Ok(EvalOutcome::Value(Value::Task(task)))
        }
    }

    fn eval_channel_method(
        &mut self,
        channel: ChannelValue,
        field: &str,
        args: &[Argument],
        env: &mut Env,
        span: crate::diag::Span,
    ) -> Result<EvalOutcome> {
        match field {
            "clone" => {
                BuiltinMember::ChannelClone.bind_args(args, span)?;
                Ok(EvalOutcome::Value(Value::Channel(channel)))
            }
            "send" => {
                let ordered_args = BuiltinMember::ChannelSend.bind_args(args, span)?;
                let send_arg = ordered_args[0].expect("`send` requires exactly one argument");
                let value = match self.eval_expr(&send_arg.value, env)? {
                    EvalOutcome::Value(value) => value,
                    EvalOutcome::Return(value) => return Ok(EvalOutcome::Return(value)),
                };
                let mut state = channel.inner.state.lock().unwrap();
                if state.closed {
                    return Ok(EvalOutcome::Value(result_err(send_error_closed(value))));
                }
                state.queue.push_back(value);
                drop(state);
                channel.inner.ready.notify_one();
                Ok(EvalOutcome::Value(result_ok(Value::Unit)))
            }
            "recv" => {
                BuiltinMember::ChannelRecv.bind_args(args, span)?;
                let mut state = channel.inner.state.lock().unwrap();
                loop {
                    if let Some(value) = state.queue.pop_front() {
                        return Ok(EvalOutcome::Value(option_some(value)));
                    }
                    if state.closed {
                        return Ok(EvalOutcome::Value(option_none()));
                    }
                    state = channel.inner.ready.wait(state).unwrap();
                }
            }
            "close" => {
                BuiltinMember::ChannelClose.bind_args(args, span)?;
                let mut state = channel.inner.state.lock().unwrap();
                state.closed = true;
                drop(state);
                channel.inner.ready.notify_all();
                Ok(EvalOutcome::Value(Value::Unit))
            }
            _ => Err(Diagnostic::at(
                span,
                format!("unsupported channel method `{}`", field),
            )),
        }
    }

    fn eval_task_method(
        &mut self,
        task: TaskValue,
        field: &str,
        args: &[Argument],
        span: crate::diag::Span,
    ) -> Result<EvalOutcome> {
        match field {
            "clone" => {
                BuiltinMember::TaskClone.bind_args(args, span)?;
                Ok(EvalOutcome::Value(Value::Task(task)))
            }
            "join" => {
                BuiltinMember::TaskJoin.bind_args(args, span)?;
                Ok(EvalOutcome::Value(self.join_task(task, span)?))
            }
            _ => Err(Diagnostic::at(
                span,
                format!("unsupported task method `{}`", field),
            )),
        }
    }

    fn eval_task_group_method(
        &mut self,
        group: TaskGroupValue,
        field: &str,
        args: &[Argument],
        env: &mut Env,
        span: crate::diag::Span,
    ) -> Result<EvalOutcome> {
        match field {
            "spawn" => {
                if args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "`spawn` expects a target function followed by its arguments",
                    ));
                }
                let ExprKind::Name(function_name) = &args[0].value.kind else {
                    return Err(Diagnostic::at(
                        args[0].span,
                        "`spawn` currently requires a named function target",
                    ));
                };
                let function = self
                    .program
                    .functions
                    .get(function_name)
                    .ok_or_else(|| {
                        Diagnostic::at(
                            args[0].span,
                            format!("unknown function `{}`", function_name),
                        )
                    })?
                    .decl
                    .clone();

                self.require_spawnable_function(&function, span)?;
                let evaluated_args = self.eval_callable_args(
                    &format!("function `{}`", function_name),
                    &function.params,
                    &args[1..],
                    env,
                    span,
                )?;
                let values = evaluated_args
                    .iter()
                    .map(|argument| argument.value.clone())
                    .collect();

                let program = self.program.clone();
                let stdout = self.stdout.clone();
                let cancellation = group.child_cancellation();
                let handle = thread::spawn(move || {
                    let mut interpreter = Interpreter {
                        program,
                        stdout,
                        cancellation,
                    };
                    interpreter
                        .call_function(&function, values)
                        .map(|outcome| outcome.value)
                        .map_err(|error| error.to_string())
                });
                let task = TaskValue {
                    inner: Arc::new(TaskState {
                        handle: Mutex::new(TaskHandle::Running(Some(handle))),
                    }),
                };
                group.register_task(task.clone());
                Ok(EvalOutcome::Value(Value::Task(task)))
            }
            "cancel" => {
                BuiltinMember::TaskGroupCancel.bind_args(args, span)?;
                group.cancel();
                Ok(EvalOutcome::Value(Value::Unit))
            }
            _ => Err(Diagnostic::at(
                span,
                format!("unsupported task group method `{}`", field),
            )),
        }
    }

    fn join_task(&mut self, task: TaskValue, span: crate::diag::Span) -> Result<Value> {
        let handle = {
            let mut state = task.inner.handle.lock().unwrap();
            match &mut *state {
                TaskHandle::Completed(result) => {
                    return result
                        .clone()
                        .map_err(|message| Diagnostic::at(span, message));
                }
                TaskHandle::Running(handle) => handle.take(),
            }
        };

        let Some(handle) = handle else {
            return Err(Diagnostic::at(span, "task join handle was not available"));
        };

        let result = handle
            .join()
            .map_err(|_| Diagnostic::at(span, "spawned task panicked"))?;
        let mut state = task.inner.handle.lock().unwrap();
        *state = TaskHandle::Completed(result.clone());
        result.map_err(|message| Diagnostic::at(span, message))
    }

    fn read_assign_target(
        &mut self,
        target: &AssignTarget,
        env: &mut Env,
        span: crate::diag::Span,
    ) -> Result<Value> {
        match target {
            AssignTarget::Name(name) => env.get(name).cloned().ok_or_else(|| {
                Diagnostic::at(
                    span,
                    format!("unknown name `{}` in compound assignment", name),
                )
            }),
            AssignTarget::Member { object, field } => {
                let value = self.read_place_expr(object, env)?;
                let Value::Instance(instance) = value else {
                    return Err(Diagnostic::at(
                        span,
                        format!("cannot assign member `{}` on non-instance value", field),
                    ));
                };
                instance.fields.get(field).cloned().ok_or_else(|| {
                    Diagnostic::at(
                        span,
                        format!("class `{}` has no field `{}`", instance.class_name, field),
                    )
                })
            }
        }
    }

    fn write_assign_target(
        &mut self,
        target: &AssignTarget,
        env: &mut Env,
        value: Value,
        span: crate::diag::Span,
    ) -> Result<()> {
        match target {
            AssignTarget::Name(name) => {
                if let Some(binding_ty) = env.get_type(name).cloned() {
                    self.validate_value_fits_type(&value, &binding_ty, span)?;
                }
                env.set(name.clone(), value);
                Ok(())
            }
            AssignTarget::Member { object, field } => {
                let mut object_value = self.read_place_expr(object, env)?;
                let Value::Instance(instance) = &mut object_value else {
                    return Err(Diagnostic::at(
                        span,
                        format!("cannot assign member `{}` on non-instance value", field),
                    ));
                };
                if !instance.fields.contains_key(field) {
                    return Err(Diagnostic::at(
                        span,
                        format!("class `{}` has no field `{}`", instance.class_name, field),
                    ));
                }
                if let Some(field_ty) = self
                    .program
                    .classes
                    .get(&instance.class_name)
                    .and_then(|class_info| class_info.fields.get(field))
                    .map(|field_info| field_info.ty.clone())
                {
                    self.validate_value_fits_type(&value, &field_ty, span)?;
                }
                instance.fields.insert(field.clone(), value);
                self.write_place_expr(object, env, object_value)
            }
        }
    }

    fn read_place_expr(&mut self, expr: &Expr, env: &mut Env) -> Result<Value> {
        match &expr.kind {
            ExprKind::Name(name) => env
                .get(name)
                .cloned()
                .ok_or_else(|| Diagnostic::at(expr.span, format!("unknown name `{}`", name))),
            ExprKind::Group(inner) => self.read_place_expr(inner, env),
            ExprKind::Member { object, field } => {
                let object_value = self.read_place_expr(object, env)?;
                let Value::Instance(instance) = object_value else {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!("cannot access field `{}` on non-instance value", field),
                    ));
                };
                instance.fields.get(field).cloned().ok_or_else(|| {
                    Diagnostic::at(
                        expr.span,
                        format!("class `{}` has no field `{}`", instance.class_name, field),
                    )
                })
            }
            _ => Err(Diagnostic::at(
                expr.span,
                "expression is not a mutable place",
            )),
        }
    }

    fn write_place_expr(&mut self, expr: &Expr, env: &mut Env, value: Value) -> Result<()> {
        match &expr.kind {
            ExprKind::Name(name) => {
                env.set(name.clone(), value);
                Ok(())
            }
            ExprKind::Group(inner) => self.write_place_expr(inner, env, value),
            ExprKind::Member { object, field } => {
                let mut object_value = self.read_place_expr(object, env)?;
                let Value::Instance(instance) = &mut object_value else {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!("cannot assign field `{}` on non-instance value", field),
                    ));
                };
                if !instance.fields.contains_key(field) {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!("class `{}` has no field `{}`", instance.class_name, field),
                    ));
                }
                instance.fields.insert(field.clone(), value);
                self.write_place_expr(object, env, object_value)
            }
            _ => Err(Diagnostic::at(
                expr.span,
                "expression is not a mutable place",
            )),
        }
    }

    fn run_with_cleanup(
        &mut self,
        binding: &str,
        env: &mut Env,
        span: crate::diag::Span,
        cancel_before_cleanup: bool,
    ) -> Result<()> {
        let resource = env.get(binding).cloned().ok_or_else(|| {
            Diagnostic::at(
                span,
                format!("with binding `{}` was not available for cleanup", binding),
            )
        })?;
        if let Value::TaskGroup(group) = resource {
            return self.close_task_group(group, cancel_before_cleanup, span);
        }
        let Value::Instance(instance) = resource else {
            return Err(Diagnostic::at(
                span,
                format!("with binding `{}` is not a resource instance", binding),
            ));
        };
        let method = self
            .program
            .classes
            .get(&instance.class_name)
            .and_then(|class_info| class_info.methods.get("close"))
            .cloned()
            .ok_or_else(|| {
                Diagnostic::at(
                    span,
                    format!(
                        "class `{}` cannot be used with `with` because it has no `close` method",
                        instance.class_name
                    ),
                )
            })?;
        let outcome = self.call_function(&method.decl, vec![Value::Instance(instance)])?;
        if let Some(updated_receiver) = outcome.updated_receiver {
            env.set(binding.to_string(), updated_receiver);
        }
        Ok(())
    }

    fn close_task_group(
        &mut self,
        group: TaskGroupValue,
        cancel_before_cleanup: bool,
        span: crate::diag::Span,
    ) -> Result<()> {
        if cancel_before_cleanup {
            group.cancel();
        }

        let mut first_error = None;
        for task in group.drain_tasks() {
            if let Err(error) = self.join_task(task, span) {
                group.cancel();
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        Ok(())
    }

    fn eval_callable_args<'a>(
        &mut self,
        callee_name: &str,
        params: &[Param],
        args: &'a [Argument],
        env: &mut Env,
        span: crate::diag::Span,
    ) -> Result<Vec<EvaluatedArg<'a>>> {
        let ordered_args = bind_call_arguments(
            callee_name,
            &callable_params_from_decl(params),
            args,
            span,
            CallConvention::PositionalOrNamed,
        )?;
        let mut values = Vec::with_capacity(ordered_args.len());
        for (argument, param) in ordered_args.into_iter().zip(params.iter()) {
            let expr = if let Some(argument) = argument {
                &argument.value
            } else {
                param
                    .default
                    .as_ref()
                    .expect("optional parameter should provide a default expression")
            };
            values.push(match self.eval_expr(expr, env)? {
                EvalOutcome::Value(value) => EvaluatedArg { argument, value },
                EvalOutcome::Return(_) => {
                    unreachable!("return flow should not escape expression argument evaluation")
                }
            });
        }
        Ok(values)
    }

    fn apply_borrowed_param_writebacks(
        &mut self,
        params: &[Param],
        evaluated_args: &[EvaluatedArg<'_>],
        updated_params: &[(usize, Value)],
        env: &mut Env,
    ) -> Result<()> {
        for (index, value) in updated_params {
            let Some(param) = params.get(*index) else {
                continue;
            };
            if param.passing != ReceiverKind::BorrowMut {
                continue;
            }
            let argument = evaluated_args
                .get(*index)
                .and_then(|evaluated| evaluated.argument)
                .ok_or_else(|| {
                    Diagnostic::at(
                        param.span,
                        format!(
                            "mutable borrowed parameter `{}` requires an explicit argument",
                            param.name
                        ),
                    )
                })?;
            self.write_place_expr(&argument.value, env, value.clone())?;
        }
        Ok(())
    }

    fn require_spawnable_function(
        &self,
        function: &FunctionDecl,
        span: crate::diag::Span,
    ) -> Result<()> {
        if let Some(param) = function
            .params
            .iter()
            .find(|param| param.passing != ReceiverKind::Value)
        {
            return Err(Diagnostic::at(
                span,
                format!(
                    "`spawn` does not yet support borrowed parameter `{}` on function `{}`",
                    param.name, function.name
                ),
            ));
        }
        Ok(())
    }
}
