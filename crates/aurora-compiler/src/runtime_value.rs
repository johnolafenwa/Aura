use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::fs::{self, File as StdFile, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::net::{
    Shutdown, TcpListener as StdTcpListener, TcpStream as StdTcpStream, ToSocketAddrs,
    UdpSocket as StdUdpSocket,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Once, OnceLock};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use corosensei::stack::DefaultStack;
use corosensei::{Coroutine, CoroutineResult, Yielder};
use httparse::{
    Request as HttpParseRequest, Response as HttpParseResponse, Status as HttpParseStatus,
};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection};
use rustls_pemfile::{certs, private_key};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{accept as websocket_accept, client_tls_with_config, Message, WebSocket};
use url::Url;

#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};

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
    File(FileValue),
    TcpListener(TcpListenerValue),
    TcpStream(TcpStreamValue),
    UdpSocket(UdpSocketValue),
    UdpDatagram(UdpDatagramValue),
    HttpListener(HttpListenerValue),
    HttpExchange(HttpExchangeValue),
    HttpResponse(HttpResponseValue),
    WebSocketListener(WebSocketListenerValue),
    WebSocket(WebSocketValue),
    UnixListener(UnixListenerValue),
    UnixStream(UnixStreamValue),
    TlsListener(TlsListenerValue),
    TlsStream(TlsStreamValue),
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

#[derive(Clone)]
pub struct FileValue {
    inner: Arc<FileState>,
}

#[derive(Clone)]
pub struct TcpListenerValue {
    inner: Arc<TcpListenerState>,
}

#[derive(Clone)]
pub struct TcpStreamValue {
    inner: Arc<TcpStreamState>,
}

#[derive(Clone)]
pub struct UdpSocketValue {
    inner: Arc<UdpSocketState>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UdpDatagramValue {
    pub address: String,
    pub data: Vec<u8>,
}

#[derive(Clone)]
pub struct HttpListenerValue {
    inner: Arc<HttpListenerState>,
}

#[derive(Clone)]
pub struct HttpExchangeValue {
    inner: Arc<HttpExchangeState>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HttpResponseValue {
    status: i32,
    reason: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Clone)]
pub struct WebSocketListenerValue {
    inner: Arc<WebSocketListenerState>,
}

#[derive(Clone)]
pub struct WebSocketValue {
    inner: Arc<WebSocketState>,
}

#[derive(Clone)]
pub struct UnixListenerValue {
    inner: Arc<UnixListenerState>,
}

#[derive(Clone)]
pub struct UnixStreamValue {
    inner: Arc<UnixStreamState>,
}

#[derive(Clone)]
pub struct TlsListenerValue {
    inner: Arc<TlsListenerState>,
}

#[derive(Clone)]
pub struct TlsStreamValue {
    inner: Arc<TlsStreamState>,
}

struct TaskState {
    handle: Mutex<TaskHandle>,
    ready: Condvar,
    lightweight: bool,
}

struct TaskGroupState {
    tasks: Mutex<Vec<TaskValue>>,
    cancel_flag: Arc<AtomicBool>,
    parent_flags: Vec<Arc<AtomicBool>>,
}

struct FileState {
    file: Mutex<Option<StdFile>>,
}

struct TcpListenerState {
    listener: Mutex<Option<StdTcpListener>>,
}

struct TcpStreamState {
    stream: Mutex<Option<StdTcpStream>>,
}

struct UdpSocketState {
    socket: Mutex<Option<StdUdpSocket>>,
}

struct HttpListenerState {
    listener: Mutex<Option<StdTcpListener>>,
}

struct HttpExchangeState {
    stream: Mutex<Option<TcpStreamValue>>,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct WebSocketListenerState {
    listener: Mutex<Option<StdTcpListener>>,
}

enum WebSocketStateKind {
    Plain(WebSocket<StdTcpStream>),
    MaybeTls(WebSocket<MaybeTlsStream<StdTcpStream>>),
}

struct WebSocketState {
    socket: Mutex<Option<WebSocketStateKind>>,
}

#[cfg(unix)]
struct UnixListenerState {
    listener: Mutex<Option<StdUnixListener>>,
}

#[cfg(not(unix))]
struct UnixListenerState;

#[cfg(unix)]
struct UnixStreamState {
    stream: Mutex<Option<StdUnixStream>>,
}

#[cfg(not(unix))]
struct UnixStreamState;

struct TlsListenerState {
    listener: Mutex<Option<StdTcpListener>>,
    config: Arc<ServerConfig>,
}

enum TlsStreamKind {
    Client(rustls::StreamOwned<ClientConnection, StdTcpStream>),
    Server(rustls::StreamOwned<ServerConnection, StdTcpStream>),
}

struct TlsStreamState {
    stream: Mutex<Option<TlsStreamKind>>,
}

type TaskResult = std::result::Result<Value, Diagnostic>;

enum TaskHandle {
    Running { waiters: Vec<u64> },
    Completed(TaskResult),
}

#[derive(Clone, Default)]
pub(crate) struct CancellationContext {
    flags: Vec<Arc<AtomicBool>>,
}

enum TaskYield {
    Wait(TaskWaitRegistration),
    Park,
}

#[derive(Clone)]
struct TaskWaitRegistration {
    channels: Vec<ChannelValue>,
    ignore_closed_channels: bool,
    deadline: Option<Instant>,
    cancellation: Option<CancellationContext>,
    fd_wait: Option<FdWaitRegistration>,
}

#[derive(Clone, Copy)]
struct FdWaitRegistration {
    fd: libc::c_int,
    events: libc::c_short,
}

struct LightweightTaskContext {
    scheduler: *mut LightweightTaskScheduler,
    task_id: u64,
    yielder: Cell<*const Yielder<RuntimeSchedulerWakeReason, TaskYield>>,
    cancellation: Option<CancellationContext>,
}

struct LightweightTaskRecord {
    state: Arc<TaskState>,
    context: Box<LightweightTaskContext>,
    coroutine: Coroutine<RuntimeSchedulerWakeReason, TaskYield, TaskResult>,
}

struct LightweightTaskScheduler {
    next_task_id: u64,
    ready: VecDeque<(u64, RuntimeSchedulerWakeReason)>,
    waiting: BTreeMap<u64, TaskWaitRegistration>,
    tasks: BTreeMap<u64, LightweightTaskRecord>,
}

const LIGHTWEIGHT_TASK_STACK_SIZE: usize = 64 * 1024 * 1024;

#[derive(Clone)]
struct RuntimeSchedulerRegistration {
    channels: Vec<ChannelValue>,
    ignore_closed_channels: bool,
    deadline: Option<Instant>,
    cancellation: Option<CancellationContext>,
    waiter: Arc<RuntimeSchedulerWaiter>,
}

struct RuntimeSchedulerWaiter {
    state: Mutex<Option<RuntimeSchedulerWakeReason>>,
    ready: Condvar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeSchedulerWakeReason {
    Ready,
    TimedOut,
    Cancelled,
}

struct RuntimeSchedulerState {
    registrations: BTreeMap<u64, RuntimeSchedulerRegistration>,
}

struct RuntimeScheduler {
    state: Mutex<RuntimeSchedulerState>,
    ready: Condvar,
    next_id: AtomicU64,
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

impl fmt::Debug for FileValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FileValue(..)")
    }
}

impl fmt::Debug for TcpListenerValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TcpListenerValue(..)")
    }
}

impl fmt::Debug for TcpStreamValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TcpStreamValue(..)")
    }
}

impl fmt::Debug for UdpSocketValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UdpSocketValue(..)")
    }
}

impl fmt::Debug for HttpListenerValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HttpListenerValue(..)")
    }
}

impl fmt::Debug for HttpExchangeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HttpExchangeValue(..)")
    }
}

impl fmt::Debug for WebSocketListenerValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WebSocketListenerValue(..)")
    }
}

impl fmt::Debug for WebSocketValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WebSocketValue(..)")
    }
}

impl fmt::Debug for UnixListenerValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UnixListenerValue(..)")
    }
}

impl fmt::Debug for UnixStreamValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UnixStreamValue(..)")
    }
}

impl fmt::Debug for TlsListenerValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TlsListenerValue(..)")
    }
}

impl fmt::Debug for TlsStreamValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TlsStreamValue(..)")
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

impl PartialEq for FileValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for TcpListenerValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for TcpStreamValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for UdpSocketValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for HttpListenerValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for HttpExchangeValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for WebSocketListenerValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for WebSocketValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for UnixListenerValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for UnixStreamValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for TlsListenerValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for TlsStreamValue {
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

fn wait_timeout_condvar<'a, T>(
    condvar: &Condvar,
    guard: MutexGuard<'a, T>,
    timeout: StdDuration,
) -> (MutexGuard<'a, T>, bool) {
    let (guard, timeout_result) = condvar
        .wait_timeout(guard, timeout)
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (guard, timeout_result.timed_out())
}

impl RuntimeSchedulerWaiter {
    fn finish(&self, reason: RuntimeSchedulerWakeReason) {
        let mut state = lock_mutex(&self.state);
        if state.is_none() {
            *state = Some(reason);
            self.ready.notify_all();
        }
    }

    fn wait(&self) -> RuntimeSchedulerWakeReason {
        let mut state = lock_mutex(&self.state);
        loop {
            if let Some(reason) = *state {
                return reason;
            }
            state = wait_condvar(&self.ready, state);
        }
    }
}

struct RuntimeSchedulerHandle {
    id: u64,
    waiter: Arc<RuntimeSchedulerWaiter>,
    scheduler: Arc<RuntimeScheduler>,
}

impl RuntimeSchedulerHandle {
    fn wait(&self) -> RuntimeSchedulerWakeReason {
        self.waiter.wait()
    }
}

impl Drop for RuntimeSchedulerHandle {
    fn drop(&mut self) {
        self.scheduler.unregister(self.id);
    }
}

impl RuntimeScheduler {
    fn start() -> Arc<Self> {
        let scheduler = Arc::new(Self {
            state: Mutex::new(RuntimeSchedulerState {
                registrations: BTreeMap::new(),
            }),
            ready: Condvar::new(),
            next_id: AtomicU64::new(1),
        });
        let worker = scheduler.clone();
        std::thread::spawn(move || worker.run());
        scheduler
    }

    fn register(
        &self,
        channels: Vec<ChannelValue>,
        ignore_closed_channels: bool,
        deadline: Option<Instant>,
        cancellation: Option<CancellationContext>,
    ) -> RuntimeSchedulerHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let waiter = Arc::new(RuntimeSchedulerWaiter {
            state: Mutex::new(None),
            ready: Condvar::new(),
        });
        let registration = RuntimeSchedulerRegistration {
            channels,
            ignore_closed_channels,
            deadline,
            cancellation,
            waiter: waiter.clone(),
        };
        lock_mutex(&self.state)
            .registrations
            .insert(id, registration);
        self.ready.notify_all();
        RuntimeSchedulerHandle {
            id,
            waiter,
            scheduler: runtime_scheduler().clone(),
        }
    }

    fn unregister(&self, id: u64) {
        let mut state = lock_mutex(&self.state);
        if state.registrations.remove(&id).is_some() {
            self.ready.notify_all();
        }
    }

    fn notify(&self) {
        self.ready.notify_all();
    }

    fn run(self: Arc<Self>) {
        loop {
            let mut state = lock_mutex(&self.state);
            while state.registrations.is_empty() {
                state = wait_condvar(&self.ready, state);
            }

            let mut finished = Vec::new();
            let mut next_deadline = None;
            let now = Instant::now();
            for (id, registration) in &state.registrations {
                if registration
                    .cancellation
                    .as_ref()
                    .is_some_and(CancellationContext::is_cancelled)
                {
                    registration
                        .waiter
                        .finish(RuntimeSchedulerWakeReason::Cancelled);
                    finished.push(*id);
                    continue;
                }

                if registration.channels.iter().any(|channel| {
                    channel.is_ready_for_scheduler_recv(registration.ignore_closed_channels)
                }) {
                    registration
                        .waiter
                        .finish(RuntimeSchedulerWakeReason::Ready);
                    finished.push(*id);
                    continue;
                }

                if let Some(deadline) = registration.deadline {
                    if now >= deadline {
                        registration
                            .waiter
                            .finish(RuntimeSchedulerWakeReason::TimedOut);
                        finished.push(*id);
                        continue;
                    }
                    next_deadline = Some(match next_deadline {
                        Some(current) => std::cmp::min(current, deadline),
                        None => deadline,
                    });
                }
            }

            for id in finished {
                state.registrations.remove(&id);
            }

            if state.registrations.is_empty() {
                continue;
            }

            state = if let Some(deadline) = next_deadline {
                let now = Instant::now();
                let timeout = deadline.saturating_duration_since(now);
                wait_timeout_condvar(&self.ready, state, timeout).0
            } else {
                wait_condvar(&self.ready, state)
            };
            drop(state);
        }
    }
}

fn runtime_scheduler() -> &'static Arc<RuntimeScheduler> {
    static SCHEDULER: OnceLock<Arc<RuntimeScheduler>> = OnceLock::new();
    SCHEDULER.get_or_init(RuntimeScheduler::start)
}

fn wait_for_runtime_scheduler(
    channels: Vec<ChannelValue>,
    ignore_closed_channels: bool,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> RuntimeSchedulerWakeReason {
    if cancellation.is_some_and(CancellationContext::is_cancelled) {
        return RuntimeSchedulerWakeReason::Cancelled;
    }
    if channels
        .iter()
        .any(|channel| channel.is_ready_for_scheduler_recv(ignore_closed_channels))
    {
        return RuntimeSchedulerWakeReason::Ready;
    }
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return RuntimeSchedulerWakeReason::TimedOut;
    }

    if let Some(reason) = yield_current_lightweight_wait(TaskWaitRegistration {
        channels: channels.clone(),
        ignore_closed_channels,
        deadline,
        cancellation: cancellation.cloned(),
        fd_wait: None,
    }) {
        return reason;
    }

    runtime_scheduler()
        .register(
            channels,
            ignore_closed_channels,
            deadline,
            cancellation.cloned(),
        )
        .wait()
}

pub(crate) fn wait_for_select_progress(
    channels: &[ChannelValue],
    ignore_closed_channels: bool,
    deadlines: &[Instant],
    cancellation: Option<&CancellationContext>,
) -> RuntimeSchedulerWakeReason {
    let deadline = deadlines.iter().copied().min();
    wait_for_runtime_scheduler(
        channels.to_vec(),
        ignore_closed_channels,
        deadline,
        cancellation,
    )
}

pub(crate) fn sleep_with_runtime_scheduler(
    duration: StdDuration,
    cancellation: Option<&CancellationContext>,
) {
    let deadline = Instant::now().checked_add(duration);
    let _ = wait_for_runtime_scheduler(Vec::new(), false, deadline, cancellation);
}

thread_local! {
    static CURRENT_LIGHTWEIGHT_TASK_CONTEXT: Cell<*const LightweightTaskContext> =
        const { Cell::new(std::ptr::null()) };
    static CURRENT_LIGHTWEIGHT_TASK_CANCELLATION: std::cell::RefCell<Option<CancellationContext>> =
        const { std::cell::RefCell::new(None) };
}

struct LightweightTaskContextGuard {
    previous: *const LightweightTaskContext,
    previous_cancellation: Option<CancellationContext>,
}

impl Drop for LightweightTaskContextGuard {
    fn drop(&mut self) {
        CURRENT_LIGHTWEIGHT_TASK_CONTEXT.with(|slot| slot.set(self.previous));
        CURRENT_LIGHTWEIGHT_TASK_CANCELLATION
            .with(|slot| *slot.borrow_mut() = self.previous_cancellation.take());
    }
}

fn enter_lightweight_task_context(context: &LightweightTaskContext) -> LightweightTaskContextGuard {
    let previous = CURRENT_LIGHTWEIGHT_TASK_CONTEXT.with(|slot| {
        let previous = slot.get();
        slot.set(context as *const _);
        previous
    });
    let previous_cancellation = CURRENT_LIGHTWEIGHT_TASK_CANCELLATION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let previous = slot.clone();
        *slot = context.cancellation.clone();
        previous
    });
    LightweightTaskContextGuard {
        previous,
        previous_cancellation,
    }
}

fn current_lightweight_task_context() -> Option<&'static LightweightTaskContext> {
    CURRENT_LIGHTWEIGHT_TASK_CONTEXT.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &*ptr })
        }
    })
}

pub(crate) fn current_lightweight_task_cancellation() -> Option<CancellationContext> {
    CURRENT_LIGHTWEIGHT_TASK_CANCELLATION.with(|slot| slot.borrow().clone())
}

fn yield_current_lightweight_task(wait: TaskYield) -> Option<RuntimeSchedulerWakeReason> {
    let context = current_lightweight_task_context()?;
    let yielder_ptr = context.yielder.get();
    if yielder_ptr.is_null() {
        return None;
    }
    let yielder = unsafe { &*yielder_ptr };
    Some(yielder.suspend(wait))
}

fn yield_current_lightweight_wait(
    wait: TaskWaitRegistration,
) -> Option<RuntimeSchedulerWakeReason> {
    yield_current_lightweight_task(TaskYield::Wait(wait))
}

fn park_current_lightweight_task() -> Option<RuntimeSchedulerWakeReason> {
    yield_current_lightweight_task(TaskYield::Park)
}

impl TaskWaitRegistration {
    fn ready_reason(&self, fd_ready: bool) -> Option<RuntimeSchedulerWakeReason> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(CancellationContext::is_cancelled)
        {
            return Some(RuntimeSchedulerWakeReason::Cancelled);
        }
        if self
            .channels
            .iter()
            .any(|channel| channel.is_ready_for_scheduler_recv(self.ignore_closed_channels))
        {
            return Some(RuntimeSchedulerWakeReason::Ready);
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Some(RuntimeSchedulerWakeReason::TimedOut);
        }
        if fd_ready {
            return Some(RuntimeSchedulerWakeReason::Ready);
        }
        None
    }
}

impl LightweightTaskScheduler {
    fn new() -> Self {
        Self {
            next_task_id: 1,
            ready: VecDeque::new(),
            waiting: BTreeMap::new(),
            tasks: BTreeMap::new(),
        }
    }

    fn spawn_task<F>(
        &mut self,
        cancellation: Option<CancellationContext>,
        entry: F,
    ) -> std::result::Result<TaskValue, Diagnostic>
    where
        F: FnOnce() -> TaskResult + 'static,
    {
        let task_id = self.next_task_id;
        self.next_task_id += 1;

        let state = Arc::new(TaskState {
            handle: Mutex::new(TaskHandle::Running {
                waiters: Vec::new(),
            }),
            ready: Condvar::new(),
            lightweight: true,
        });
        let context = Box::new(LightweightTaskContext {
            scheduler: self as *mut _,
            task_id,
            yielder: Cell::new(std::ptr::null()),
            cancellation,
        });
        let context_ptr = &*context as *const LightweightTaskContext;
        let stack = DefaultStack::new(LIGHTWEIGHT_TASK_STACK_SIZE).map_err(|error| {
            Diagnostic::new(format!("failed to allocate Aurora task stack: {error}"))
        })?;
        let coroutine = Coroutine::with_stack(stack, move |yielder, _| {
            let context = unsafe { &*context_ptr };
            context.yielder.set(yielder as *const _);
            entry()
        });

        self.tasks.insert(
            task_id,
            LightweightTaskRecord {
                state: state.clone(),
                context,
                coroutine,
            },
        );
        self.ready
            .push_back((task_id, RuntimeSchedulerWakeReason::Ready));
        Ok(TaskValue { inner: state })
    }

    fn complete_task(&mut self, task_id: u64, task_state: &Arc<TaskState>, result: TaskResult) {
        self.waiting.remove(&task_id);
        let waiters = {
            let mut state = lock_mutex(&task_state.handle);
            let waiters = match &mut *state {
                TaskHandle::Running { waiters } => std::mem::take(waiters),
                TaskHandle::Completed(_) => Vec::new(),
            };
            *state = TaskHandle::Completed(result.clone());
            task_state.ready.notify_all();
            waiters
        };
        for waiter in waiters {
            self.waiting.remove(&waiter);
            self.ready
                .push_back((waiter, RuntimeSchedulerWakeReason::Ready));
        }
    }

    fn resume_task(&mut self, task_id: u64, reason: RuntimeSchedulerWakeReason) {
        let Some(mut record) = self.tasks.remove(&task_id) else {
            return;
        };
        self.waiting.remove(&task_id);
        let _guard = enter_lightweight_task_context(&record.context);
        match record.coroutine.resume(reason) {
            CoroutineResult::Yield(TaskYield::Wait(wait)) => {
                self.waiting.insert(task_id, wait);
                self.tasks.insert(task_id, record);
            }
            CoroutineResult::Yield(TaskYield::Park) => {
                self.tasks.insert(task_id, record);
            }
            CoroutineResult::Return(result) => {
                self.complete_task(task_id, &record.state, result);
            }
        }
    }

    fn promote_ready_waiters(&mut self, fd_ready: Option<&BTreeMap<u64, bool>>) {
        let mut ready = Vec::new();
        for (task_id, wait) in &self.waiting {
            let fd_ready = fd_ready
                .and_then(|ready_map| ready_map.get(task_id))
                .copied()
                .unwrap_or(false);
            if let Some(reason) = wait.ready_reason(fd_ready) {
                ready.push((*task_id, reason));
            }
        }
        for (task_id, reason) in ready {
            self.waiting.remove(&task_id);
            self.ready.push_back((task_id, reason));
        }
    }

    fn wait_for_external_events(&mut self) {
        self.promote_ready_waiters(None);
        if !self.ready.is_empty() {
            return;
        }

        let next_deadline = self.waiting.values().filter_map(|wait| wait.deadline).min();
        let mut task_ids = Vec::new();
        let mut descriptors = Vec::new();
        for (task_id, wait) in &self.waiting {
            if let Some(fd_wait) = wait.fd_wait {
                task_ids.push(*task_id);
                descriptors.push(libc::pollfd {
                    fd: fd_wait.fd,
                    events: fd_wait.events,
                    revents: 0,
                });
            }
        }

        if descriptors.is_empty() {
            if let Some(deadline) = next_deadline {
                let now = Instant::now();
                if deadline > now {
                    thread::sleep(deadline.saturating_duration_since(now));
                }
            } else {
                // Only irrecoverable deadlocks end up here (for example, every task parked on
                // queue waits with no remaining runnable producer). Avoid a hot spin while
                // preserving the historical "wait forever" behavior for such programs.
                thread::park_timeout(StdDuration::from_millis(1));
            }
            self.promote_ready_waiters(None);
            return;
        }

        let timeout_ms = match next_deadline {
            Some(deadline) => {
                let now = Instant::now();
                if deadline <= now {
                    0
                } else {
                    duration_to_poll_timeout(deadline.saturating_duration_since(now))
                }
            }
            None => -1,
        };

        let result =
            unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, timeout_ms) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                thread::park_timeout(StdDuration::from_millis(1));
            }
            self.promote_ready_waiters(None);
            return;
        }

        let mut fd_ready = BTreeMap::new();
        for (task_id, descriptor) in task_ids.into_iter().zip(descriptors.into_iter()) {
            if descriptor.revents != 0 {
                fd_ready.insert(task_id, true);
            }
        }
        self.promote_ready_waiters(Some(&fd_ready));
    }

    fn run_until_root(&mut self, root: &TaskValue) -> TaskResult {
        loop {
            if let Some(result) = root.completed_result() {
                return result;
            }

            if let Some((task_id, reason)) = self.ready.pop_front() {
                self.resume_task(task_id, reason);
                continue;
            }

            self.wait_for_external_events();
        }
    }
}

pub(crate) fn spawn_lightweight_task<F>(entry: F) -> std::result::Result<TaskValue, Diagnostic>
where
    F: FnOnce() -> TaskResult + 'static,
{
    let Some(context) = current_lightweight_task_context() else {
        return Err(Diagnostic::new(
            "lightweight Aurora task spawn requires an active task scheduler",
        ));
    };
    let scheduler = unsafe { &mut *context.scheduler };
    scheduler.spawn_task(None, entry)
}

pub(crate) fn spawn_lightweight_task_with_cancellation<F>(
    cancellation: CancellationContext,
    entry: F,
) -> std::result::Result<TaskValue, Diagnostic>
where
    F: FnOnce() -> TaskResult + 'static,
{
    let Some(context) = current_lightweight_task_context() else {
        return Err(Diagnostic::new(
            "lightweight Aurora task spawn requires an active task scheduler",
        ));
    };
    let scheduler = unsafe { &mut *context.scheduler };
    scheduler.spawn_task(Some(cancellation), entry)
}

pub(crate) fn run_lightweight_root_task<F>(entry: F) -> TaskResult
where
    F: FnOnce() -> TaskResult + 'static,
{
    let mut scheduler = Box::new(LightweightTaskScheduler::new());
    let root = scheduler.spawn_task(None, entry)?;
    scheduler.run_until_root(&root)
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
            (Value::File(left), Value::File(right)) => left == right,
            (Value::TcpListener(left), Value::TcpListener(right)) => left == right,
            (Value::TcpStream(left), Value::TcpStream(right)) => left == right,
            (Value::UdpSocket(left), Value::UdpSocket(right)) => left == right,
            (Value::UdpDatagram(left), Value::UdpDatagram(right)) => left == right,
            (Value::HttpListener(left), Value::HttpListener(right)) => left == right,
            (Value::HttpExchange(left), Value::HttpExchange(right)) => left == right,
            (Value::HttpResponse(left), Value::HttpResponse(right)) => left == right,
            (Value::WebSocketListener(left), Value::WebSocketListener(right)) => left == right,
            (Value::WebSocket(left), Value::WebSocket(right)) => left == right,
            (Value::UnixListener(left), Value::UnixListener(right)) => left == right,
            (Value::UnixStream(left), Value::UnixStream(right)) => left == right,
            (Value::TlsListener(left), Value::TlsListener(right)) => left == right,
            (Value::TlsStream(left), Value::TlsStream(right)) => left == right,
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
            Value::Channel(_) => "<queue>".to_string(),
            Value::Task(_) => "<task>".to_string(),
            Value::TaskGroup(_) => "<tasks>".to_string(),
            Value::File(_) => "<file>".to_string(),
            Value::TcpListener(_) => "<tcp-listener>".to_string(),
            Value::TcpStream(_) => "<tcp-stream>".to_string(),
            Value::UdpSocket(_) => "<udp-socket>".to_string(),
            Value::UdpDatagram(datagram) => format!(
                "<udp-datagram {} {} bytes>",
                datagram.address,
                datagram.data.len()
            ),
            Value::HttpListener(_) => "<http-listener>".to_string(),
            Value::HttpExchange(_) => "<http-exchange>".to_string(),
            Value::HttpResponse(response) => format!(
                "<http-response {} {} bytes>",
                response.status,
                response.body.len()
            ),
            Value::WebSocketListener(_) => "<websocket-listener>".to_string(),
            Value::WebSocket(_) => "<websocket>".to_string(),
            Value::UnixListener(_) => "<unix-listener>".to_string(),
            Value::UnixStream(_) => "<unix-stream>".to_string(),
            Value::TlsListener(_) => "<tls-listener>".to_string(),
            Value::TlsStream(_) => "<tls-stream>".to_string(),
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
    fn is_ready_for_scheduler_recv(&self, ignore_closed: bool) -> bool {
        let state = lock_mutex(&self.inner.state);
        !state.queue.is_empty() || (!ignore_closed && state.closed)
    }

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
        runtime_scheduler().notify();
        Ok(())
    }

    pub(crate) fn recv_with_cancellation(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> Option<Value> {
        let deadline = deadline_from_timeout(timeout);
        loop {
            match self.try_recv() {
                TryRecvResult::Value(value) => return Some(value),
                TryRecvResult::Closed => return None,
                TryRecvResult::Empty => {}
            }

            match wait_for_runtime_scheduler(vec![self.clone()], false, deadline, cancellation) {
                RuntimeSchedulerWakeReason::Ready => {}
                RuntimeSchedulerWakeReason::TimedOut | RuntimeSchedulerWakeReason::Cancelled => {
                    return None;
                }
            }
        }
    }

    pub(crate) fn close(&self) {
        let mut state = lock_mutex(&self.inner.state);
        state.closed = true;
        drop(state);
        runtime_scheduler().notify();
    }
}

fn closed_resource_error() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "resource is closed")
}

fn cancelled_resource_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "operation cancelled")
}

fn timeout_resource_error() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "operation timed out")
}

fn deadline_from_timeout(timeout: Option<StdDuration>) -> Option<Instant> {
    timeout.and_then(|duration| Instant::now().checked_add(duration))
}

fn duration_to_poll_timeout(duration: StdDuration) -> libc::c_int {
    if duration.is_zero() {
        return 0;
    }
    let millis = duration.as_millis();
    if millis > i32::MAX as u128 {
        i32::MAX
    } else {
        millis as libc::c_int
    }
}

fn next_wait_slice(
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Option<StdDuration>> {
    if cancellation.is_some_and(|context| context.is_cancelled()) {
        return Err(cancelled_resource_error());
    }

    let poll_slice = StdDuration::from_millis(50);
    match deadline {
        Some(deadline) => {
            let now = Instant::now();
            if now >= deadline {
                Err(timeout_resource_error())
            } else {
                Ok(Some(std::cmp::min(
                    deadline.saturating_duration_since(now),
                    poll_slice,
                )))
            }
        }
        None if cancellation.is_some() => Ok(Some(poll_slice)),
        None => Ok(None),
    }
}

fn is_retryable_network_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

fn ensure_rustls_crypto_provider() {
    static INSTALL_PROVIDER: Once = Once::new();
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(not(unix))]
trait ReadTimeoutStream: Read {
    fn set_read_timeout_value(&mut self, timeout: Option<StdDuration>) -> io::Result<()>;
}

#[cfg(not(unix))]
impl ReadTimeoutStream for StdTcpStream {
    fn set_read_timeout_value(&mut self, timeout: Option<StdDuration>) -> io::Result<()> {
        self.set_read_timeout(timeout)
    }
}

#[cfg(not(unix))]
impl ReadTimeoutStream for rustls::StreamOwned<ClientConnection, StdTcpStream> {
    fn set_read_timeout_value(&mut self, timeout: Option<StdDuration>) -> io::Result<()> {
        self.sock.set_read_timeout(timeout)
    }
}

#[cfg(not(unix))]
impl ReadTimeoutStream for rustls::StreamOwned<ServerConnection, StdTcpStream> {
    fn set_read_timeout_value(&mut self, timeout: Option<StdDuration>) -> io::Result<()> {
        self.sock.set_read_timeout(timeout)
    }
}

#[cfg(unix)]
fn wait_for_fd_event(
    fd: i32,
    events: i16,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<()> {
    if let Some(reason) = yield_current_lightweight_wait(TaskWaitRegistration {
        channels: Vec::new(),
        ignore_closed_channels: false,
        deadline,
        cancellation: cancellation.cloned(),
        fd_wait: Some(FdWaitRegistration { fd, events }),
    }) {
        return match reason {
            RuntimeSchedulerWakeReason::Ready => Ok(()),
            RuntimeSchedulerWakeReason::TimedOut => Err(timeout_resource_error()),
            RuntimeSchedulerWakeReason::Cancelled => Err(cancelled_resource_error()),
        };
    }

    loop {
        let slice = next_wait_slice(deadline, cancellation)?;
        let timeout_ms = match slice {
            Some(slice) => duration_to_poll_timeout(slice),
            None => -1,
        };
        let mut descriptor = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
        if result > 0 {
            return Ok(());
        }
        if result == 0 {
            if deadline.is_some() && deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(timeout_resource_error());
            }
            if deadline.is_none() && cancellation.is_none() {
                return Err(timeout_resource_error());
            }
            continue;
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(error);
    }
}

pub(crate) fn io_decode_utf8(bytes: &[u8]) -> io::Result<String> {
    String::from_utf8(bytes.to_vec()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("received non-UTF-8 data: {}", error),
        )
    })
}

fn trim_line_endings(text: String) -> String {
    text.trim_end_matches('\n')
        .trim_end_matches('\r')
        .to_string()
}

#[cfg(unix)]
fn read_line_with_fd_deadline<R>(
    reader: &mut R,
    fd: i32,
    readiness_events: i16,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Option<String>>
where
    R: Read,
{
    let mut buffer = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        match reader.read(&mut byte) {
            Ok(0) if buffer.is_empty() => return Ok(None),
            Ok(0) => break,
            Ok(_) => {
                buffer.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(error) if is_retryable_network_error(&error) => {
                wait_for_fd_event(fd, readiness_events, deadline, cancellation)?;
            }
            Err(error) => return Err(error),
        }
    }

    Ok(Some(trim_line_endings(io_decode_utf8(&buffer)?)))
}

#[cfg(unix)]
fn read_line_with_deadline<R>(
    reader: &mut R,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Option<String>>
where
    R: Read + AsRawFd,
{
    read_line_with_fd_deadline(
        reader,
        reader.as_raw_fd(),
        libc::POLLIN,
        deadline,
        cancellation,
    )
}

#[cfg(not(unix))]
fn read_line_with_deadline<R>(
    reader: &mut R,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Option<String>>
where
    R: ReadTimeoutStream,
{
    let mut buffer = Vec::new();
    loop {
        reader.set_read_timeout_value(next_wait_slice(deadline, cancellation)?)?;
        let mut byte = [0u8; 1];
        match reader.read(&mut byte) {
            Ok(0) if buffer.is_empty() => return Ok(None),
            Ok(0) => break,
            Ok(_) => {
                buffer.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(error) if is_retryable_network_error(&error) => continue,
            Err(error) => return Err(error),
        }
    }

    Ok(Some(trim_line_endings(io_decode_utf8(&buffer)?)))
}

#[cfg(unix)]
fn read_exact_with_fd_deadline<R>(
    reader: &mut R,
    fd: i32,
    count: usize,
    readiness_events: i16,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Vec<u8>>
where
    R: Read,
{
    let mut buffer = vec![0u8; count];
    let mut offset = 0;
    while offset < count {
        match reader.read(&mut buffer[offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stream closed before reading the requested number of bytes",
                ))
            }
            Ok(bytes) => offset += bytes,
            Err(error) if is_retryable_network_error(&error) => {
                wait_for_fd_event(fd, readiness_events, deadline, cancellation)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(buffer)
}

#[cfg(unix)]
fn read_exact_with_deadline<R>(
    reader: &mut R,
    count: usize,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Vec<u8>>
where
    R: Read + AsRawFd,
{
    read_exact_with_fd_deadline(
        reader,
        reader.as_raw_fd(),
        count,
        libc::POLLIN,
        deadline,
        cancellation,
    )
}

#[cfg(not(unix))]
fn read_exact_with_deadline<R>(
    reader: &mut R,
    count: usize,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Vec<u8>>
where
    R: ReadTimeoutStream,
{
    let mut buffer = vec![0u8; count];
    let mut offset = 0;
    while offset < count {
        reader.set_read_timeout_value(next_wait_slice(deadline, cancellation)?)?;
        match reader.read(&mut buffer[offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stream closed before reading the requested number of bytes",
                ))
            }
            Ok(bytes) => offset += bytes,
            Err(error) if is_retryable_network_error(&error) => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(buffer)
}

#[cfg(unix)]
fn read_some_with_fd_deadline<R>(
    reader: &mut R,
    fd: i32,
    max_bytes: usize,
    readiness_events: i16,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Option<Vec<u8>>>
where
    R: Read,
{
    let mut buffer = vec![0u8; max_bytes.max(1)];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(None),
            Ok(bytes) => {
                buffer.truncate(bytes);
                return Ok(Some(buffer));
            }
            Err(error) if is_retryable_network_error(&error) => {
                wait_for_fd_event(fd, readiness_events, deadline, cancellation)?;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn read_some_with_deadline<R>(
    reader: &mut R,
    max_bytes: usize,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Option<Vec<u8>>>
where
    R: Read + AsRawFd,
{
    read_some_with_fd_deadline(
        reader,
        reader.as_raw_fd(),
        max_bytes,
        libc::POLLIN,
        deadline,
        cancellation,
    )
}

#[cfg(not(unix))]
fn read_some_with_deadline<R>(
    reader: &mut R,
    max_bytes: usize,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Option<Vec<u8>>>
where
    R: ReadTimeoutStream,
{
    let mut buffer = vec![0u8; max_bytes.max(1)];
    loop {
        reader.set_read_timeout_value(next_wait_slice(deadline, cancellation)?)?;
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(None),
            Ok(bytes) => {
                buffer.truncate(bytes);
                return Ok(Some(buffer));
            }
            Err(error) if is_retryable_network_error(&error) => continue,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn read_all_with_fd_deadline<R>(
    reader: &mut R,
    fd: i32,
    readiness_events: i16,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Vec<u8>>
where
    R: Read,
{
    let mut contents = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(bytes) => contents.extend_from_slice(&chunk[..bytes]),
            Err(error) if is_retryable_network_error(&error) => {
                wait_for_fd_event(fd, readiness_events, deadline, cancellation)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(contents)
}

#[cfg(unix)]
fn read_all_with_deadline<R>(
    reader: &mut R,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Vec<u8>>
where
    R: Read + AsRawFd,
{
    read_all_with_fd_deadline(
        reader,
        reader.as_raw_fd(),
        libc::POLLIN,
        deadline,
        cancellation,
    )
}

#[cfg(unix)]
fn write_all_with_fd_deadline<W>(
    writer: &mut W,
    fd: i32,
    bytes: &[u8],
    readiness_events: i16,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<()>
where
    W: Write,
{
    let mut written = 0usize;
    while written < bytes.len() {
        match writer.write(&bytes[written..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "stream ended before enough bytes were written",
                ));
            }
            Ok(count) => {
                written += count;
            }
            Err(error) if is_retryable_network_error(&error) => {
                wait_for_fd_event(fd, readiness_events, deadline, cancellation)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn write_all_with_deadline<W>(
    writer: &mut W,
    bytes: &[u8],
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<()>
where
    W: Write + AsRawFd,
{
    write_all_with_fd_deadline(
        writer,
        writer.as_raw_fd(),
        bytes,
        libc::POLLOUT,
        deadline,
        cancellation,
    )
}

fn load_tls_server_config(
    cert_pem_path: &str,
    key_pem_path: &str,
) -> io::Result<Arc<ServerConfig>> {
    ensure_rustls_crypto_provider();
    let mut cert_reader = io::BufReader::new(StdFile::open(cert_pem_path)?);
    let cert_chain = certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<CertificateDer<'static>>, _>>()?;
    let mut key_reader = io::BufReader::new(StdFile::open(key_pem_path)?);
    let Some(private_key) = private_key(&mut key_reader)? else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "TLS private key PEM did not contain a key",
        ));
    };
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .map_err(io::Error::other)?;
    Ok(Arc::new(config))
}

fn load_tls_root_store(ca_pem_path: Option<&str>) -> io::Result<RootCertStore> {
    ensure_rustls_crypto_provider();
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(ca_pem_path) = ca_pem_path.filter(|path| !path.is_empty()) {
        let mut reader = io::BufReader::new(StdFile::open(ca_pem_path)?);
        for certificate in certs(&mut reader) {
            let certificate = certificate?;
            roots.add(certificate).map_err(io::Error::other)?;
        }
    }
    Ok(roots)
}

fn parse_http_response(
    status: i32,
    reason: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> HttpResponseValue {
    HttpResponseValue {
        status,
        reason,
        headers,
        body,
    }
}

const MAX_HTTP_HEADERS: usize = 64;
const MAX_HTTP_MESSAGE_BYTES: usize = 1024 * 1024;

fn http_reason_phrase(status: i32) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        426 => "Upgrade Required",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

fn http_header_name_eq(name: &str, expected: &str) -> bool {
    name.eq_ignore_ascii_case(expected)
}

fn parse_http_headers(headers: &[httparse::Header<'_>]) -> io::Result<Vec<(String, String)>> {
    headers
        .iter()
        .map(|header| {
            let value = std::str::from_utf8(header.value).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("received non-UTF-8 HTTP header value: {}", error),
                )
            })?;
            Ok((header.name.to_string(), value.to_string()))
        })
        .collect()
}

fn parse_http_content_length(headers: &[(String, String)]) -> io::Result<Option<usize>> {
    let mut content_length = None;
    for (name, value) in headers {
        if http_header_name_eq(name, "Transfer-Encoding") && !value.eq_ignore_ascii_case("identity")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Aurora HTTP currently does not support transfer-encoding other than identity",
            ));
        }
        if http_header_name_eq(name, "Content-Length") {
            let parsed = value.parse::<usize>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid HTTP content length `{}`: {}", value, error),
                )
            })?;
            if let Some(existing) = content_length {
                if existing != parsed {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "conflicting HTTP content-length headers",
                    ));
                }
            } else {
                content_length = Some(parsed);
            }
        }
    }
    Ok(content_length)
}

fn push_http_chunk(buffer: &mut Vec<u8>, chunk: &[u8]) -> io::Result<()> {
    if buffer.len().saturating_add(chunk.len()) > MAX_HTTP_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "HTTP message exceeds the supported size limit of {} bytes",
                MAX_HTTP_MESSAGE_BYTES
            ),
        ));
    }
    buffer.extend_from_slice(chunk);
    Ok(())
}

fn parse_http_request_head(
    buffer: &[u8],
) -> io::Result<Option<(usize, String, String, Vec<(String, String)>, usize)>> {
    let mut raw_headers = [httparse::EMPTY_HEADER; MAX_HTTP_HEADERS];
    let mut request = HttpParseRequest::new(&mut raw_headers);
    match request.parse(buffer).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid HTTP request: {}", error),
        )
    })? {
        HttpParseStatus::Complete(header_len) => {
            let method = request
                .method
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "HTTP request missing method")
                })?
                .to_string();
            let path = request
                .path
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "HTTP request missing path")
                })?
                .to_string();
            let headers = parse_http_headers(request.headers)?;
            let content_length = parse_http_content_length(&headers)?.unwrap_or(0);
            Ok(Some((header_len, method, path, headers, content_length)))
        }
        HttpParseStatus::Partial => Ok(None),
    }
}

fn parse_http_response_head(
    buffer: &[u8],
) -> io::Result<Option<(usize, i32, String, Vec<(String, String)>, Option<usize>)>> {
    let mut raw_headers = [httparse::EMPTY_HEADER; MAX_HTTP_HEADERS];
    let mut response = HttpParseResponse::new(&mut raw_headers);
    match response.parse(buffer).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid HTTP response: {}", error),
        )
    })? {
        HttpParseStatus::Complete(header_len) => {
            let status = i32::from(response.code.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "HTTP response missing status code",
                )
            })?);
            let reason = response
                .reason
                .unwrap_or(http_reason_phrase(status))
                .to_string();
            let headers = parse_http_headers(response.headers)?;
            let content_length = parse_http_content_length(&headers)?;
            Ok(Some((header_len, status, reason, headers, content_length)))
        }
        HttpParseStatus::Partial => Ok(None),
    }
}

fn read_http_request_from_stream(
    stream: &mut StdTcpStream,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<(String, String, Vec<(String, String)>, Vec<u8>)> {
    let mut buffer = Vec::new();
    let (header_len, method, path, headers, content_length) = loop {
        if let Some(parsed) = parse_http_request_head(&buffer)? {
            break parsed;
        }
        let Some(chunk) = read_some_with_deadline(stream, 4096, deadline, cancellation)? else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "stream closed before a complete HTTP request was received",
            ));
        };
        push_http_chunk(&mut buffer, &chunk)?;
    };

    while buffer.len() < header_len.saturating_add(content_length) {
        let Some(chunk) = read_some_with_deadline(stream, 4096, deadline, cancellation)? else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "stream closed before the HTTP request body was fully received",
            ));
        };
        push_http_chunk(&mut buffer, &chunk)?;
    }

    let body_end = header_len.saturating_add(content_length);
    Ok((method, path, headers, buffer[header_len..body_end].to_vec()))
}

fn read_http_response_from_stream(
    stream: &mut StdTcpStream,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<HttpResponseValue> {
    let mut buffer = Vec::new();
    let (header_len, status, reason, headers, content_length) = loop {
        if let Some(parsed) = parse_http_response_head(&buffer)? {
            break parsed;
        }
        let Some(chunk) = read_some_with_deadline(stream, 4096, deadline, cancellation)? else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "stream closed before a complete HTTP response was received",
            ));
        };
        push_http_chunk(&mut buffer, &chunk)?;
    };

    let body = if let Some(content_length) = content_length {
        while buffer.len() < header_len.saturating_add(content_length) {
            let Some(chunk) = read_some_with_deadline(stream, 4096, deadline, cancellation)? else {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stream closed before the HTTP response body was fully received",
                ));
            };
            push_http_chunk(&mut buffer, &chunk)?;
        }
        buffer[header_len..header_len.saturating_add(content_length)].to_vec()
    } else {
        let mut body = buffer[header_len..].to_vec();
        let rest = read_all_with_deadline(stream, deadline, cancellation)?;
        push_http_chunk(&mut body, &rest)?;
        body
    };

    Ok(parse_http_response(status, reason, headers, body))
}

fn build_http_request_bytes(
    method: &str,
    url: &Url,
    body: &[u8],
    headers: Vec<(String, String)>,
) -> Vec<u8> {
    let mut path = if url.path().is_empty() {
        "/".to_string()
    } else {
        url.path().to_string()
    };
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }

    let default_port = url.port_or_known_default();
    let host = match (url.host(), url.port()) {
        (Some(url::Host::Ipv6(host)), Some(port)) if Some(port) != default_port => {
            format!("[{}]:{}", host, port)
        }
        (Some(url::Host::Ipv6(host)), _) => format!("[{}]", host),
        (Some(url::Host::Ipv4(host)), Some(port)) if Some(port) != default_port => {
            format!("{}:{}", host, port)
        }
        (Some(url::Host::Ipv4(host)), _) => host.to_string(),
        (Some(url::Host::Domain(host)), Some(port)) if Some(port) != default_port => {
            format!("{}:{}", host, port)
        }
        (Some(url::Host::Domain(host)), _) => host.to_string(),
        (None, _) => String::new(),
    };

    let mut rendered = format!("{} {} HTTP/1.1\r\n", method, path).into_bytes();
    let mut saw_host = false;
    let mut saw_content_length = false;
    let mut saw_connection = false;
    for (name, value) in headers {
        if http_header_name_eq(&name, "Host") {
            saw_host = true;
        } else if http_header_name_eq(&name, "Content-Length") {
            saw_content_length = true;
        } else if http_header_name_eq(&name, "Connection") {
            saw_connection = true;
        }
        rendered.extend_from_slice(name.as_bytes());
        rendered.extend_from_slice(b": ");
        rendered.extend_from_slice(value.as_bytes());
        rendered.extend_from_slice(b"\r\n");
    }
    if !saw_host {
        rendered.extend_from_slice(b"Host: ");
        rendered.extend_from_slice(host.as_bytes());
        rendered.extend_from_slice(b"\r\n");
    }
    if !saw_content_length {
        rendered.extend_from_slice(b"Content-Length: ");
        rendered.extend_from_slice(body.len().to_string().as_bytes());
        rendered.extend_from_slice(b"\r\n");
    }
    if !saw_connection {
        rendered.extend_from_slice(b"Connection: close\r\n");
    }
    rendered.extend_from_slice(b"\r\n");
    rendered.extend_from_slice(body);
    rendered
}

fn write_http_response_to_stream(
    stream: &mut StdTcpStream,
    status: i32,
    headers: Vec<(String, String)>,
    body: &[u8],
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<()> {
    let mut rendered =
        format!("HTTP/1.1 {} {}\r\n", status, http_reason_phrase(status)).into_bytes();
    let mut saw_content_length = false;
    let mut saw_connection = false;
    for (name, value) in headers {
        if http_header_name_eq(&name, "Content-Length") {
            saw_content_length = true;
        } else if http_header_name_eq(&name, "Connection") {
            saw_connection = true;
        }
        rendered.extend_from_slice(name.as_bytes());
        rendered.extend_from_slice(b": ");
        rendered.extend_from_slice(value.as_bytes());
        rendered.extend_from_slice(b"\r\n");
    }
    if !saw_content_length {
        rendered.extend_from_slice(b"Content-Length: ");
        rendered.extend_from_slice(body.len().to_string().as_bytes());
        rendered.extend_from_slice(b"\r\n");
    }
    if !saw_connection {
        rendered.extend_from_slice(b"Connection: close\r\n");
    }
    rendered.extend_from_slice(b"\r\n");
    rendered.extend_from_slice(body);
    write_all_with_deadline(stream, &rendered, deadline, cancellation)
}

#[cfg(unix)]
fn websocket_raw_fd(socket: &WebSocketStateKind) -> i32 {
    match socket {
        WebSocketStateKind::Plain(socket) => socket.get_ref().as_raw_fd(),
        WebSocketStateKind::MaybeTls(socket) => match socket.get_ref() {
            MaybeTlsStream::Plain(stream) => stream.as_raw_fd(),
            MaybeTlsStream::Rustls(stream) => stream.get_ref().as_raw_fd(),
            _ => unreachable!("unsupported websocket transport"),
        },
    }
}

#[cfg(unix)]
fn maybe_tls_stream_raw_fd(stream: &MaybeTlsStream<StdTcpStream>) -> i32 {
    match stream {
        MaybeTlsStream::Plain(stream) => stream.as_raw_fd(),
        MaybeTlsStream::Rustls(stream) => stream.get_ref().as_raw_fd(),
        _ => unreachable!("unsupported websocket transport"),
    }
}

#[cfg(unix)]
trait WebSocketHandshakeStream {
    fn raw_fd(&self) -> i32;
}

#[cfg(unix)]
impl WebSocketHandshakeStream for StdTcpStream {
    fn raw_fd(&self) -> i32 {
        self.as_raw_fd()
    }
}

#[cfg(unix)]
impl WebSocketHandshakeStream for MaybeTlsStream<StdTcpStream> {
    fn raw_fd(&self) -> i32 {
        maybe_tls_stream_raw_fd(self)
    }
}

#[cfg(unix)]
fn finish_websocket_handshake<Role>(
    mut mid: tungstenite::handshake::MidHandshake<Role>,
    deadline: Option<Instant>,
) -> io::Result<Role::FinalResult>
where
    Role: tungstenite::handshake::HandshakeRole,
    Role::InternalStream: Read + Write + WebSocketHandshakeStream,
{
    loop {
        match mid.handshake() {
            Ok(result) => return Ok(result),
            Err(tungstenite::handshake::HandshakeError::Interrupted(next_mid)) => {
                wait_for_fd_event(
                    next_mid.get_ref().get_ref().raw_fd(),
                    libc::POLLIN | libc::POLLOUT,
                    deadline,
                    None,
                )?;
                mid = next_mid;
            }
            Err(tungstenite::handshake::HandshakeError::Failure(error)) => {
                return Err(io::Error::other(error));
            }
        }
    }
}

#[cfg(unix)]
fn websocket_set_nonblocking(socket: &mut WebSocketStateKind, enabled: bool) -> io::Result<()> {
    match socket {
        WebSocketStateKind::Plain(socket) => socket.get_mut().set_nonblocking(enabled),
        WebSocketStateKind::MaybeTls(socket) => match socket.get_mut() {
            MaybeTlsStream::Plain(stream) => stream.set_nonblocking(enabled),
            MaybeTlsStream::Rustls(stream) => stream.get_mut().set_nonblocking(enabled),
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported websocket transport",
            )),
        },
    }
}

fn websocket_read_message(
    socket: &mut WebSocketStateKind,
    timeout: Option<StdDuration>,
) -> io::Result<Option<Message>> {
    let deadline = deadline_from_timeout(timeout);
    loop {
        let result = match socket {
            WebSocketStateKind::Plain(socket) => socket.read(),
            WebSocketStateKind::MaybeTls(socket) => socket.read(),
        };

        match result {
            Ok(Message::Close(_)) => return Ok(None),
            Ok(message) => return Ok(Some(message)),
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Ok(None)
            }
            Err(tungstenite::Error::Io(error)) if is_retryable_network_error(&error) => {
                #[cfg(unix)]
                {
                    wait_for_fd_event(
                        websocket_raw_fd(socket),
                        libc::POLLIN | libc::POLLOUT,
                        deadline,
                        None,
                    )?;
                    continue;
                }
                #[cfg(not(unix))]
                {
                    continue;
                }
            }
            Err(error) => return Err(io::Error::other(error)),
        }
    }
}

impl FileValue {
    fn from_std(file: StdFile) -> Self {
        Self {
            inner: Arc::new(FileState {
                file: Mutex::new(Some(file)),
            }),
        }
    }

    pub(crate) fn open(path: &str) -> io::Result<Self> {
        Ok(Self::from_std(StdFile::open(path)?))
    }

    pub(crate) fn create(path: &str) -> io::Result<Self> {
        Ok(Self::from_std(StdFile::create(path)?))
    }

    pub(crate) fn append(path: &str) -> io::Result<Self> {
        Ok(Self::from_std(
            OpenOptions::new().create(true).append(true).open(path)?,
        ))
    }

    pub(crate) fn read_all(&self) -> io::Result<String> {
        let mut file = lock_mutex(&self.inner.file);
        let Some(file) = file.as_mut() else {
            return Err(closed_resource_error());
        };
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents)
    }

    pub(crate) fn read_bytes(&self) -> io::Result<Vec<u8>> {
        let mut file = lock_mutex(&self.inner.file);
        let Some(file) = file.as_mut() else {
            return Err(closed_resource_error());
        };
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        Ok(contents)
    }

    pub(crate) fn write_all(&self, text: &str) -> io::Result<()> {
        self.write_bytes(text.as_bytes())
    }

    pub(crate) fn write_bytes(&self, bytes: &[u8]) -> io::Result<()> {
        let mut file = lock_mutex(&self.inner.file);
        let Some(file) = file.as_mut() else {
            return Err(closed_resource_error());
        };
        file.write_all(bytes)
    }

    pub(crate) fn flush(&self) -> io::Result<()> {
        let mut file = lock_mutex(&self.inner.file);
        let Some(file) = file.as_mut() else {
            return Err(closed_resource_error());
        };
        file.flush()
    }

    pub(crate) fn close(&self) {
        let mut file = lock_mutex(&self.inner.file);
        *file = None;
    }
}

impl TcpListenerValue {
    fn from_std(listener: StdTcpListener) -> Self {
        #[cfg(unix)]
        listener
            .set_nonblocking(true)
            .expect("tcp listeners should switch to nonblocking mode");
        Self {
            inner: Arc::new(TcpListenerState {
                listener: Mutex::new(Some(listener)),
            }),
        }
    }

    pub(crate) fn bind(address: &str) -> io::Result<Self> {
        Ok(Self::from_std(StdTcpListener::bind(address)?))
    }

    pub(crate) fn accept(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<TcpStreamValue> {
        let mut listener = lock_mutex(&self.inner.listener);
        let Some(listener) = listener.as_mut() else {
            return Err(closed_resource_error());
        };
        let deadline = deadline_from_timeout(timeout);
        loop {
            match listener.accept() {
                Ok((stream, _)) => return Ok(TcpStreamValue::from_std(stream)),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    wait_for_fd_event(listener.as_raw_fd(), libc::POLLIN, deadline, cancellation)?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) fn local_addr(&self) -> io::Result<String> {
        let mut listener = lock_mutex(&self.inner.listener);
        let Some(listener) = listener.as_mut() else {
            return Err(closed_resource_error());
        };
        Ok(listener.local_addr()?.to_string())
    }

    pub(crate) fn close(&self) {
        let mut listener = lock_mutex(&self.inner.listener);
        *listener = None;
    }
}

impl TcpStreamValue {
    fn from_std(stream: StdTcpStream) -> Self {
        #[cfg(unix)]
        stream
            .set_nonblocking(true)
            .expect("tcp streams should switch to nonblocking mode");
        Self {
            inner: Arc::new(TcpStreamState {
                stream: Mutex::new(Some(stream)),
            }),
        }
    }

    pub(crate) fn connect(
        address: &str,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Self> {
        if cancellation.is_some_and(|context| context.is_cancelled()) {
            return Err(cancelled_resource_error());
        }
        let addresses = address.to_socket_addrs()?.collect::<Vec<_>>();
        if addresses.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{}` did not resolve to any socket addresses", address),
            ));
        }
        let last_error = if let Some(timeout) = timeout {
            let mut last_error = None;
            for candidate in addresses {
                match StdTcpStream::connect_timeout(&candidate, timeout) {
                    Ok(stream) => return Ok(Self::from_std(stream)),
                    Err(error) => last_error = Some(error),
                }
            }
            last_error
        } else {
            let mut last_error = None;
            for candidate in addresses {
                match StdTcpStream::connect(candidate) {
                    Ok(stream) => return Ok(Self::from_std(stream)),
                    Err(error) => last_error = Some(error),
                }
            }
            last_error
        };
        Err(last_error.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                "failed to connect to any socket address",
            )
        }))
    }

    pub(crate) fn read_all(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<String> {
        Ok(io_decode_utf8(
            &self.read_bytes_all(timeout, cancellation)?,
        )?)
    }

    pub(crate) fn read_bytes_all(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Vec<u8>> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        #[cfg(unix)]
        {
            read_all_with_deadline(stream, deadline_from_timeout(timeout), cancellation)
        }
        #[cfg(not(unix))]
        {
            let mut contents = Vec::new();
            let deadline = deadline_from_timeout(timeout);
            loop {
                stream.set_read_timeout(next_wait_slice(deadline, cancellation)?)?;
                let mut chunk = [0u8; 4096];
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(bytes) => contents.extend_from_slice(&chunk[..bytes]),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        if deadline.is_some() && Instant::now() >= deadline.unwrap() {
                            return Err(timeout_resource_error());
                        }
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(contents)
        }
    }

    pub(crate) fn read_line(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<String>> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        read_line_with_deadline(stream, deadline_from_timeout(timeout), cancellation)
    }

    pub(crate) fn read_bytes(
        &self,
        max_bytes: usize,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<Vec<u8>>> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        read_some_with_deadline(
            stream,
            max_bytes,
            deadline_from_timeout(timeout),
            cancellation,
        )
    }

    pub(crate) fn read_exact(
        &self,
        count: usize,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Vec<u8>> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        read_exact_with_deadline(stream, count, deadline_from_timeout(timeout), cancellation)
    }

    pub(crate) fn write_all(
        &self,
        text: &str,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<()> {
        self.write_bytes(text.as_bytes(), timeout, cancellation)
    }

    pub(crate) fn write_bytes(
        &self,
        bytes: &[u8],
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<()> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        #[cfg(unix)]
        {
            write_all_with_deadline(stream, bytes, deadline_from_timeout(timeout), cancellation)
        }
        #[cfg(not(unix))]
        {
            stream.set_write_timeout(next_wait_slice(
                deadline_from_timeout(timeout),
                cancellation,
            )?)?;
            stream.write_all(bytes)
        }
    }

    pub(crate) fn flush(&self) -> io::Result<()> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        stream.flush()
    }

    pub(crate) fn local_addr(&self) -> io::Result<String> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        Ok(stream.local_addr()?.to_string())
    }

    pub(crate) fn peer_addr(&self) -> io::Result<String> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        Ok(stream.peer_addr()?.to_string())
    }

    pub(crate) fn close(&self) {
        let mut stream = lock_mutex(&self.inner.stream);
        if let Some(stream) = stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    pub(crate) fn shutdown_read(&self) -> io::Result<()> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        stream.shutdown(Shutdown::Read)
    }

    pub(crate) fn shutdown_write(&self) -> io::Result<()> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        stream.shutdown(Shutdown::Write)
    }

    pub(crate) fn shutdown_both(&self) -> io::Result<()> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        stream.shutdown(Shutdown::Both)
    }
}

impl UdpDatagramValue {
    pub(crate) fn address(&self) -> String {
        self.address.clone()
    }

    pub(crate) fn bytes(&self) -> Vec<u8> {
        self.data.clone()
    }

    pub(crate) fn text(&self) -> io::Result<String> {
        io_decode_utf8(&self.data)
    }
}

impl UdpSocketValue {
    fn from_std(socket: StdUdpSocket) -> Self {
        #[cfg(unix)]
        socket
            .set_nonblocking(true)
            .expect("udp sockets should switch to nonblocking mode");
        Self {
            inner: Arc::new(UdpSocketState {
                socket: Mutex::new(Some(socket)),
            }),
        }
    }

    pub(crate) fn bind(address: &str) -> io::Result<Self> {
        Ok(Self::from_std(StdUdpSocket::bind(address)?))
    }

    pub(crate) fn send_to_text(
        &self,
        address: &str,
        text: &str,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<()> {
        self.send_to_bytes(address, text.as_bytes(), timeout, cancellation)
    }

    pub(crate) fn send_to_bytes(
        &self,
        address: &str,
        bytes: &[u8],
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<()> {
        let mut socket = lock_mutex(&self.inner.socket);
        let Some(socket) = socket.as_mut() else {
            return Err(closed_resource_error());
        };
        #[cfg(unix)]
        {
            let deadline = deadline_from_timeout(timeout);
            loop {
                match socket.send_to(bytes, address) {
                    Ok(_) => return Ok(()),
                    Err(error) if is_retryable_network_error(&error) => {
                        wait_for_fd_event(
                            socket.as_raw_fd(),
                            libc::POLLOUT,
                            deadline,
                            cancellation,
                        )?;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        #[cfg(not(unix))]
        {
            socket.set_write_timeout(next_wait_slice(
                deadline_from_timeout(timeout),
                cancellation,
            )?)?;
            socket.send_to(bytes, address)?;
            Ok(())
        }
    }

    pub(crate) fn recv(
        &self,
        max_bytes: usize,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<Vec<u8>>> {
        let mut socket = lock_mutex(&self.inner.socket);
        let Some(socket) = socket.as_mut() else {
            return Err(closed_resource_error());
        };
        #[cfg(unix)]
        {
            let deadline = deadline_from_timeout(timeout);
            let mut buffer = vec![0u8; max_bytes.max(1)];
            loop {
                match socket.recv(&mut buffer) {
                    Ok(0) => return Ok(None),
                    Ok(bytes) => {
                        buffer.truncate(bytes);
                        return Ok(Some(buffer));
                    }
                    Err(error) if is_retryable_network_error(&error) => {
                        wait_for_fd_event(
                            socket.as_raw_fd(),
                            libc::POLLIN,
                            deadline,
                            cancellation,
                        )?;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        #[cfg(not(unix))]
        {
            socket.set_read_timeout(next_wait_slice(
                deadline_from_timeout(timeout),
                cancellation,
            )?)?;
            let mut buffer = vec![0u8; max_bytes.max(1)];
            match socket.recv(&mut buffer) {
                Ok(0) => Ok(None),
                Ok(bytes) => {
                    buffer.truncate(bytes);
                    Ok(Some(buffer))
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    Ok(None)
                }
                Err(error) => Err(error),
            }
        }
    }

    pub(crate) fn recv_from(
        &self,
        max_bytes: usize,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<UdpDatagramValue>> {
        let mut socket = lock_mutex(&self.inner.socket);
        let Some(socket) = socket.as_mut() else {
            return Err(closed_resource_error());
        };
        #[cfg(unix)]
        {
            let deadline = deadline_from_timeout(timeout);
            let mut buffer = vec![0u8; max_bytes.max(1)];
            loop {
                match socket.recv_from(&mut buffer) {
                    Ok((bytes, address)) => {
                        buffer.truncate(bytes);
                        return Ok(Some(UdpDatagramValue {
                            address: address.to_string(),
                            data: buffer,
                        }));
                    }
                    Err(error) if is_retryable_network_error(&error) => {
                        wait_for_fd_event(
                            socket.as_raw_fd(),
                            libc::POLLIN,
                            deadline,
                            cancellation,
                        )?;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        #[cfg(not(unix))]
        {
            socket.set_read_timeout(next_wait_slice(
                deadline_from_timeout(timeout),
                cancellation,
            )?)?;
            let mut buffer = vec![0u8; max_bytes.max(1)];
            match socket.recv_from(&mut buffer) {
                Ok((bytes, address)) => {
                    buffer.truncate(bytes);
                    Ok(Some(UdpDatagramValue {
                        address: address.to_string(),
                        data: buffer,
                    }))
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    Ok(None)
                }
                Err(error) => Err(error),
            }
        }
    }

    pub(crate) fn local_addr(&self) -> io::Result<String> {
        let socket = lock_mutex(&self.inner.socket);
        let Some(socket) = socket.as_ref() else {
            return Err(closed_resource_error());
        };
        Ok(socket.local_addr()?.to_string())
    }

    pub(crate) fn peer_addr(&self) -> io::Result<String> {
        let socket = lock_mutex(&self.inner.socket);
        let Some(socket) = socket.as_ref() else {
            return Err(closed_resource_error());
        };
        Ok(socket.peer_addr()?.to_string())
    }

    pub(crate) fn close(&self) {
        let mut socket = lock_mutex(&self.inner.socket);
        *socket = None;
    }
}

#[cfg(unix)]
impl UnixListenerValue {
    fn from_std(listener: StdUnixListener) -> Self {
        listener
            .set_nonblocking(true)
            .expect("unix listeners should switch to nonblocking mode");
        Self {
            inner: Arc::new(UnixListenerState {
                listener: Mutex::new(Some(listener)),
            }),
        }
    }

    pub(crate) fn bind(path: &str) -> io::Result<Self> {
        let _ = fs::remove_file(path);
        Ok(Self::from_std(StdUnixListener::bind(path)?))
    }

    pub(crate) fn accept(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<UnixStreamValue> {
        let mut listener = lock_mutex(&self.inner.listener);
        let Some(listener) = listener.as_mut() else {
            return Err(closed_resource_error());
        };
        let deadline = deadline_from_timeout(timeout);
        loop {
            match listener.accept() {
                Ok((stream, _)) => return Ok(UnixStreamValue::from_std(stream)),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    wait_for_fd_event(listener.as_raw_fd(), libc::POLLIN, deadline, cancellation)?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) fn close(&self) {
        let mut listener = lock_mutex(&self.inner.listener);
        *listener = None;
    }
}

#[cfg(not(unix))]
impl UnixListenerValue {
    pub(crate) fn bind(_path: &str) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unix domain sockets are not supported on this platform",
        ))
    }
}

#[cfg(unix)]
impl UnixStreamValue {
    fn from_std(stream: StdUnixStream) -> Self {
        stream
            .set_nonblocking(true)
            .expect("unix streams should switch to nonblocking mode");
        Self {
            inner: Arc::new(UnixStreamState {
                stream: Mutex::new(Some(stream)),
            }),
        }
    }

    pub(crate) fn connect(
        path: &str,
        _timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Self> {
        if cancellation.is_some_and(|context| context.is_cancelled()) {
            return Err(cancelled_resource_error());
        }
        Ok(Self::from_std(StdUnixStream::connect(path)?))
    }

    pub(crate) fn read_line(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<String>> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        read_line_with_deadline(stream, deadline_from_timeout(timeout), cancellation)
    }

    pub(crate) fn read_exact(
        &self,
        count: usize,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Vec<u8>> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        read_exact_with_deadline(stream, count, deadline_from_timeout(timeout), cancellation)
    }

    pub(crate) fn write_all(
        &self,
        text: &str,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<()> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        write_all_with_deadline(
            stream,
            text.as_bytes(),
            deadline_from_timeout(timeout),
            cancellation,
        )
    }

    pub(crate) fn close(&self) {
        let mut stream = lock_mutex(&self.inner.stream);
        *stream = None;
    }
}

#[cfg(not(unix))]
impl UnixStreamValue {
    pub(crate) fn connect(
        _path: &str,
        _timeout: Option<StdDuration>,
        _cancellation: Option<&CancellationContext>,
    ) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unix domain sockets are not supported on this platform",
        ))
    }
}

impl TlsListenerValue {
    pub(crate) fn bind(address: &str, cert_pem_path: &str, key_pem_path: &str) -> io::Result<Self> {
        let listener = StdTcpListener::bind(address)?;
        #[cfg(unix)]
        listener.set_nonblocking(true)?;
        Ok(Self {
            inner: Arc::new(TlsListenerState {
                listener: Mutex::new(Some(listener)),
                config: load_tls_server_config(cert_pem_path, key_pem_path)?,
            }),
        })
    }

    pub(crate) fn accept(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<TlsStreamValue> {
        let mut listener = lock_mutex(&self.inner.listener);
        let Some(listener) = listener.as_mut() else {
            return Err(closed_resource_error());
        };
        let deadline = deadline_from_timeout(timeout);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    #[cfg(unix)]
                    stream.set_nonblocking(true)?;
                    let connection = ServerConnection::new(self.inner.config.clone())
                        .map_err(io::Error::other)?;
                    return Ok(TlsStreamValue {
                        inner: Arc::new(TlsStreamState {
                            stream: Mutex::new(Some(TlsStreamKind::Server(
                                rustls::StreamOwned::new(connection, stream),
                            ))),
                        }),
                    });
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    wait_for_fd_event(listener.as_raw_fd(), libc::POLLIN, deadline, cancellation)?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) fn local_addr(&self) -> io::Result<String> {
        let listener = lock_mutex(&self.inner.listener);
        let Some(listener) = listener.as_ref() else {
            return Err(closed_resource_error());
        };
        Ok(listener.local_addr()?.to_string())
    }

    pub(crate) fn close(&self) {
        let mut listener = lock_mutex(&self.inner.listener);
        *listener = None;
    }
}

impl TlsStreamValue {
    pub(crate) fn connect(
        address: &str,
        server_name: &str,
        ca_pem_path: Option<&str>,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Self> {
        ensure_rustls_crypto_provider();
        let tcp = TcpStreamValue::connect(address, timeout, cancellation)?;
        let mut guard = lock_mutex(&tcp.inner.stream);
        let Some(stream) = guard.take() else {
            return Err(closed_resource_error());
        };
        let config = ClientConfig::builder()
            .with_root_certificates(load_tls_root_store(ca_pem_path)?)
            .with_no_client_auth();
        let server_name = ServerName::try_from(server_name.to_string())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid TLS server name"))?;
        let connection =
            ClientConnection::new(Arc::new(config), server_name).map_err(io::Error::other)?;
        Ok(Self {
            inner: Arc::new(TlsStreamState {
                stream: Mutex::new(Some(TlsStreamKind::Client(rustls::StreamOwned::new(
                    connection, stream,
                )))),
            }),
        })
    }

    pub(crate) fn read_line(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<String>> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        match stream {
            TlsStreamKind::Client(stream) => read_line_with_fd_deadline(
                stream,
                stream.sock.as_raw_fd(),
                libc::POLLIN | libc::POLLOUT,
                deadline_from_timeout(timeout),
                cancellation,
            ),
            TlsStreamKind::Server(stream) => read_line_with_fd_deadline(
                stream,
                stream.sock.as_raw_fd(),
                libc::POLLIN | libc::POLLOUT,
                deadline_from_timeout(timeout),
                cancellation,
            ),
        }
    }

    pub(crate) fn read_exact(
        &self,
        count: usize,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Vec<u8>> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        match stream {
            TlsStreamKind::Client(stream) => read_exact_with_fd_deadline(
                stream,
                stream.sock.as_raw_fd(),
                count,
                libc::POLLIN | libc::POLLOUT,
                deadline_from_timeout(timeout),
                cancellation,
            ),
            TlsStreamKind::Server(stream) => read_exact_with_fd_deadline(
                stream,
                stream.sock.as_raw_fd(),
                count,
                libc::POLLIN | libc::POLLOUT,
                deadline_from_timeout(timeout),
                cancellation,
            ),
        }
    }

    pub(crate) fn write_all(
        &self,
        text: &str,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<()> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.as_mut() else {
            return Err(closed_resource_error());
        };
        match stream {
            TlsStreamKind::Client(stream) => write_all_with_fd_deadline(
                stream,
                stream.sock.as_raw_fd(),
                text.as_bytes(),
                libc::POLLIN | libc::POLLOUT,
                deadline_from_timeout(timeout),
                cancellation,
            ),
            TlsStreamKind::Server(stream) => write_all_with_fd_deadline(
                stream,
                stream.sock.as_raw_fd(),
                text.as_bytes(),
                libc::POLLIN | libc::POLLOUT,
                deadline_from_timeout(timeout),
                cancellation,
            ),
        }
    }

    pub(crate) fn close(&self) {
        let mut stream = lock_mutex(&self.inner.stream);
        if let Some(stream) = stream.take() {
            match stream {
                TlsStreamKind::Client(stream) => {
                    let _ = stream.sock.shutdown(Shutdown::Both);
                }
                TlsStreamKind::Server(stream) => {
                    let _ = stream.sock.shutdown(Shutdown::Both);
                }
            }
        }
    }
}

impl HttpListenerValue {
    pub(crate) fn bind(address: &str) -> io::Result<Self> {
        let listener = StdTcpListener::bind(address)?;
        #[cfg(unix)]
        listener.set_nonblocking(true)?;
        Ok(Self {
            inner: Arc::new(HttpListenerState {
                listener: Mutex::new(Some(listener)),
            }),
        })
    }

    pub(crate) fn accept(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<HttpExchangeValue> {
        let mut listener = lock_mutex(&self.inner.listener);
        let Some(listener) = listener.as_mut() else {
            return Err(closed_resource_error());
        };
        let deadline = deadline_from_timeout(timeout);
        #[cfg(unix)]
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break TcpStreamValue::from_std(stream),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    wait_for_fd_event(listener.as_raw_fd(), libc::POLLIN, deadline, cancellation)?;
                }
                Err(error) => return Err(error),
            }
        };
        #[cfg(not(unix))]
        let stream = TcpStreamValue::from_std(listener.accept()?.0);
        let (method, path, headers, body) = {
            let mut raw_stream = lock_mutex(&stream.inner.stream);
            let Some(raw_stream) = raw_stream.as_mut() else {
                return Err(closed_resource_error());
            };
            read_http_request_from_stream(raw_stream, deadline, cancellation)?
        };
        Ok(HttpExchangeValue {
            inner: Arc::new(HttpExchangeState {
                stream: Mutex::new(Some(stream)),
                method,
                path,
                headers,
                body,
            }),
        })
    }

    pub(crate) fn local_addr(&self) -> io::Result<String> {
        let listener = lock_mutex(&self.inner.listener);
        let Some(listener) = listener.as_ref() else {
            return Err(closed_resource_error());
        };
        Ok(listener.local_addr()?.to_string())
    }

    pub(crate) fn close(&self) {
        let mut listener = lock_mutex(&self.inner.listener);
        *listener = None;
    }
}

impl HttpExchangeValue {
    pub(crate) fn method(&self) -> String {
        self.inner.method.clone()
    }

    pub(crate) fn path(&self) -> String {
        self.inner.path.clone()
    }

    pub(crate) fn headers(&self) -> Vec<(String, String)> {
        self.inner.headers.clone()
    }

    pub(crate) fn body_text(&self) -> io::Result<String> {
        io_decode_utf8(&self.inner.body)
    }

    pub(crate) fn body_bytes(&self) -> Vec<u8> {
        self.inner.body.clone()
    }

    pub(crate) fn respond_text(
        &self,
        status: i32,
        body: &str,
        headers: Vec<(String, String)>,
    ) -> io::Result<()> {
        self.respond_bytes(status, body.as_bytes(), headers)
    }

    pub(crate) fn respond_bytes(
        &self,
        status: i32,
        body: &[u8],
        headers: Vec<(String, String)>,
    ) -> io::Result<()> {
        let mut stream = lock_mutex(&self.inner.stream);
        let Some(stream) = stream.take() else {
            return Err(closed_resource_error());
        };
        let result = {
            let mut raw_stream = lock_mutex(&stream.inner.stream);
            let Some(raw_stream) = raw_stream.as_mut() else {
                return Err(closed_resource_error());
            };
            write_http_response_to_stream(raw_stream, status, headers, body, None, None)
        };
        stream.close();
        result
    }
}

impl HttpResponseValue {
    pub(crate) fn request_text(
        method: &str,
        url: &str,
        body: &str,
        headers: Vec<(String, String)>,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Self> {
        Self::request_bytes(method, url, body.as_bytes(), headers, timeout, cancellation)
    }

    pub(crate) fn request_bytes(
        method: &str,
        url: &str,
        body: &[u8],
        headers: Vec<(String, String)>,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Self> {
        let url = Url::parse(url).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid URL `{}`: {}", url, error),
            )
        })?;
        if url.scheme() != "http" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Aurora HTTP requests currently require `http://` URLs, found `{}`",
                    url
                ),
            ));
        }
        let host = match url.host() {
            Some(url::Host::Ipv6(host)) => {
                format!("[{}]:{}", host, url.port_or_known_default().unwrap_or(80))
            }
            Some(url::Host::Ipv4(host)) => {
                format!("{}:{}", host, url.port_or_known_default().unwrap_or(80))
            }
            Some(url::Host::Domain(host)) => {
                format!("{}:{}", host, url.port_or_known_default().unwrap_or(80))
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid URL `{}`: missing host", url),
                ))
            }
        };
        let request = build_http_request_bytes(method, &url, body, headers);
        let stream = TcpStreamValue::connect(&host, timeout, cancellation)?;
        let deadline = deadline_from_timeout(timeout);
        let response = {
            let mut raw_stream = lock_mutex(&stream.inner.stream);
            let Some(raw_stream) = raw_stream.as_mut() else {
                return Err(closed_resource_error());
            };
            write_all_with_deadline(raw_stream, &request, deadline, cancellation)?;
            read_http_response_from_stream(raw_stream, deadline, cancellation)?
        };
        stream.close();
        Ok(response)
    }

    pub(crate) fn status(&self) -> i32 {
        self.status
    }

    pub(crate) fn reason(&self) -> String {
        self.reason.clone()
    }

    pub(crate) fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }

    pub(crate) fn text(&self) -> io::Result<String> {
        io_decode_utf8(&self.body)
    }

    pub(crate) fn bytes(&self) -> Vec<u8> {
        self.body.clone()
    }
}

impl WebSocketListenerValue {
    pub(crate) fn bind(address: &str) -> io::Result<Self> {
        let listener = StdTcpListener::bind(address)?;
        #[cfg(unix)]
        listener.set_nonblocking(true)?;
        Ok(Self {
            inner: Arc::new(WebSocketListenerState {
                listener: Mutex::new(Some(listener)),
            }),
        })
    }

    pub(crate) fn accept(&self, timeout: Option<StdDuration>) -> io::Result<WebSocketValue> {
        let mut listener = lock_mutex(&self.inner.listener);
        let Some(listener) = listener.as_mut() else {
            return Err(closed_resource_error());
        };
        let deadline = deadline_from_timeout(timeout);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    #[cfg(unix)]
                    stream.set_nonblocking(true)?;
                    let socket = match websocket_accept(stream) {
                        Ok(socket) => socket,
                        #[cfg(unix)]
                        Err(tungstenite::handshake::HandshakeError::Interrupted(mid)) => {
                            finish_websocket_handshake(mid, deadline)?
                        }
                        #[cfg(not(unix))]
                        Err(tungstenite::handshake::HandshakeError::Interrupted(_)) => {
                            return Err(io::Error::new(
                                io::ErrorKind::WouldBlock,
                                "websocket handshake unexpectedly blocked",
                            ));
                        }
                        Err(tungstenite::handshake::HandshakeError::Failure(error)) => {
                            return Err(io::Error::other(error));
                        }
                    };
                    let mut state = WebSocketStateKind::Plain(socket);
                    #[cfg(unix)]
                    websocket_set_nonblocking(&mut state, true)?;
                    return Ok(WebSocketValue {
                        inner: Arc::new(WebSocketState {
                            socket: Mutex::new(Some(state)),
                        }),
                    });
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    wait_for_fd_event(listener.as_raw_fd(), libc::POLLIN, deadline, None)?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) fn local_addr(&self) -> io::Result<String> {
        let listener = lock_mutex(&self.inner.listener);
        let Some(listener) = listener.as_ref() else {
            return Err(closed_resource_error());
        };
        Ok(listener.local_addr()?.to_string())
    }
}

impl WebSocketValue {
    pub(crate) fn connect(url: &str, timeout: Option<StdDuration>) -> io::Result<Self> {
        #[cfg(unix)]
        {
            let parsed = url::Url::parse(url).map_err(io::Error::other)?;
            let host = parsed.host_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "websocket URL is missing a host",
                )
            })?;
            let port = parsed.port_or_known_default().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "websocket URL is missing a known port",
                )
            })?;
            let address = if host.contains(':') && !host.starts_with('[') {
                format!("[{host}]:{port}")
            } else {
                format!("{host}:{port}")
            };
            let tcp = TcpStreamValue::connect(&address, timeout, None)?;
            let mut guard = lock_mutex(&tcp.inner.stream);
            let Some(stream) = guard.take() else {
                return Err(closed_resource_error());
            };
            stream.set_nonblocking(true)?;
            stream.set_nodelay(true)?;
            let (socket, _) = match client_tls_with_config(url, stream, None, None) {
                Ok(result) => result,
                Err(tungstenite::handshake::HandshakeError::Interrupted(mid)) => {
                    finish_websocket_handshake(mid, deadline_from_timeout(timeout))?
                }
                Err(tungstenite::handshake::HandshakeError::Failure(error)) => {
                    return Err(io::Error::other(error));
                }
            };
            let mut state = WebSocketStateKind::MaybeTls(socket);
            websocket_set_nonblocking(&mut state, true)?;
            return Ok(Self {
                inner: Arc::new(WebSocketState {
                    socket: Mutex::new(Some(state)),
                }),
            });
        }

        #[cfg(not(unix))]
        {
            let (socket, _) = tungstenite::connect(url).map_err(io::Error::other)?;
            let mut state = WebSocketStateKind::MaybeTls(socket);
            let _ = timeout;
            Ok(Self {
                inner: Arc::new(WebSocketState {
                    socket: Mutex::new(Some(state)),
                }),
            })
        }
    }

    pub(crate) fn send_text(&self, text: &str, timeout: Option<StdDuration>) -> io::Result<()> {
        let mut socket = lock_mutex(&self.inner.socket);
        let deadline = deadline_from_timeout(timeout);
        let mut message = Message::Text(text.to_string().into());
        match socket.as_mut() {
            Some(socket) => loop {
                let result = match socket {
                    WebSocketStateKind::Plain(socket) => socket.send(message.clone()),
                    WebSocketStateKind::MaybeTls(socket) => socket.send(message.clone()),
                };
                match result {
                    Ok(()) => return Ok(()),
                    Err(tungstenite::Error::WriteBufferFull(returned)) => {
                        message = returned;
                        #[cfg(unix)]
                        wait_for_fd_event(websocket_raw_fd(socket), libc::POLLOUT, deadline, None)?;
                        #[cfg(not(unix))]
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "websocket write buffer is full",
                        ));
                    }
                    Err(tungstenite::Error::Io(error)) if is_retryable_network_error(&error) => {
                        #[cfg(unix)]
                        wait_for_fd_event(websocket_raw_fd(socket), libc::POLLOUT, deadline, None)?;
                        #[cfg(not(unix))]
                        return Err(error);
                    }
                    Err(error) => return Err(io::Error::other(error)),
                }
            },
            None => Err(closed_resource_error()),
        }
    }

    pub(crate) fn send_bytes(&self, bytes: &[u8], timeout: Option<StdDuration>) -> io::Result<()> {
        let mut socket = lock_mutex(&self.inner.socket);
        let deadline = deadline_from_timeout(timeout);
        let mut message = Message::Binary(bytes.to_vec().into());
        match socket.as_mut() {
            Some(socket) => loop {
                let result = match socket {
                    WebSocketStateKind::Plain(socket) => socket.send(message.clone()),
                    WebSocketStateKind::MaybeTls(socket) => socket.send(message.clone()),
                };
                match result {
                    Ok(()) => return Ok(()),
                    Err(tungstenite::Error::WriteBufferFull(returned)) => {
                        message = returned;
                        #[cfg(unix)]
                        wait_for_fd_event(websocket_raw_fd(socket), libc::POLLOUT, deadline, None)?;
                        #[cfg(not(unix))]
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "websocket write buffer is full",
                        ));
                    }
                    Err(tungstenite::Error::Io(error)) if is_retryable_network_error(&error) => {
                        #[cfg(unix)]
                        wait_for_fd_event(websocket_raw_fd(socket), libc::POLLOUT, deadline, None)?;
                        #[cfg(not(unix))]
                        return Err(error);
                    }
                    Err(error) => return Err(io::Error::other(error)),
                }
            },
            None => Err(closed_resource_error()),
        }
    }

    pub(crate) fn recv_text(&self, timeout: Option<StdDuration>) -> io::Result<Option<String>> {
        let mut socket = lock_mutex(&self.inner.socket);
        let message = match socket.as_mut() {
            Some(socket) => match websocket_read_message(socket, timeout)? {
                Some(message) => message,
                None => return Ok(None),
            },
            None => return Err(closed_resource_error()),
        };
        match message {
            Message::Text(text) => Ok(Some(text.to_string())),
            Message::Binary(bytes) => Ok(Some(io_decode_utf8(bytes.as_ref())?)),
            Message::Close(_) => Ok(None),
            _ => Ok(None),
        }
    }

    pub(crate) fn recv_bytes(&self, timeout: Option<StdDuration>) -> io::Result<Option<Vec<u8>>> {
        let mut socket = lock_mutex(&self.inner.socket);
        let message = match socket.as_mut() {
            Some(socket) => match websocket_read_message(socket, timeout)? {
                Some(message) => message,
                None => return Ok(None),
            },
            None => return Err(closed_resource_error()),
        };
        match message {
            Message::Text(text) => Ok(Some(text.as_str().as_bytes().to_vec())),
            Message::Binary(bytes) => Ok(Some(bytes.to_vec())),
            Message::Close(_) => Ok(None),
            _ => Ok(None),
        }
    }

    pub(crate) fn close(&self) -> io::Result<()> {
        let mut socket = lock_mutex(&self.inner.socket);
        match socket.as_mut() {
            Some(WebSocketStateKind::Plain(socket)) => socket.close(None).map_err(io::Error::other),
            Some(WebSocketStateKind::MaybeTls(socket)) => {
                socket.close(None).map_err(io::Error::other)
            }
            None => Err(closed_resource_error()),
        }
    }
}

pub(crate) fn io_read_line() -> io::Result<Option<String>> {
    let mut stdin = io::stdin().lock();
    let mut buffer = String::new();
    let bytes = stdin.read_line(&mut buffer)?;
    if bytes == 0 {
        return Ok(None);
    }
    Ok(Some(trim_line_endings(buffer)))
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
        runtime_scheduler().notify();
    }

    // Invariant: callers drain only after they have finished registering tasks for the group.
    pub(crate) fn drain_tasks(&self) -> Vec<TaskValue> {
        let mut tasks = lock_mutex(&self.inner.tasks);
        std::mem::take(&mut *tasks)
    }
}

impl TaskValue {
    fn completed_result(&self) -> Option<TaskResult> {
        let state = lock_mutex(&self.inner.handle);
        match &*state {
            TaskHandle::Completed(result) => Some(result.clone()),
            TaskHandle::Running { .. } => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_handle(handle: thread::JoinHandle<TaskResult>) -> Self {
        let inner = Arc::new(TaskState {
            handle: Mutex::new(TaskHandle::Running {
                waiters: Vec::new(),
            }),
            ready: Condvar::new(),
            lightweight: false,
        });
        let state = inner.clone();
        thread::spawn(move || {
            let result = match handle.join() {
                Ok(result) => result,
                Err(_) => Err(Diagnostic::new("spawned task panicked")),
            };
            let mut task_state = lock_mutex(&state.handle);
            *task_state = TaskHandle::Completed(result);
            state.ready.notify_all();
        });
        Self { inner }
    }

    pub(crate) fn join_result(&self) -> TaskResult {
        loop {
            if let Some(result) = self.completed_result() {
                return result;
            }

            if self.inner.lightweight {
                if let Some(context) = current_lightweight_task_context() {
                    {
                        let mut state = lock_mutex(&self.inner.handle);
                        match &mut *state {
                            TaskHandle::Completed(result) => return result.clone(),
                            TaskHandle::Running { waiters } => {
                                if !waiters.contains(&context.task_id) {
                                    waiters.push(context.task_id);
                                }
                            }
                        }
                    }
                    let _ = park_current_lightweight_task();
                    continue;
                }
            }

            let mut state = lock_mutex(&self.inner.handle);
            loop {
                match &*state {
                    TaskHandle::Completed(result) => return result.clone(),
                    TaskHandle::Running { .. } => {
                        state = wait_condvar(&self.inner.ready, state);
                    }
                }
            }
        }
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

pub(crate) fn io_error(error: io::Error) -> Value {
    let (variant_name, payloads) = match error.kind() {
        io::ErrorKind::NotFound => ("NotFound", Vec::new()),
        io::ErrorKind::PermissionDenied => ("PermissionDenied", Vec::new()),
        io::ErrorKind::AlreadyExists => ("AlreadyExists", Vec::new()),
        io::ErrorKind::ConnectionRefused => ("ConnectionRefused", Vec::new()),
        io::ErrorKind::ConnectionReset => ("ConnectionReset", Vec::new()),
        io::ErrorKind::ConnectionAborted => ("ConnectionAborted", Vec::new()),
        io::ErrorKind::NotConnected => ("NotConnected", Vec::new()),
        io::ErrorKind::AddrInUse => ("AddrInUse", Vec::new()),
        io::ErrorKind::AddrNotAvailable => ("AddrNotAvailable", Vec::new()),
        io::ErrorKind::BrokenPipe => ("BrokenPipe", Vec::new()),
        io::ErrorKind::TimedOut => ("TimedOut", Vec::new()),
        io::ErrorKind::WouldBlock => ("WouldBlock", Vec::new()),
        io::ErrorKind::UnexpectedEof => ("UnexpectedEof", Vec::new()),
        io::ErrorKind::InvalidInput => ("InvalidInput", Vec::new()),
        io::ErrorKind::InvalidData => ("InvalidData", Vec::new()),
        _ => ("Other", vec![Value::String(error.to_string())]),
    };
    Value::EnumVariant(EnumVariantValue {
        enum_name: "io.Error".to_string(),
        variant_name: variant_name.to_string(),
        payloads,
    })
}

#[cfg(test)]
#[path = "runtime_value_tests.rs"]
mod tests;
