use std::any::Any;
use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::fs::{File as StdFile, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::net::{
    Shutdown, TcpListener as StdTcpListener, TcpStream as StdTcpStream, ToSocketAddrs,
    UdpSocket as StdUdpSocket,
};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::process::{
    Child as StdChild, ChildStderr as StdChildStderr, ChildStdin as StdChildStdin,
    ChildStdout as StdChildStdout, Command as StdCommand, ExitStatus as StdExitStatus,
    Stdio as StdProcessStdio,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Once, OnceLock};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use base64::Engine as _;
use corosensei::stack::DefaultStack;
use corosensei::{Coroutine, CoroutineResult, Yielder};
use httparse::{
    Request as HttpParseRequest, Response as HttpParseResponse, Status as HttpParseStatus,
};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection};
use rustls_pemfile::{certs, private_key};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{
    accept_with_config as websocket_accept_with_config, client_tls_with_config, Message, WebSocket,
};
use url::Url;

#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};
#[cfg(unix)]
use std::os::unix::process::CommandExt;

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
    ProcessChild(ProcessChildValue),
    ProcessPipe(ProcessPipeValue),
    ProcessCompleted(ProcessCompletedValue),
    ProcessSupervisor(ProcessSupervisorValue),
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
    capacity: Option<usize>,
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
pub struct ProcessChildValue {
    inner: Arc<ProcessChildState>,
}

#[derive(Clone)]
pub struct ProcessPipeValue {
    inner: Arc<ProcessPipeState>,
}

#[derive(Clone)]
pub struct ProcessCompletedValue {
    inner: Arc<ProcessCompletedState>,
}

#[derive(Clone)]
pub struct ProcessSupervisorValue {
    inner: Arc<ProcessSupervisorState>,
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

struct ProcessChildState {
    child: Mutex<Option<StdChild>>,
    waited: Mutex<Option<StdExitStatus>>,
    process_group_id: Option<i32>,
    stdin: Option<ProcessPipeValue>,
    stdout: Option<ProcessPipeValue>,
    stderr: Option<ProcessPipeValue>,
}

enum ProcessPipeKind {
    Stdin(StdChildStdin),
    Stdout(StdChildStdout),
    Stderr(StdChildStderr),
}

struct ProcessPipeState {
    pipe: Mutex<Option<ProcessPipeKind>>,
}

struct ProcessCompletedState {
    status: Value,
    stdout: String,
    stderr: String,
}

struct ProcessSupervisorState {
    services: Mutex<BTreeMap<String, ProcessSupervisorEntry>>,
}

#[derive(Clone)]
struct ProcessSupervisorSpec {
    command: Vec<String>,
    cwd: Option<String>,
    env: Vec<(String, String)>,
    stdin: ProcessStdioConfig,
    stdout: ProcessStdioConfig,
    stderr: ProcessStdioConfig,
    restart: ProcessRestartPolicy,
    backoff: StdDuration,
    max_restarts: Option<i32>,
    group: bool,
}

struct ProcessSupervisorEntry {
    spec: ProcessSupervisorSpec,
    child: Option<ProcessChildValue>,
    restart_count: i32,
    pending_restart_status: Option<StdExitStatus>,
    next_restart_at: Option<Instant>,
}

pub(crate) enum ProcessChildWaitStatus {
    Exited(StdExitStatus),
    TimedOut,
    Cancelled,
    Failed(io::Error),
}

pub(crate) enum ProcessSupervisorWaitStatus {
    Event(Value),
    TimedOut,
    Cancelled,
}

#[derive(Clone, Copy)]
pub(crate) enum ProcessStdioConfig {
    Inherit,
    Null,
    Pipe,
}

impl ProcessStdioConfig {
    fn as_stdio(self) -> StdProcessStdio {
        match self {
            Self::Inherit => StdProcessStdio::inherit(),
            Self::Null => StdProcessStdio::null(),
            Self::Pipe => StdProcessStdio::piped(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessRestartPolicy {
    Never,
    OnFailure,
    Always,
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

#[derive(Clone, Debug)]
pub(crate) enum TaskExecutionResult {
    Ready(std::result::Result<Value, Diagnostic>),
    Cancelled,
}

enum TaskHandle {
    Running { waiters: Vec<u64> },
    Completed(TaskExecutionResult),
}

#[derive(Clone, Default)]
pub(crate) struct CancellationContext {
    flags: Vec<Arc<AtomicBool>>,
}

enum TaskYield {
    Wait(TaskWaitRegistration),
    Park,
    YieldNow,
}

#[derive(Clone)]
struct TaskWaitRegistration {
    recv_channels: Vec<ChannelValue>,
    ignore_closed_recv_channels: bool,
    send_channels: Vec<ChannelValue>,
    task_waits: Vec<TaskValue>,
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
    coroutine: Coroutine<RuntimeSchedulerWakeReason, TaskYield, TaskExecutionResult>,
}

struct LightweightTaskScheduler {
    next_task_id: u64,
    ready: VecDeque<(u64, RuntimeSchedulerWakeReason)>,
    waiting: BTreeMap<u64, TaskWaitRegistration>,
    tasks: BTreeMap<u64, LightweightTaskRecord>,
}

// Network-heavy lightweight tasks can traverse substantial library stacks
// (URL parsing, rustls handshakes, websocket framing). 256 KiB is too small
// and was causing reproducible EXC_BAD_ACCESS faults on maintained examples.
const LIGHTWEIGHT_TASK_STACK_SIZE: usize = 1024 * 1024;
const MAX_READ_ALL_BYTES: usize = 64 * 1024 * 1024;
const MAX_UDP_DATAGRAM_BYTES: usize = 65_535;
const MAX_WEBSOCKET_MESSAGE_BYTES: usize = 64 << 20;
const MAX_WEBSOCKET_FRAME_BYTES: usize = 16 << 20;
const MAX_WEBSOCKET_WRITE_BUFFER_BYTES: usize = 16 << 20;
const DEFAULT_TLS_HANDSHAKE_TIMEOUT: StdDuration = StdDuration::from_secs(10);
const MIN_SUPERVISOR_RESTART_BACKOFF: StdDuration = StdDuration::from_millis(10);

#[derive(Clone)]
struct RuntimeSchedulerRegistration {
    recv_channels: Vec<ChannelValue>,
    ignore_closed_recv_channels: bool,
    send_channels: Vec<ChannelValue>,
    task_waits: Vec<TaskValue>,
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

type BlockingIoJob = Box<dyn FnOnce() + Send + 'static>;

struct BlockingIoPool {
    queue: Mutex<VecDeque<BlockingIoJob>>,
    ready: Condvar,
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
            Value::ProcessChild(_) => "process.Child".to_string(),
            Value::ProcessPipe(_) => "process.Pipe".to_string(),
            Value::ProcessCompleted(_) => "process.Completed".to_string(),
            Value::ProcessSupervisor(_) => "process.Supervisor".to_string(),
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

impl fmt::Debug for ProcessChildValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProcessChildValue(..)")
    }
}

impl fmt::Debug for ProcessPipeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProcessPipeValue(..)")
    }
}

impl fmt::Debug for ProcessCompletedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProcessCompletedValue(..)")
    }
}

impl fmt::Debug for ProcessSupervisorValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProcessSupervisorValue(..)")
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

impl PartialEq for ProcessChildValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for ProcessPipeValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for ProcessCompletedValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PartialEq for ProcessSupervisorValue {
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
        recv_channels: Vec<ChannelValue>,
        ignore_closed_recv_channels: bool,
        send_channels: Vec<ChannelValue>,
        task_waits: Vec<TaskValue>,
        deadline: Option<Instant>,
        cancellation: Option<CancellationContext>,
    ) -> RuntimeSchedulerHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let waiter = Arc::new(RuntimeSchedulerWaiter {
            state: Mutex::new(None),
            ready: Condvar::new(),
        });
        let registration = RuntimeSchedulerRegistration {
            recv_channels,
            ignore_closed_recv_channels,
            send_channels,
            task_waits,
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

                if registration.recv_channels.iter().any(|channel| {
                    channel.is_ready_for_scheduler_recv(registration.ignore_closed_recv_channels)
                }) || registration
                    .send_channels
                    .iter()
                    .any(ChannelValue::is_ready_for_scheduler_send)
                    || registration
                        .task_waits
                        .iter()
                        .any(|task| task.completed_result().is_some())
                {
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

impl BlockingIoPool {
    fn start() -> Arc<Self> {
        let pool = Arc::new(Self {
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
        });
        let worker_count = thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(4)
            .clamp(2, 8);
        for _ in 0..worker_count {
            let worker = pool.clone();
            thread::spawn(move || worker.run());
        }
        pool
    }

    fn run(self: Arc<Self>) {
        loop {
            let job = {
                let mut queue = lock_mutex(&self.queue);
                loop {
                    if let Some(job) = queue.pop_front() {
                        break job;
                    }
                    queue = wait_condvar(&self.ready, queue);
                }
            };
            job();
        }
    }

    fn submit(&self, job: BlockingIoJob) {
        let mut queue = lock_mutex(&self.queue);
        queue.push_back(job);
        drop(queue);
        self.ready.notify_one();
    }
}

fn blocking_io_pool() -> &'static Arc<BlockingIoPool> {
    static POOL: OnceLock<Arc<BlockingIoPool>> = OnceLock::new();
    POOL.get_or_init(BlockingIoPool::start)
}

pub(crate) fn wait_for_runtime_scheduler(
    recv_channels: Vec<ChannelValue>,
    ignore_closed_recv_channels: bool,
    send_channels: Vec<ChannelValue>,
    task_waits: Vec<TaskValue>,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> RuntimeSchedulerWakeReason {
    if cancellation.is_some_and(CancellationContext::is_cancelled) {
        return RuntimeSchedulerWakeReason::Cancelled;
    }
    if recv_channels
        .iter()
        .any(|channel| channel.is_ready_for_scheduler_recv(ignore_closed_recv_channels))
        || send_channels
            .iter()
            .any(ChannelValue::is_ready_for_scheduler_send)
        || task_waits
            .iter()
            .any(|task| task.completed_result().is_some())
    {
        return RuntimeSchedulerWakeReason::Ready;
    }
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return RuntimeSchedulerWakeReason::TimedOut;
    }

    if let Some(reason) = yield_current_lightweight_wait(TaskWaitRegistration {
        recv_channels: recv_channels.clone(),
        ignore_closed_recv_channels,
        send_channels: send_channels.clone(),
        task_waits: task_waits.clone(),
        deadline,
        cancellation: cancellation.cloned(),
        fd_wait: None,
    }) {
        return reason;
    }

    runtime_scheduler()
        .register(
            recv_channels,
            ignore_closed_recv_channels,
            send_channels,
            task_waits,
            deadline,
            cancellation.cloned(),
        )
        .wait()
}

pub(crate) fn sleep_with_runtime_scheduler(
    duration: StdDuration,
    cancellation: Option<&CancellationContext>,
) -> RuntimeSchedulerWakeReason {
    let deadline = Instant::now().checked_add(duration);
    wait_for_runtime_scheduler(
        Vec::new(),
        false,
        Vec::new(),
        Vec::new(),
        deadline,
        cancellation,
    )
}

thread_local! {
    static CURRENT_LIGHTWEIGHT_TASK_CONTEXT: Cell<*const LightweightTaskContext> =
        const { Cell::new(std::ptr::null()) };
    static CURRENT_LIGHTWEIGHT_TASK_CANCELLATION: std::cell::RefCell<Option<CancellationContext>> =
        const { std::cell::RefCell::new(None) };
}

static LIGHTWEIGHT_TASK_PANIC_HOOK: Once = Once::new();

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

fn with_current_lightweight_task_context<T>(
    f: impl FnOnce(&LightweightTaskContext) -> T,
) -> Option<T> {
    CURRENT_LIGHTWEIGHT_TASK_CONTEXT.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            None
        } else {
            Some(f(unsafe { &*ptr }))
        }
    })
}

pub(crate) fn current_lightweight_task_cancellation() -> Option<CancellationContext> {
    CURRENT_LIGHTWEIGHT_TASK_CANCELLATION.with(|slot| slot.borrow().clone())
}

#[derive(Debug)]
struct TaskCancelledSignal;

pub(crate) fn cancel_current_lightweight_task() -> ! {
    panic::panic_any(TaskCancelledSignal);
}

fn task_panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else {
        "task panicked with a non-string payload".to_string()
    }
}

fn install_lightweight_task_panic_hook() {
    LIGHTWEIGHT_TASK_PANIC_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if info.payload().is::<TaskCancelledSignal>() {
                return;
            }
            previous(info);
        }));
    });
}

fn finalize_task_execution<F>(entry: F) -> TaskExecutionResult
where
    F: FnOnce() -> std::result::Result<Value, Diagnostic>,
{
    match panic::catch_unwind(AssertUnwindSafe(entry)) {
        Ok(result) => TaskExecutionResult::Ready(result),
        Err(payload) if payload.is::<TaskCancelledSignal>() => TaskExecutionResult::Cancelled,
        Err(payload) => TaskExecutionResult::Ready(Err(Diagnostic::new(format!(
            "internal error: Aurora task panicked: {}",
            task_panic_message(&*payload)
        )))),
    }
}

pub(crate) fn run_blocking_io<T, F>(
    operation: F,
    cancellation: Option<&CancellationContext>,
) -> io::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> io::Result<T> + Send + 'static,
{
    if with_current_lightweight_task_context(|_| ()).is_none() {
        return operation();
    }
    if cancellation.is_some_and(CancellationContext::is_cancelled) {
        return Err(cancelled_resource_error());
    }

    let completion = ChannelValue::new();
    let result = Arc::new(Mutex::new(None::<io::Result<T>>));
    let result_slot = result.clone();
    let completion_signal = completion.clone();
    blocking_io_pool().submit(Box::new(move || {
        let outcome = operation();
        *lock_mutex(&result_slot) = Some(outcome);
        let _ = completion_signal.send(Value::Unit);
        completion_signal.close();
    }));

    match completion.recv_with_cancellation(None, cancellation) {
        Some(_) => lock_mutex(&result).take().unwrap_or_else(|| {
            Err(io::Error::other(
                "blocking I/O task completed without returning a result",
            ))
        }),
        None if cancellation.is_some_and(CancellationContext::is_cancelled) => {
            Err(cancelled_resource_error())
        }
        None => lock_mutex(&result).take().unwrap_or_else(|| {
            Err(io::Error::other(
                "blocking I/O wait ended before the task completed",
            ))
        }),
    }
}

fn yield_current_lightweight_task(wait: TaskYield) -> Option<RuntimeSchedulerWakeReason> {
    with_current_lightweight_task_context(|context| {
        let yielder_ptr = context.yielder.get();
        if yielder_ptr.is_null() {
            None
        } else {
            let yielder = unsafe { &*yielder_ptr };
            Some(yielder.suspend(wait))
        }
    })
    .flatten()
}

fn yield_current_lightweight_wait(
    wait: TaskWaitRegistration,
) -> Option<RuntimeSchedulerWakeReason> {
    yield_current_lightweight_task(TaskYield::Wait(wait))
}

fn park_current_lightweight_task() -> Option<RuntimeSchedulerWakeReason> {
    yield_current_lightweight_task(TaskYield::Park)
}

fn yield_now_current_lightweight_task() -> Option<RuntimeSchedulerWakeReason> {
    yield_current_lightweight_task(TaskYield::YieldNow)
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
            .recv_channels
            .iter()
            .any(|channel| channel.is_ready_for_scheduler_recv(self.ignore_closed_recv_channels))
            || self
                .send_channels
                .iter()
                .any(ChannelValue::is_ready_for_scheduler_send)
            || self
                .task_waits
                .iter()
                .any(|task| task.completed_result().is_some())
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
        F: FnOnce() -> std::result::Result<Value, Diagnostic> + 'static,
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
            finalize_task_execution(entry)
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

    fn complete_task(
        &mut self,
        task_id: u64,
        task_state: &Arc<TaskState>,
        result: TaskExecutionResult,
    ) {
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
        runtime_scheduler().notify();
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
            CoroutineResult::Yield(TaskYield::YieldNow) => {
                self.tasks.insert(task_id, record);
                self.ready
                    .push_back((task_id, RuntimeSchedulerWakeReason::Ready));
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

        let poll_slice = StdDuration::from_millis(1);
        let next_deadline = self.waiting.values().filter_map(|wait| wait.deadline).min();
        let has_non_fd_waiters = self.waiting.values().any(|wait| wait.fd_wait.is_none());
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
            let wait_duration = next_deadline
                .map(|deadline| {
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(poll_slice)
                })
                .unwrap_or(poll_slice);
            // Channel readiness, task completion, and blocking-I/O completions can change without
            // any fd becoming readable. Poll those states on a short slice instead of sleeping
            // until the full deadline, otherwise completions remain invisible until timeout.
            thread::park_timeout(wait_duration);
            self.promote_ready_waiters(None);
            return;
        }

        let timeout_ms = match next_deadline {
            Some(deadline) => {
                let now = Instant::now();
                if deadline <= now {
                    0
                } else {
                    let duration = deadline.saturating_duration_since(now);
                    let duration = if has_non_fd_waiters {
                        duration.min(poll_slice)
                    } else {
                        duration
                    };
                    duration_to_poll_timeout(duration)
                }
            }
            None if has_non_fd_waiters => duration_to_poll_timeout(poll_slice),
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

    fn run_until_root(&mut self, root: &TaskValue) -> std::result::Result<Value, Diagnostic> {
        loop {
            if let Some(result) = root.completed_result() {
                return match result {
                    TaskExecutionResult::Ready(result) => result,
                    TaskExecutionResult::Cancelled => {
                        Err(Diagnostic::new("root Aurora task was cancelled"))
                    }
                };
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
    F: FnOnce() -> std::result::Result<Value, Diagnostic> + 'static,
{
    let Some(scheduler) = with_current_lightweight_task_context(|context| context.scheduler) else {
        return Err(Diagnostic::new(
            "lightweight Aurora task start requires an active task scheduler",
        ));
    };
    let scheduler = unsafe { &mut *scheduler };
    scheduler.spawn_task(None, entry)
}

pub(crate) fn spawn_lightweight_task_with_cancellation<F>(
    cancellation: CancellationContext,
    entry: F,
) -> std::result::Result<TaskValue, Diagnostic>
where
    F: FnOnce() -> std::result::Result<Value, Diagnostic> + 'static,
{
    let Some(scheduler) = with_current_lightweight_task_context(|context| context.scheduler) else {
        return Err(Diagnostic::new(
            "lightweight Aurora task start requires an active task scheduler",
        ));
    };
    let scheduler = unsafe { &mut *scheduler };
    scheduler.spawn_task(Some(cancellation), entry)
}

pub(crate) fn run_lightweight_root_task<F>(entry: F) -> std::result::Result<Value, Diagnostic>
where
    F: FnOnce() -> std::result::Result<Value, Diagnostic> + 'static,
{
    install_lightweight_task_panic_hook();
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
            (Value::ProcessChild(left), Value::ProcessChild(right)) => left == right,
            (Value::ProcessPipe(left), Value::ProcessPipe(right)) => left == right,
            (Value::ProcessCompleted(left), Value::ProcessCompleted(right)) => left == right,
            (Value::ProcessSupervisor(left), Value::ProcessSupervisor(right)) => left == right,
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
            Value::ProcessChild(_) => "<process-child>".to_string(),
            Value::ProcessPipe(_) => "<process-pipe>".to_string(),
            Value::ProcessCompleted(completed) => {
                format!("<process-completed {}>", completed.status().render())
            }
            Value::ProcessSupervisor(_) => "<process-supervisor>".to_string(),
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
    fn has_pending_values(&self) -> bool {
        !lock_mutex(&self.inner.state).queue.is_empty()
    }

    pub(crate) fn new() -> Self {
        Self::with_optional_capacity(None)
    }

    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self::with_optional_capacity(Some(capacity))
    }

    fn with_optional_capacity(capacity: Option<usize>) -> Self {
        Self {
            inner: Arc::new(ChannelState {
                state: Mutex::new(ChannelInner {
                    queue: VecDeque::new(),
                    closed: false,
                    capacity,
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TrySendResult {
    Sent,
    Closed(Value),
    Full(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SendValueError {
    Closed(Value),
    Cancelled(Value),
    TimedOut(Value),
    Full(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RecvValueResult {
    Value(Value),
    Closed,
    TimedOut,
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) enum TaskWaitStatus {
    Ready(std::result::Result<Value, Diagnostic>),
    TimedOut,
    Cancelled,
}

impl ChannelValue {
    fn is_ready_for_scheduler_recv(&self, ignore_closed: bool) -> bool {
        let state = lock_mutex(&self.inner.state);
        !state.queue.is_empty() || (!ignore_closed && state.closed)
    }

    pub(crate) fn is_ready_for_scheduler_send(&self) -> bool {
        let state = lock_mutex(&self.inner.state);
        state.closed
            || state
                .capacity
                .is_none_or(|capacity| state.queue.len() < capacity)
    }

    pub(crate) fn try_recv(&self) -> TryRecvResult {
        let mut state = lock_mutex(&self.inner.state);
        if let Some(value) = state.queue.pop_front() {
            drop(state);
            runtime_scheduler().notify();
            return TryRecvResult::Value(value);
        }
        if state.closed {
            return TryRecvResult::Closed;
        }
        TryRecvResult::Empty
    }

    pub(crate) fn try_send(&self, value: Value) -> TrySendResult {
        let mut state = lock_mutex(&self.inner.state);
        if state.closed {
            return TrySendResult::Closed(value);
        }
        if state
            .capacity
            .is_some_and(|capacity| state.queue.len() >= capacity)
        {
            return TrySendResult::Full(value);
        }
        state.queue.push_back(value);
        drop(state);
        runtime_scheduler().notify();
        TrySendResult::Sent
    }

    pub(crate) fn send(&self, value: Value) -> std::result::Result<(), Value> {
        match self.send_with_cancellation(value, None) {
            Ok(()) => Ok(()),
            Err(SendValueError::Closed(value))
            | Err(SendValueError::Cancelled(value))
            | Err(SendValueError::TimedOut(value))
            | Err(SendValueError::Full(value)) => Err(value),
        }
    }

    pub(crate) fn send_with_cancellation(
        &self,
        value: Value,
        cancellation: Option<&CancellationContext>,
    ) -> std::result::Result<(), SendValueError> {
        self.send_with_deadline(value, None, cancellation, false)
    }

    pub(crate) fn try_send_result(&self, value: Value) -> std::result::Result<(), SendValueError> {
        match self.try_send(value) {
            TrySendResult::Sent => Ok(()),
            TrySendResult::Closed(value) => Err(SendValueError::Closed(value)),
            TrySendResult::Full(value) => Err(SendValueError::Full(value)),
        }
    }

    pub(crate) fn send_with_timeout(
        &self,
        value: Value,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> std::result::Result<(), SendValueError> {
        self.send_with_deadline(value, deadline_from_timeout(timeout), cancellation, false)
    }

    fn send_with_deadline(
        &self,
        mut value: Value,
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
        fail_on_full: bool,
    ) -> std::result::Result<(), SendValueError> {
        loop {
            value = match self.try_send(value) {
                TrySendResult::Sent => return Ok(()),
                TrySendResult::Closed(value) => return Err(SendValueError::Closed(value)),
                TrySendResult::Full(value) if fail_on_full => {
                    return Err(SendValueError::Full(value));
                }
                TrySendResult::Full(value) => value,
            };

            match wait_for_runtime_scheduler(
                Vec::new(),
                false,
                vec![self.clone()],
                Vec::new(),
                deadline,
                cancellation,
            ) {
                RuntimeSchedulerWakeReason::Ready => {}
                RuntimeSchedulerWakeReason::TimedOut => {
                    return Err(SendValueError::TimedOut(value));
                }
                RuntimeSchedulerWakeReason::Cancelled => {
                    return Err(SendValueError::Cancelled(value));
                }
            }
        }
    }

    pub(crate) fn recv_with_cancellation(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> Option<Value> {
        match self.recv_result_with_cancellation(timeout, cancellation) {
            RecvValueResult::Value(value) => Some(value),
            RecvValueResult::Closed | RecvValueResult::TimedOut | RecvValueResult::Cancelled => {
                None
            }
        }
    }

    pub(crate) fn recv_result_with_cancellation(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> RecvValueResult {
        let deadline = deadline_from_timeout(timeout);
        loop {
            match self.try_recv() {
                TryRecvResult::Value(value) => {
                    if self.has_pending_values() {
                        let _ = yield_now_current_lightweight_task();
                    }
                    return RecvValueResult::Value(value);
                }
                TryRecvResult::Closed => return RecvValueResult::Closed,
                TryRecvResult::Empty => {}
            }

            match wait_for_runtime_scheduler(
                vec![self.clone()],
                false,
                Vec::new(),
                Vec::new(),
                deadline,
                cancellation,
            ) {
                RuntimeSchedulerWakeReason::Ready => {}
                RuntimeSchedulerWakeReason::TimedOut => return RecvValueResult::TimedOut,
                RuntimeSchedulerWakeReason::Cancelled => return RecvValueResult::Cancelled,
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

fn validate_udp_datagram_limit(max_bytes: usize) -> io::Result<usize> {
    if max_bytes > MAX_UDP_DATAGRAM_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "UDP reads are limited to {} bytes per datagram",
                MAX_UDP_DATAGRAM_BYTES
            ),
        ));
    }
    Ok(max_bytes.max(1))
}

fn normalize_udp_send_error(error: io::Error) -> io::Error {
    #[cfg(unix)]
    if error.raw_os_error() == Some(libc::EMSGSIZE) {
        return io::Error::new(
            io::ErrorKind::InvalidInput,
            "UDP datagram exceeds the platform send limit",
        );
    }
    error
}

pub(crate) fn create_dir_once(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::create_dir(path)
}

pub(crate) fn remove_file_checked(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                "path is a directory",
            ));
        }
        Ok(_) => {}
        Err(error) => return Err(error),
    }
    std::fs::remove_file(path)
}

fn deadline_from_timeout(timeout: Option<StdDuration>) -> Option<Instant> {
    timeout.and_then(|duration| Instant::now().checked_add(duration))
}

fn check_deadline_and_cancellation(
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<()> {
    if cancellation.is_some_and(CancellationContext::is_cancelled) {
        return Err(cancelled_resource_error());
    }
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(timeout_resource_error());
    }
    Ok(())
}

fn tls_handshake_deadline(deadline: Option<Instant>) -> Option<Instant> {
    let cap = Instant::now().checked_add(DEFAULT_TLS_HANDSHAKE_TIMEOUT);
    match (deadline, cap) {
        (Some(deadline), Some(cap)) => Some(std::cmp::min(deadline, cap)),
        (Some(deadline), None) => Some(deadline),
        (None, cap) => cap,
    }
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

#[cfg(unix)]
fn set_fd_nonblocking(fd: libc::c_int, enabled: bool) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut updated = flags;
    if enabled {
        updated |= libc::O_NONBLOCK;
    } else {
        updated &= !libc::O_NONBLOCK;
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, updated) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn set_process_pipe_nonblocking<P: AsRawFd>(pipe: &P, enabled: bool) -> io::Result<()> {
    set_fd_nonblocking(pipe.as_raw_fd(), enabled)
}

fn timeout_deadline(timeout: Option<StdDuration>) -> Option<Instant> {
    timeout.and_then(|timeout| Instant::now().checked_add(timeout))
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
        recv_channels: Vec::new(),
        ignore_closed_recv_channels: false,
        send_channels: Vec::new(),
        task_waits: Vec::new(),
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

pub(crate) fn read_all_limit_error(label: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "{} exceeded the supported read_all limit of {} bytes",
            label, MAX_READ_ALL_BYTES
        ),
    )
}

fn push_limited_bytes(contents: &mut Vec<u8>, chunk: &[u8], label: &str) -> io::Result<()> {
    if contents.len().saturating_add(chunk.len()) > MAX_READ_ALL_BYTES {
        return Err(read_all_limit_error(label));
    }
    contents.extend_from_slice(chunk);
    Ok(())
}

pub(crate) fn read_all_from_reader<R: Read>(reader: &mut R, label: &str) -> io::Result<Vec<u8>> {
    let mut limited = reader.take((MAX_READ_ALL_BYTES as u64) + 1);
    let mut contents = Vec::new();
    limited.read_to_end(&mut contents)?;
    if contents.len() > MAX_READ_ALL_BYTES {
        return Err(read_all_limit_error(label));
    }
    Ok(contents)
}

pub(crate) fn read_file_limited(path: &str, label: &str) -> io::Result<Vec<u8>> {
    let mut file = StdFile::open(path)?;
    read_all_from_reader(&mut file, label)
}

fn validate_http_header_name(name: &str) -> io::Result<()> {
    fn is_tchar(byte: u8) -> bool {
        matches!(
            byte,
            b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_'
                | b'`' | b'|' | b'~' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
        )
    }

    if name.is_empty() || name.bytes().any(|byte| !is_tchar(byte)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid HTTP header name `{}`", name),
        ));
    }
    Ok(())
}

fn validate_http_header_value(value: &str) -> io::Result<()> {
    if value.bytes().any(|byte| {
        matches!(byte, b'\r' | b'\n')
            || (byte < 0x20 && byte != b'\t')
            || byte == 0x7f
            || byte >= 0x80
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTP header values may not contain control characters",
        ));
    }
    Ok(())
}

fn validate_http_headers(headers: &[(String, String)]) -> io::Result<()> {
    for (name, value) in headers {
        validate_http_header_name(name)?;
        validate_http_header_value(value)?;
    }
    Ok(())
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
            Ok(bytes) => push_limited_bytes(&mut contents, &chunk[..bytes], "network read_all")?,
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

fn read_pem_file_limited(
    path: &str,
    label: &str,
) -> io::Result<io::BufReader<io::Cursor<Vec<u8>>>> {
    Ok(io::BufReader::new(io::Cursor::new(read_file_limited(
        path, label,
    )?)))
}

fn load_tls_server_config(
    cert_pem_path: &str,
    key_pem_path: &str,
) -> io::Result<Arc<ServerConfig>> {
    ensure_rustls_crypto_provider();
    let mut cert_reader = read_pem_file_limited(cert_pem_path, "TLS certificate PEM")?;
    let cert_chain = certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<CertificateDer<'static>>, _>>()?;
    let mut key_reader = read_pem_file_limited(key_pem_path, "TLS private key PEM")?;
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
        let mut reader = read_pem_file_limited(ca_pem_path, "TLS CA PEM")?;
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
const HTTP_MESSAGE_TOO_LARGE_PREFIX: &str = "HTTP message exceeds the supported size limit";
const HTTP_HEADERS_TOO_LARGE_PREFIX: &str = "HTTP request exceeded the supported header count";

fn http_message_too_large_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "{} of {} bytes",
            HTTP_MESSAGE_TOO_LARGE_PREFIX, MAX_HTTP_MESSAGE_BYTES
        ),
    )
}

fn is_http_message_too_large_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::InvalidData
        && error.to_string().starts_with(HTTP_MESSAGE_TOO_LARGE_PREFIX)
}

fn http_headers_too_large_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "{} of {} headers",
            HTTP_HEADERS_TOO_LARGE_PREFIX, MAX_HTTP_HEADERS
        ),
    )
}

fn is_http_headers_too_large_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::InvalidData
        && error.to_string().starts_with(HTTP_HEADERS_TOO_LARGE_PREFIX)
}

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
        431 => "Request Header Fields Too Large",
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
        return Err(http_message_too_large_error());
    }
    buffer.extend_from_slice(chunk);
    Ok(())
}

fn parse_http_request_head(
    buffer: &[u8],
) -> io::Result<Option<(usize, String, String, Vec<(String, String)>, usize)>> {
    let mut raw_headers = [httparse::EMPTY_HEADER; MAX_HTTP_HEADERS];
    let mut request = HttpParseRequest::new(&mut raw_headers);
    match request.parse(buffer).map_err(|error| match error {
        httparse::Error::TooManyHeaders => http_headers_too_large_error(),
        other => io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid HTTP request: {}", other),
        ),
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

    if header_len.saturating_add(content_length) > MAX_HTTP_MESSAGE_BYTES {
        return Err(http_message_too_large_error());
    }

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
) -> io::Result<Vec<u8>> {
    validate_http_headers(&headers)?;
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
    Ok(rendered)
}

fn write_http_response_to_stream(
    stream: &mut StdTcpStream,
    status: i32,
    headers: Vec<(String, String)>,
    body: &[u8],
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<()> {
    validate_http_headers(&headers)?;
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
fn unsupported_websocket_transport_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported websocket transport",
    )
}

#[cfg(unix)]
fn websocket_raw_fd(socket: &WebSocketStateKind) -> io::Result<i32> {
    match socket {
        WebSocketStateKind::Plain(socket) => Ok(socket.get_ref().as_raw_fd()),
        WebSocketStateKind::MaybeTls(socket) => maybe_tls_stream_raw_fd(socket.get_ref()),
    }
}

#[cfg(unix)]
fn maybe_tls_stream_raw_fd(stream: &MaybeTlsStream<StdTcpStream>) -> io::Result<i32> {
    match stream {
        MaybeTlsStream::Plain(stream) => Ok(stream.as_raw_fd()),
        MaybeTlsStream::Rustls(stream) => Ok(stream.get_ref().as_raw_fd()),
        _ => Err(unsupported_websocket_transport_error()),
    }
}

fn websocket_error_to_io(error: tungstenite::Error) -> io::Error {
    match error {
        tungstenite::Error::Io(error) => error,
        other => io::Error::other(other),
    }
}

#[cfg(unix)]
trait WebSocketHandshakeStream {
    fn raw_fd(&self) -> io::Result<i32>;
}

#[cfg(unix)]
impl WebSocketHandshakeStream for StdTcpStream {
    fn raw_fd(&self) -> io::Result<i32> {
        Ok(self.as_raw_fd())
    }
}

#[cfg(unix)]
impl WebSocketHandshakeStream for MaybeTlsStream<StdTcpStream> {
    fn raw_fd(&self) -> io::Result<i32> {
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
                    next_mid.get_ref().get_ref().raw_fd()?,
                    libc::POLLIN | libc::POLLOUT,
                    deadline,
                    None,
                )?;
                mid = next_mid;
            }
            Err(tungstenite::handshake::HandshakeError::Failure(error)) => {
                return Err(websocket_error_to_io(error));
            }
        }
    }
}

#[cfg(unix)]
fn accept_websocket_stream(
    stream: StdTcpStream,
    deadline: Option<Instant>,
) -> io::Result<WebSocketStateKind> {
    stream.set_nonblocking(true)?;
    let socket = match websocket_accept_with_config(stream, Some(websocket_config())) {
        Ok(socket) => socket,
        Err(tungstenite::handshake::HandshakeError::Interrupted(mid)) => {
            finish_websocket_handshake(mid, deadline)?
        }
        Err(tungstenite::handshake::HandshakeError::Failure(error)) => {
            return Err(websocket_error_to_io(error));
        }
    };
    Ok(WebSocketStateKind::Plain(socket))
}

#[cfg(unix)]
fn connect_websocket_stream(
    stream: StdTcpStream,
    request: tungstenite::http::Request<()>,
    deadline: Option<Instant>,
) -> io::Result<WebSocketStateKind> {
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let (socket, _) = match client_tls_with_config(request, stream, Some(websocket_config()), None)
    {
        Ok(result) => result,
        Err(tungstenite::handshake::HandshakeError::Interrupted(mid)) => {
            finish_websocket_handshake(mid, deadline)?
        }
        Err(tungstenite::handshake::HandshakeError::Failure(error)) => {
            return Err(websocket_error_to_io(error));
        }
    };
    Ok(WebSocketStateKind::MaybeTls(socket))
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

fn websocket_config() -> WebSocketConfig {
    #[allow(deprecated)]
    WebSocketConfig {
        max_send_queue: None,
        write_buffer_size: 128 * 1024,
        max_write_buffer_size: MAX_WEBSOCKET_WRITE_BUFFER_BYTES,
        max_message_size: Some(MAX_WEBSOCKET_MESSAGE_BYTES),
        max_frame_size: Some(MAX_WEBSOCKET_FRAME_BYTES),
        accept_unmasked_frames: false,
    }
}

fn websocket_client_key() -> io::Result<String> {
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).map_err(io::Error::other)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(nonce))
}

fn websocket_host_header(parsed: &Url) -> io::Result<String> {
    let host = parsed.host_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "websocket URL is missing a host",
        )
    })?;
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    Ok(match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

fn websocket_client_request(parsed: &Url) -> io::Result<tungstenite::http::Request<()>> {
    tungstenite::http::Request::builder()
        .method("GET")
        .uri(parsed.as_str())
        .header("Host", websocket_host_header(parsed)?)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", websocket_client_key()?)
        .body(())
        .map_err(io::Error::other)
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
                        websocket_raw_fd(socket)?,
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
            Err(error) => return Err(websocket_error_to_io(error)),
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
        let path = path.to_string();
        run_blocking_io(
            move || StdFile::open(path).map(Self::from_std),
            current_lightweight_task_cancellation().as_ref(),
        )
    }

    pub(crate) fn create(path: &str) -> io::Result<Self> {
        let path = path.to_string();
        run_blocking_io(
            move || StdFile::create(path).map(Self::from_std),
            current_lightweight_task_cancellation().as_ref(),
        )
    }

    pub(crate) fn append(path: &str) -> io::Result<Self> {
        let path = path.to_string();
        run_blocking_io(
            move || {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map(Self::from_std)
            },
            current_lightweight_task_cancellation().as_ref(),
        )
    }

    pub(crate) fn read_all(&self) -> io::Result<String> {
        let state = self.inner.clone();
        run_blocking_io(
            move || {
                let mut file = lock_mutex(&state.file);
                let Some(file) = file.as_mut() else {
                    return Err(closed_resource_error());
                };
                io_decode_utf8(&read_all_from_reader(file, "file read_all")?)
            },
            current_lightweight_task_cancellation().as_ref(),
        )
    }

    pub(crate) fn read_bytes(&self) -> io::Result<Vec<u8>> {
        let state = self.inner.clone();
        run_blocking_io(
            move || {
                let mut file = lock_mutex(&state.file);
                let Some(file) = file.as_mut() else {
                    return Err(closed_resource_error());
                };
                read_all_from_reader(file, "file read_bytes")
            },
            current_lightweight_task_cancellation().as_ref(),
        )
    }

    pub(crate) fn write_all(&self, text: &str) -> io::Result<()> {
        self.write_bytes(text.as_bytes())
    }

    pub(crate) fn write_bytes(&self, bytes: &[u8]) -> io::Result<()> {
        let state = self.inner.clone();
        let bytes = bytes.to_vec();
        run_blocking_io(
            move || {
                let mut file = lock_mutex(&state.file);
                let Some(file) = file.as_mut() else {
                    return Err(closed_resource_error());
                };
                file.write_all(&bytes)
            },
            current_lightweight_task_cancellation().as_ref(),
        )
    }

    pub(crate) fn flush(&self) -> io::Result<()> {
        let state = self.inner.clone();
        run_blocking_io(
            move || {
                let mut file = lock_mutex(&state.file);
                let Some(file) = file.as_mut() else {
                    return Err(closed_resource_error());
                };
                file.flush()
            },
            current_lightweight_task_cancellation().as_ref(),
        )
    }

    pub(crate) fn close(&self) {
        let mut file = lock_mutex(&self.inner.file);
        *file = None;
    }
}

impl ProcessPipeValue {
    #[cfg(unix)]
    fn from_stdin(stdin: StdChildStdin) -> io::Result<Self> {
        set_process_pipe_nonblocking(&stdin, true)?;
        Ok(Self {
            inner: Arc::new(ProcessPipeState {
                pipe: Mutex::new(Some(ProcessPipeKind::Stdin(stdin))),
            }),
        })
    }

    #[cfg(not(unix))]
    fn from_stdin(stdin: StdChildStdin) -> io::Result<Self> {
        Ok(Self {
            inner: Arc::new(ProcessPipeState {
                pipe: Mutex::new(Some(ProcessPipeKind::Stdin(stdin))),
            }),
        })
    }

    #[cfg(unix)]
    fn from_stdout(stdout: StdChildStdout) -> io::Result<Self> {
        set_process_pipe_nonblocking(&stdout, true)?;
        Ok(Self {
            inner: Arc::new(ProcessPipeState {
                pipe: Mutex::new(Some(ProcessPipeKind::Stdout(stdout))),
            }),
        })
    }

    #[cfg(not(unix))]
    fn from_stdout(stdout: StdChildStdout) -> io::Result<Self> {
        Ok(Self {
            inner: Arc::new(ProcessPipeState {
                pipe: Mutex::new(Some(ProcessPipeKind::Stdout(stdout))),
            }),
        })
    }

    #[cfg(unix)]
    fn from_stderr(stderr: StdChildStderr) -> io::Result<Self> {
        set_process_pipe_nonblocking(&stderr, true)?;
        Ok(Self {
            inner: Arc::new(ProcessPipeState {
                pipe: Mutex::new(Some(ProcessPipeKind::Stderr(stderr))),
            }),
        })
    }

    #[cfg(not(unix))]
    fn from_stderr(stderr: StdChildStderr) -> io::Result<Self> {
        Ok(Self {
            inner: Arc::new(ProcessPipeState {
                pipe: Mutex::new(Some(ProcessPipeKind::Stderr(stderr))),
            }),
        })
    }

    pub(crate) fn read_all(
        &self,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<String> {
        let mut pipe = lock_mutex(&self.inner.pipe);
        let Some(pipe) = pipe.as_mut() else {
            return Err(closed_resource_error());
        };
        #[cfg(unix)]
        let bytes = match pipe {
            ProcessPipeKind::Stdout(stdout) => read_all_with_deadline(stdout, None, cancellation)?,
            ProcessPipeKind::Stderr(stderr) => read_all_with_deadline(stderr, None, cancellation)?,
            ProcessPipeKind::Stdin(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot read from a process stdin pipe",
                ))
            }
        };
        #[cfg(not(unix))]
        let bytes = match pipe {
            ProcessPipeKind::Stdout(stdout) => {
                read_all_from_reader(stdout, "process pipe read_all")?
            }
            ProcessPipeKind::Stderr(stderr) => {
                read_all_from_reader(stderr, "process pipe read_all")?
            }
            ProcessPipeKind::Stdin(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot read from a process stdin pipe",
                ))
            }
        };
        io_decode_utf8(&bytes)
    }

    pub(crate) fn read_line(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<String>> {
        let deadline = timeout_deadline(timeout);
        let mut pipe = lock_mutex(&self.inner.pipe);
        let Some(pipe) = pipe.as_mut() else {
            return Err(closed_resource_error());
        };
        #[cfg(unix)]
        {
            match pipe {
                ProcessPipeKind::Stdout(stdout) => {
                    read_line_with_deadline(stdout, deadline, cancellation)
                }
                ProcessPipeKind::Stderr(stderr) => {
                    read_line_with_deadline(stderr, deadline, cancellation)
                }
                ProcessPipeKind::Stdin(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot read from a process stdin pipe",
                )),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = deadline;
            let _ = cancellation;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process pipe line reads are only supported on unix in the bootstrap runtime",
            ))
        }
    }

    pub(crate) fn read_bytes(
        &self,
        max_bytes: usize,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<Vec<u8>>> {
        let deadline = timeout_deadline(timeout);
        let mut pipe = lock_mutex(&self.inner.pipe);
        let Some(pipe) = pipe.as_mut() else {
            return Err(closed_resource_error());
        };
        #[cfg(unix)]
        {
            match pipe {
                ProcessPipeKind::Stdout(stdout) => {
                    read_some_with_deadline(stdout, max_bytes, deadline, cancellation)
                }
                ProcessPipeKind::Stderr(stderr) => {
                    read_some_with_deadline(stderr, max_bytes, deadline, cancellation)
                }
                ProcessPipeKind::Stdin(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot read from a process stdin pipe",
                )),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = max_bytes;
            let _ = deadline;
            let _ = cancellation;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process pipe byte reads are only supported on unix in the bootstrap runtime",
            ))
        }
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
        let deadline = timeout_deadline(timeout);
        let mut pipe = lock_mutex(&self.inner.pipe);
        let Some(pipe) = pipe.as_mut() else {
            return Err(closed_resource_error());
        };
        #[cfg(unix)]
        {
            match pipe {
                ProcessPipeKind::Stdin(stdin) => {
                    write_all_with_deadline(stdin, bytes, deadline, cancellation)
                }
                ProcessPipeKind::Stdout(_) | ProcessPipeKind::Stderr(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot write to a process output pipe",
                )),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = bytes;
            let _ = deadline;
            let _ = cancellation;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process pipe writes are only supported on unix in the bootstrap runtime",
            ))
        }
    }

    pub(crate) fn flush(&self) -> io::Result<()> {
        let mut pipe = lock_mutex(&self.inner.pipe);
        let Some(pipe) = pipe.as_mut() else {
            return Err(closed_resource_error());
        };
        match pipe {
            ProcessPipeKind::Stdin(stdin) => stdin.flush(),
            ProcessPipeKind::Stdout(_) | ProcessPipeKind::Stderr(_) => Ok(()),
        }
    }

    pub(crate) fn close(&self) {
        let mut pipe = lock_mutex(&self.inner.pipe);
        *pipe = None;
    }
}

impl ProcessCompletedValue {
    pub(crate) fn new(status: Value, stdout: String, stderr: String) -> Self {
        Self {
            inner: Arc::new(ProcessCompletedState {
                status,
                stdout,
                stderr,
            }),
        }
    }

    pub(crate) fn status(&self) -> Value {
        self.inner.status.clone()
    }

    pub(crate) fn stdout(&self) -> String {
        self.inner.stdout.clone()
    }

    pub(crate) fn stderr(&self) -> String {
        self.inner.stderr.clone()
    }

    pub(crate) fn success(&self) -> bool {
        matches!(
            &self.inner.status,
            Value::EnumVariant(EnumVariantValue {
                enum_name,
                variant_name,
                payloads,
            }) if matches!(enum_name.as_str(), "ExitStatus" | "process.ExitStatus")
                && variant_name == "Exited"
                && matches!(payloads.as_slice(), [Value::Int(code)] if code.as_i128() == Some(0))
        )
    }

    pub(crate) fn check(&self) -> std::result::Result<(), Value> {
        if self.success() {
            Ok(())
        } else {
            Err(process_error_other(format!(
                "process exited with {}",
                self.status().render()
            )))
        }
    }
}

impl ProcessSupervisorValue {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(ProcessSupervisorState {
                services: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub(crate) fn start(
        &self,
        name: String,
        command: Vec<String>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        stdin: ProcessStdioConfig,
        stdout: ProcessStdioConfig,
        stderr: ProcessStdioConfig,
        restart: ProcessRestartPolicy,
        backoff: StdDuration,
        max_restarts: Option<i32>,
        group: bool,
    ) -> std::result::Result<(), Value> {
        if command.is_empty() {
            return Err(process_error_no_command());
        }
        if !matches!(restart, ProcessRestartPolicy::Never)
            && backoff < MIN_SUPERVISOR_RESTART_BACKOFF
        {
            return Err(process_error_io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "supervisor restart backoff must be at least {:?} when restart is enabled",
                    MIN_SUPERVISOR_RESTART_BACKOFF
                ),
            )));
        }

        let child = ProcessChildValue::spawn(
            command.clone(),
            cwd.clone(),
            env.clone(),
            stdin,
            stdout,
            stderr,
            group,
        )
        .map_err(|error| process_error_spawn(error.to_string()))?;

        let mut services = lock_mutex(&self.inner.services);
        if services.contains_key(&name) {
            return Err(process_error_other(format!(
                "supervisor already manages a child named `{}`",
                name
            )));
        }
        services.insert(
            name,
            ProcessSupervisorEntry {
                spec: ProcessSupervisorSpec {
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
                },
                child: Some(child),
                restart_count: 0,
                pending_restart_status: None,
                next_restart_at: None,
            },
        );
        Ok(())
    }

    pub(crate) fn wait(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> ProcessSupervisorWaitStatus {
        let deadline = timeout_deadline(timeout);
        loop {
            match self.try_collect_event() {
                Ok(Some(event)) => return ProcessSupervisorWaitStatus::Event(event),
                Ok(None) => {}
                Err(error) => {
                    return ProcessSupervisorWaitStatus::Event(process_supervisor_event_failed(
                        "<supervisor>".to_string(),
                        error,
                        IntegerValue::from_signed(0),
                    ))
                }
            }

            if self.is_empty() {
                return ProcessSupervisorWaitStatus::TimedOut;
            }
            if cancellation.is_some_and(CancellationContext::is_cancelled) {
                return ProcessSupervisorWaitStatus::Cancelled;
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return ProcessSupervisorWaitStatus::TimedOut;
            }

            sleep_with_runtime_scheduler(StdDuration::from_millis(5), cancellation);
        }
    }

    pub(crate) fn wait_or_none(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> std::result::Result<Option<Value>, Value> {
        match self.wait(timeout, cancellation) {
            ProcessSupervisorWaitStatus::Event(event) => Ok(Some(event)),
            ProcessSupervisorWaitStatus::TimedOut => Ok(None),
            ProcessSupervisorWaitStatus::Cancelled => Err(process_error_cancelled()),
        }
    }

    pub(crate) fn stop(&self) -> std::result::Result<(), Value> {
        let drained: Vec<ProcessChildValue> = {
            let mut services = lock_mutex(&self.inner.services);
            std::mem::take(&mut *services)
                .into_iter()
                .filter_map(|(_, entry)| entry.child)
                .collect::<Vec<_>>()
        };
        for child in drained {
            child.close();
        }
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        lock_mutex(&self.inner.services).is_empty()
    }

    pub(crate) fn close(&self) {
        let _ = self.stop();
    }

    fn try_collect_event(&self) -> std::result::Result<Option<Value>, Value> {
        let now = Instant::now();
        let names = {
            let services = lock_mutex(&self.inner.services);
            services.keys().cloned().collect::<Vec<_>>()
        };

        for name in names {
            enum Action {
                None,
                Emit(Value),
                RemoveAndEmit(Value),
            }

            let action = {
                let mut services = lock_mutex(&self.inner.services);
                let Some(entry) = services.get_mut(&name) else {
                    continue;
                };

                if let Some(child) = entry.child.clone() {
                    match child.try_wait_once() {
                        Ok(Some(status)) => {
                            if should_restart_process(
                                entry.spec.restart,
                                &status,
                                entry.restart_count,
                                entry.spec.max_restarts,
                            ) {
                                entry.restart_count += 1;
                                if entry.spec.backoff.is_zero() {
                                    match ProcessChildValue::spawn(
                                        entry.spec.command.clone(),
                                        entry.spec.cwd.clone(),
                                        entry.spec.env.clone(),
                                        entry.spec.stdin,
                                        entry.spec.stdout,
                                        entry.spec.stderr,
                                        entry.spec.group,
                                    ) {
                                        Ok(restarted_child) => {
                                            entry.child = Some(restarted_child);
                                            Action::Emit(process_supervisor_event_restarted(
                                                name.clone(),
                                                status,
                                                IntegerValue::from_signed(
                                                    entry.restart_count as i128,
                                                ),
                                            ))
                                        }
                                        Err(error) => {
                                            Action::RemoveAndEmit(process_supervisor_event_failed(
                                                name.clone(),
                                                process_error_spawn(error.to_string()),
                                                IntegerValue::from_signed(
                                                    entry.restart_count as i128,
                                                ),
                                            ))
                                        }
                                    }
                                } else {
                                    entry.child = None;
                                    entry.pending_restart_status = Some(status);
                                    entry.next_restart_at =
                                        now.checked_add(entry.spec.backoff).or(Some(now));
                                    Action::None
                                }
                            } else {
                                Action::RemoveAndEmit(process_supervisor_event_exited(
                                    name.clone(),
                                    status,
                                    IntegerValue::from_signed(entry.restart_count as i128),
                                ))
                            }
                        }
                        Ok(None) => Action::None,
                        Err(error) => Action::RemoveAndEmit(process_supervisor_event_failed(
                            name.clone(),
                            process_error_io(error),
                            IntegerValue::from_signed(entry.restart_count as i128),
                        )),
                    }
                } else if let (Some(status), Some(next_restart_at)) =
                    (entry.pending_restart_status.clone(), entry.next_restart_at)
                {
                    if next_restart_at <= now {
                        match ProcessChildValue::spawn(
                            entry.spec.command.clone(),
                            entry.spec.cwd.clone(),
                            entry.spec.env.clone(),
                            entry.spec.stdin,
                            entry.spec.stdout,
                            entry.spec.stderr,
                            entry.spec.group,
                        ) {
                            Ok(restarted_child) => {
                                entry.child = Some(restarted_child);
                                entry.pending_restart_status = None;
                                entry.next_restart_at = None;
                                Action::Emit(process_supervisor_event_restarted(
                                    name.clone(),
                                    status,
                                    IntegerValue::from_signed(entry.restart_count as i128),
                                ))
                            }
                            Err(error) => Action::RemoveAndEmit(process_supervisor_event_failed(
                                name.clone(),
                                process_error_spawn(error.to_string()),
                                IntegerValue::from_signed(entry.restart_count as i128),
                            )),
                        }
                    } else {
                        Action::None
                    }
                } else {
                    Action::None
                }
            };

            match action {
                Action::None => {}
                Action::Emit(event) => return Ok(Some(event)),
                Action::RemoveAndEmit(event) => {
                    lock_mutex(&self.inner.services).remove(&name);
                    return Ok(Some(event));
                }
            }
        }

        Ok(None)
    }
}

impl ProcessChildValue {
    pub(crate) fn spawn(
        command: Vec<String>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        stdin: ProcessStdioConfig,
        stdout: ProcessStdioConfig,
        stderr: ProcessStdioConfig,
        group: bool,
    ) -> io::Result<Self> {
        let Some(program) = command.first().cloned() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "process.start(...) requires at least one command element",
            ));
        };
        let mut builder = StdCommand::new(program);
        builder.args(command.iter().skip(1));
        if let Some(cwd) = cwd {
            builder.current_dir(cwd);
        }
        for (key, value) in env {
            builder.env(key, value);
        }
        builder.stdin(stdin.as_stdio());
        builder.stdout(stdout.as_stdio());
        builder.stderr(stderr.as_stdio());
        #[cfg(unix)]
        if group {
            // SAFETY: This runs in the child just before exec. It only calls the
            // async-signal-safe `setpgid(0, 0)` to place the child in its own
            // process group.
            unsafe {
                builder.pre_exec(|| {
                    if libc::setpgid(0, 0) < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
        }
        #[cfg(not(unix))]
        if group {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process groups are only supported on Unix hosts",
            ));
        }
        let mut child = builder.spawn()?;
        let process_group_id = if group { Some(child.id() as i32) } else { None };
        let stdin = child
            .stdin
            .take()
            .map(ProcessPipeValue::from_stdin)
            .transpose()?;
        let stdout = child
            .stdout
            .take()
            .map(ProcessPipeValue::from_stdout)
            .transpose()?;
        let stderr = child
            .stderr
            .take()
            .map(ProcessPipeValue::from_stderr)
            .transpose()?;
        Ok(Self {
            inner: Arc::new(ProcessChildState {
                child: Mutex::new(Some(child)),
                waited: Mutex::new(None),
                process_group_id,
                stdin,
                stdout,
                stderr,
            }),
        })
    }

    pub(crate) fn stdin(&self) -> Option<ProcessPipeValue> {
        self.inner.stdin.clone()
    }

    pub(crate) fn stdout(&self) -> Option<ProcessPipeValue> {
        self.inner.stdout.clone()
    }

    pub(crate) fn stderr(&self) -> Option<ProcessPipeValue> {
        self.inner.stderr.clone()
    }

    pub(crate) fn wait(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> ProcessChildWaitStatus {
        if let Some(status) = lock_mutex(&self.inner.waited).clone() {
            return ProcessChildWaitStatus::Exited(status);
        }
        let deadline = timeout_deadline(timeout);
        loop {
            match self.try_wait_once() {
                Ok(Some(status)) => return ProcessChildWaitStatus::Exited(status),
                Ok(None) => {}
                Err(error) => return ProcessChildWaitStatus::Failed(error),
            }
            if cancellation.is_some_and(CancellationContext::is_cancelled) {
                return ProcessChildWaitStatus::Cancelled;
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return ProcessChildWaitStatus::TimedOut;
            }
            sleep_with_runtime_scheduler(StdDuration::from_millis(5), cancellation);
        }
    }

    pub(crate) fn wait_or_none(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> std::result::Result<Option<StdExitStatus>, Value> {
        match self.wait(timeout, cancellation) {
            ProcessChildWaitStatus::Exited(status) => Ok(Some(status)),
            ProcessChildWaitStatus::TimedOut => Ok(None),
            ProcessChildWaitStatus::Cancelled => Err(process_error_cancelled()),
            ProcessChildWaitStatus::Failed(error) => Err(process_error_io(error)),
        }
    }

    pub(crate) fn wait_ok(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> std::result::Result<StdExitStatus, Value> {
        match self.wait(timeout, cancellation) {
            ProcessChildWaitStatus::Exited(status) if status.success() => Ok(status),
            ProcessChildWaitStatus::Exited(status) => Err(process_error_other(format!(
                "process exited with {}",
                process_exit_status(status).render()
            ))),
            ProcessChildWaitStatus::TimedOut => Err(process_error_timed_out()),
            ProcessChildWaitStatus::Cancelled => Err(process_error_cancelled()),
            ProcessChildWaitStatus::Failed(error) => Err(process_error_io(error)),
        }
    }

    fn try_wait_once(&self) -> io::Result<Option<StdExitStatus>> {
        if let Some(status) = lock_mutex(&self.inner.waited).clone() {
            return Ok(Some(status));
        }
        let mut child_slot = lock_mutex(&self.inner.child);
        let Some(child) = child_slot.as_mut() else {
            return Ok(lock_mutex(&self.inner.waited).clone());
        };
        match child.try_wait()? {
            Some(status) => {
                *lock_mutex(&self.inner.waited) = Some(status.clone());
                *child_slot = None;
                Ok(Some(status))
            }
            None => Ok(None),
        }
    }

    pub(crate) fn kill(&self) -> io::Result<()> {
        if let Some(process_group_id) = self.inner.process_group_id {
            return signal_process_group(process_group_id, libc::SIGKILL);
        }
        let mut child = lock_mutex(&self.inner.child);
        let Some(child) = child.as_mut() else {
            return Ok(());
        };
        child.kill()
    }

    pub(crate) fn terminate(&self) -> io::Result<()> {
        if let Some(process_group_id) = self.inner.process_group_id {
            return signal_process_group(process_group_id, libc::SIGTERM);
        }
        let mut child = lock_mutex(&self.inner.child);
        let Some(child) = child.as_mut() else {
            return Ok(());
        };
        #[cfg(unix)]
        {
            if unsafe { libc::kill(child.id() as i32, libc::SIGTERM) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            child.kill()
        }
    }

    pub(crate) fn close(&self) {
        let _ = self.terminate();
        let wait_timed_out = matches!(
            self.wait(
                Some(StdDuration::from_millis(100)),
                current_lightweight_task_cancellation().as_ref(),
            ),
            ProcessChildWaitStatus::TimedOut
        );
        let group_wait_timed_out = if wait_timed_out {
            true
        } else {
            !self.wait_for_process_group_exit(
                Some(StdDuration::from_millis(100)),
                current_lightweight_task_cancellation().as_ref(),
            )
        };
        if wait_timed_out || group_wait_timed_out {
            let _ = self.kill();
            let _ = self.wait(
                Some(StdDuration::from_millis(100)),
                current_lightweight_task_cancellation().as_ref(),
            );
            let _ = self.wait_for_process_group_exit(
                Some(StdDuration::from_millis(100)),
                current_lightweight_task_cancellation().as_ref(),
            );
        }
        if let Some(stdin) = &self.inner.stdin {
            stdin.close();
        }
        if let Some(stdout) = &self.inner.stdout {
            stdout.close();
        }
        if let Some(stderr) = &self.inner.stderr {
            stderr.close();
        }
    }

    fn wait_for_process_group_exit(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> bool {
        let Some(process_group_id) = self.inner.process_group_id else {
            return true;
        };
        let deadline = timeout_deadline(timeout);
        loop {
            if !process_group_alive(process_group_id) {
                return true;
            }
            if cancellation.is_some_and(CancellationContext::is_cancelled) {
                return false;
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return false;
            }
            sleep_with_runtime_scheduler(StdDuration::from_millis(5), cancellation);
        }
    }
}

#[cfg(unix)]
fn signal_process_group(process_group_id: i32, signal: libc::c_int) -> io::Result<()> {
    if unsafe { libc::kill(-process_group_id, signal) } < 0 {
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => Ok(()),
            _ => Err(error),
        }
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn signal_process_group(_process_group_id: i32, _signal: libc::c_int) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn process_group_alive(process_group_id: i32) -> bool {
    if unsafe { libc::kill(-process_group_id, 0) } == 0 {
        true
    } else {
        let error = io::Error::last_os_error();
        matches!(error.raw_os_error(), Some(libc::EPERM))
    }
}

#[cfg(not(unix))]
fn process_group_alive(_process_group_id: i32) -> bool {
    false
}

fn should_restart_process(
    policy: ProcessRestartPolicy,
    status: &StdExitStatus,
    restart_count: i32,
    max_restarts: Option<i32>,
) -> bool {
    let allowed_by_policy = match policy {
        ProcessRestartPolicy::Never => false,
        ProcessRestartPolicy::OnFailure => !status.success(),
        ProcessRestartPolicy::Always => true,
    };
    if !allowed_by_policy {
        return false;
    }
    max_restarts.is_none_or(|max_restarts| restart_count < max_restarts)
}

impl TcpListenerValue {
    fn from_std(listener: StdTcpListener) -> io::Result<Self> {
        #[cfg(unix)]
        listener.set_nonblocking(true)?;
        Ok(Self {
            inner: Arc::new(TcpListenerState {
                listener: Mutex::new(Some(listener)),
            }),
        })
    }

    pub(crate) fn bind(address: &str) -> io::Result<Self> {
        Self::from_std(StdTcpListener::bind(address)?)
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
                Ok((stream, _)) => return TcpStreamValue::from_std(stream),
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
    fn from_std(stream: StdTcpStream) -> io::Result<Self> {
        #[cfg(unix)]
        stream.set_nonblocking(true)?;
        Ok(Self {
            inner: Arc::new(TcpStreamState {
                stream: Mutex::new(Some(stream)),
            }),
        })
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
                    Ok(stream) => return Self::from_std(stream),
                    Err(error) => last_error = Some(error),
                }
            }
            last_error
        } else {
            let mut last_error = None;
            for candidate in addresses {
                match StdTcpStream::connect(candidate) {
                    Ok(stream) => return Self::from_std(stream),
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
                    Ok(bytes) => {
                        push_limited_bytes(&mut contents, &chunk[..bytes], "network read_all")?
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        if let Some(deadline) = deadline {
                            if Instant::now() >= deadline {
                                return Err(timeout_resource_error());
                            }
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
    fn from_std(socket: StdUdpSocket) -> io::Result<Self> {
        #[cfg(unix)]
        socket.set_nonblocking(true)?;
        Ok(Self {
            inner: Arc::new(UdpSocketState {
                socket: Mutex::new(Some(socket)),
            }),
        })
    }

    pub(crate) fn bind(address: &str) -> io::Result<Self> {
        Self::from_std(StdUdpSocket::bind(address)?)
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
                    Err(error) => return Err(normalize_udp_send_error(error)),
                }
            }
        }
        #[cfg(not(unix))]
        {
            socket.set_write_timeout(next_wait_slice(
                deadline_from_timeout(timeout),
                cancellation,
            )?)?;
            socket
                .send_to(bytes, address)
                .map_err(normalize_udp_send_error)?;
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
            let mut buffer = vec![0u8; validate_udp_datagram_limit(max_bytes)?];
            loop {
                match socket.recv(&mut buffer) {
                    Ok(0) => return Ok(None),
                    Ok(bytes) => {
                        buffer.truncate(bytes);
                        return Ok(Some(buffer));
                    }
                    Err(error) if is_retryable_network_error(&error) => {
                        match wait_for_fd_event(
                            socket.as_raw_fd(),
                            libc::POLLIN,
                            deadline,
                            cancellation,
                        ) {
                            Ok(()) => {}
                            Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                                return Ok(None);
                            }
                            Err(error) => return Err(error),
                        }
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
            let mut buffer = vec![0u8; validate_udp_datagram_limit(max_bytes)?];
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
            let mut buffer = vec![0u8; validate_udp_datagram_limit(max_bytes)?];
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
                        match wait_for_fd_event(
                            socket.as_raw_fd(),
                            libc::POLLIN,
                            deadline,
                            cancellation,
                        ) {
                            Ok(()) => {}
                            Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                                return Ok(None);
                            }
                            Err(error) => return Err(error),
                        }
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
            let mut buffer = vec![0u8; validate_udp_datagram_limit(max_bytes)?];
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
    fn from_std(listener: StdUnixListener) -> io::Result<Self> {
        listener.set_nonblocking(true)?;
        Ok(Self {
            inner: Arc::new(UnixListenerState {
                listener: Mutex::new(Some(listener)),
            }),
        })
    }

    pub(crate) fn bind(path: &str) -> io::Result<Self> {
        match std::fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "unix listener path already exists",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Self::from_std(StdUnixListener::bind(path)?)
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
                Ok((stream, _)) => return UnixStreamValue::from_std(stream),
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
    fn from_std(stream: StdUnixStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            inner: Arc::new(UnixStreamState {
                stream: Mutex::new(Some(stream)),
            }),
        })
    }

    pub(crate) fn connect(
        path: &str,
        _timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Self> {
        if cancellation.is_some_and(|context| context.is_cancelled()) {
            return Err(cancelled_resource_error());
        }
        Self::from_std(StdUnixStream::connect(path)?)
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
        let deadline = deadline_from_timeout(timeout);
        loop {
            let mut wait_fd = None;
            let accepted = {
                let mut listener = lock_mutex(&self.inner.listener);
                let Some(listener) = listener.as_mut() else {
                    return Err(closed_resource_error());
                };
                match listener.accept() {
                    Ok((stream, _)) => Ok(Some(stream)),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        wait_fd = Some(listener.as_raw_fd());
                        Ok(None)
                    }
                    Err(error) => Err(error),
                }
            }?;
            if let Some(fd) = wait_fd {
                wait_for_fd_event(fd, libc::POLLIN, deadline, cancellation)?;
                continue;
            }
            let Some(stream) = accepted else {
                continue;
            };
            #[cfg(unix)]
            stream.set_nonblocking(true)?;
            let connection =
                ServerConnection::new(self.inner.config.clone()).map_err(io::Error::other)?;
            let mut tls_stream = rustls::StreamOwned::new(connection, stream);
            if let Err(error) = complete_tls_server_handshake(
                &mut tls_stream,
                tls_handshake_deadline(deadline),
                cancellation,
            ) {
                if cancellation.is_some_and(CancellationContext::is_cancelled) {
                    return Err(error);
                }
                drop(tls_stream);
                continue;
            }
            return Ok(TlsStreamValue {
                inner: Arc::new(TlsStreamState {
                    stream: Mutex::new(Some(TlsStreamKind::Server(tls_stream))),
                }),
            });
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

fn complete_tls_client_handshake(
    stream: &mut rustls::StreamOwned<ClientConnection, StdTcpStream>,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<()> {
    while stream.conn.is_handshaking() {
        check_deadline_and_cancellation(deadline, cancellation)?;
        match stream.conn.complete_io(&mut stream.sock) {
            Ok(_) => {}
            Err(error) if is_retryable_network_error(&error) => {
                #[cfg(unix)]
                wait_for_fd_event(
                    stream.sock.as_raw_fd(),
                    libc::POLLIN | libc::POLLOUT,
                    deadline,
                    cancellation,
                )?;
                #[cfg(not(unix))]
                return Err(error);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn complete_tls_server_handshake(
    stream: &mut rustls::StreamOwned<ServerConnection, StdTcpStream>,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<()> {
    while stream.conn.is_handshaking() {
        check_deadline_and_cancellation(deadline, cancellation)?;
        match stream.conn.complete_io(&mut stream.sock) {
            Ok(_) => {}
            Err(error) if is_retryable_network_error(&error) => {
                #[cfg(unix)]
                wait_for_fd_event(
                    stream.sock.as_raw_fd(),
                    libc::POLLIN | libc::POLLOUT,
                    deadline,
                    cancellation,
                )?;
                #[cfg(not(unix))]
                return Err(error);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
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
        let mut stream = rustls::StreamOwned::new(connection, stream);
        complete_tls_client_handshake(
            &mut stream,
            tls_handshake_deadline(deadline_from_timeout(timeout)),
            cancellation,
        )?;
        Ok(Self {
            inner: Arc::new(TlsStreamState {
                stream: Mutex::new(Some(TlsStreamKind::Client(stream))),
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
        let deadline = deadline_from_timeout(timeout);
        loop {
            #[cfg(unix)]
            let stream = {
                let mut listener = lock_mutex(&self.inner.listener);
                let Some(listener) = listener.as_mut() else {
                    return Err(closed_resource_error());
                };
                loop {
                    match listener.accept() {
                        Ok((stream, _)) => break TcpStreamValue::from_std(stream)?,
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                            ) =>
                        {
                            wait_for_fd_event(
                                listener.as_raw_fd(),
                                libc::POLLIN,
                                deadline,
                                cancellation,
                            )?;
                        }
                        Err(error) => return Err(error),
                    }
                }
            };
            #[cfg(not(unix))]
            let stream = {
                let mut listener = lock_mutex(&self.inner.listener);
                let Some(listener) = listener.as_mut() else {
                    return Err(closed_resource_error());
                };
                TcpStreamValue::from_std(listener.accept()?.0)?
            };
            let request = {
                let mut raw_stream = lock_mutex(&stream.inner.stream);
                let Some(raw_stream) = raw_stream.as_mut() else {
                    return Err(closed_resource_error());
                };
                read_http_request_from_stream(raw_stream, deadline, cancellation)
            };
            let (method, path, headers, body) = match request {
                Ok(request) => request,
                Err(error)
                    if is_http_message_too_large_error(&error)
                        || is_http_headers_too_large_error(&error) =>
                {
                    let status = if is_http_message_too_large_error(&error) {
                        413
                    } else {
                        431
                    };
                    let mut raw_stream = lock_mutex(&stream.inner.stream);
                    if let Some(raw_stream) = raw_stream.as_mut() {
                        let _ = write_http_response_to_stream(
                            raw_stream,
                            status,
                            Vec::new(),
                            b"",
                            deadline,
                            cancellation,
                        );
                    }
                    drop(raw_stream);
                    stream.close();
                    continue;
                }
                Err(error) => return Err(error),
            };
            return Ok(HttpExchangeValue {
                inner: Arc::new(HttpExchangeState {
                    stream: Mutex::new(Some(stream)),
                    method,
                    path,
                    headers,
                    body,
                }),
            });
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
        let request = build_http_request_bytes(method, &url, body, headers)?;
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
        let cancellation = current_lightweight_task_cancellation();
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    #[cfg(unix)]
                    let mut state = run_blocking_io(
                        move || accept_websocket_stream(stream, deadline),
                        cancellation.as_ref(),
                    )?;
                    #[cfg(not(unix))]
                    let mut state = {
                        let socket =
                            match websocket_accept_with_config(stream, Some(websocket_config())) {
                                Ok(socket) => socket,
                                Err(tungstenite::handshake::HandshakeError::Interrupted(_)) => {
                                    return Err(io::Error::new(
                                        io::ErrorKind::WouldBlock,
                                        "websocket handshake unexpectedly blocked",
                                    ));
                                }
                                Err(tungstenite::handshake::HandshakeError::Failure(error)) => {
                                    return Err(websocket_error_to_io(error));
                                }
                            };
                        WebSocketStateKind::Plain(socket)
                    };
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
            let request = websocket_client_request(&parsed)?;
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
            let deadline = deadline_from_timeout(timeout);
            let cancellation = current_lightweight_task_cancellation();
            let mut state = run_blocking_io(
                move || connect_websocket_stream(stream, request, deadline),
                cancellation.as_ref(),
            )?;
            websocket_set_nonblocking(&mut state, true)?;
            return Ok(Self {
                inner: Arc::new(WebSocketState {
                    socket: Mutex::new(Some(state)),
                }),
            });
        }

        #[cfg(not(unix))]
        {
            let (socket, _) = tungstenite::connect(url).map_err(websocket_error_to_io)?;
            let state = WebSocketStateKind::MaybeTls(socket);
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
                        wait_for_fd_event(
                            websocket_raw_fd(socket)?,
                            libc::POLLOUT,
                            deadline,
                            None,
                        )?;
                        #[cfg(not(unix))]
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "websocket write buffer is full",
                        ));
                    }
                    Err(tungstenite::Error::Io(error)) if is_retryable_network_error(&error) => {
                        #[cfg(unix)]
                        wait_for_fd_event(
                            websocket_raw_fd(socket)?,
                            libc::POLLOUT,
                            deadline,
                            None,
                        )?;
                        #[cfg(not(unix))]
                        return Err(error);
                    }
                    Err(error) => return Err(websocket_error_to_io(error)),
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
                        wait_for_fd_event(
                            websocket_raw_fd(socket)?,
                            libc::POLLOUT,
                            deadline,
                            None,
                        )?;
                        #[cfg(not(unix))]
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "websocket write buffer is full",
                        ));
                    }
                    Err(tungstenite::Error::Io(error)) if is_retryable_network_error(&error) => {
                        #[cfg(unix)]
                        wait_for_fd_event(
                            websocket_raw_fd(socket)?,
                            libc::POLLOUT,
                            deadline,
                            None,
                        )?;
                        #[cfg(not(unix))]
                        return Err(error);
                    }
                    Err(error) => return Err(websocket_error_to_io(error)),
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
            Some(WebSocketStateKind::Plain(socket)) => {
                socket.close(None).map_err(websocket_error_to_io)
            }
            Some(WebSocketStateKind::MaybeTls(socket)) => {
                socket.close(None).map_err(websocket_error_to_io)
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
    pub(crate) fn completed_result(&self) -> Option<TaskExecutionResult> {
        let state = lock_mutex(&self.inner.handle);
        match &*state {
            TaskHandle::Completed(result) => Some(result.clone()),
            TaskHandle::Running { .. } => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_handle(
        handle: thread::JoinHandle<std::result::Result<Value, Diagnostic>>,
    ) -> Self {
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
                Ok(result) => TaskExecutionResult::Ready(result),
                Err(_) => TaskExecutionResult::Ready(Err(Diagnostic::new("spawned task panicked"))),
            };
            let mut task_state = lock_mutex(&state.handle);
            *task_state = TaskHandle::Completed(result);
            state.ready.notify_all();
            runtime_scheduler().notify();
        });
        Self { inner }
    }

    pub(crate) fn join_result(&self) -> TaskExecutionResult {
        loop {
            if let Some(result) = self.completed_result() {
                return result;
            }

            if self.inner.lightweight {
                if let Some(task_id) =
                    with_current_lightweight_task_context(|context| context.task_id)
                {
                    {
                        let mut state = lock_mutex(&self.inner.handle);
                        match &mut *state {
                            TaskHandle::Completed(result) => return result.clone(),
                            TaskHandle::Running { waiters } => {
                                if !waiters.contains(&task_id) {
                                    waiters.push(task_id);
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

    pub(crate) fn wait_result_with_cancellation(
        &self,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> TaskWaitStatus {
        let deadline = deadline_from_timeout(timeout);
        loop {
            if let Some(result) = self.completed_result() {
                return match result {
                    TaskExecutionResult::Ready(result) => TaskWaitStatus::Ready(result),
                    TaskExecutionResult::Cancelled => TaskWaitStatus::Cancelled,
                };
            }

            match wait_for_runtime_scheduler(
                Vec::new(),
                false,
                Vec::new(),
                vec![self.clone()],
                deadline,
                cancellation,
            ) {
                RuntimeSchedulerWakeReason::Ready => {}
                RuntimeSchedulerWakeReason::TimedOut => return TaskWaitStatus::TimedOut,
                RuntimeSchedulerWakeReason::Cancelled => return TaskWaitStatus::Cancelled,
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

pub(crate) fn send_error_cancelled(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SendError".to_string(),
        variant_name: "Cancelled".to_string(),
        payloads: vec![value],
    })
}

pub(crate) fn send_error_timed_out(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SendError".to_string(),
        variant_name: "TimedOut".to_string(),
        payloads: vec![value],
    })
}

pub(crate) fn send_error_full(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SendError".to_string(),
        variant_name: "Full".to_string(),
        payloads: vec![value],
    })
}

pub(crate) fn queue_receive_item(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "QueueReceive".to_string(),
        variant_name: "Item".to_string(),
        payloads: vec![value],
    })
}

pub(crate) fn queue_receive_closed() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "QueueReceive".to_string(),
        variant_name: "Closed".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn queue_receive_timed_out() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "QueueReceive".to_string(),
        variant_name: "TimedOut".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn queue_receive_cancelled() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "QueueReceive".to_string(),
        variant_name: "Cancelled".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn task_result_ready(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "TaskResult".to_string(),
        variant_name: "Ready".to_string(),
        payloads: vec![value],
    })
}

pub(crate) fn task_result_timed_out() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "TaskResult".to_string(),
        variant_name: "TimedOut".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn task_result_cancelled() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "TaskResult".to_string(),
        variant_name: "Cancelled".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn wait_any_ready(index: i32, value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "WaitAny".to_string(),
        variant_name: "Ready".to_string(),
        payloads: vec![Value::Int(IntegerValue::from_signed(index as i128)), value],
    })
}

pub(crate) fn wait_any_timed_out() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "WaitAny".to_string(),
        variant_name: "TimedOut".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn wait_any_cancelled() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "WaitAny".to_string(),
        variant_name: "Cancelled".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn wait_all_ready(values: Vec<Value>) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "WaitAll".to_string(),
        variant_name: "Ready".to_string(),
        payloads: vec![Value::Vec(VecValue {
            element_type: Type::named("Unknown"),
            elements: values,
        })],
    })
}

pub(crate) fn wait_all_timed_out() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "WaitAll".to_string(),
        variant_name: "TimedOut".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn wait_all_cancelled() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "WaitAll".to_string(),
        variant_name: "Cancelled".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_stdio_inherit() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Stdio".to_string(),
        variant_name: "Inherit".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_stdio_null() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Stdio".to_string(),
        variant_name: "Null".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_stdio_pipe() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Stdio".to_string(),
        variant_name: "Pipe".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_exit_status(status: StdExitStatus) -> Value {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return Value::EnumVariant(EnumVariantValue {
                enum_name: "ExitStatus".to_string(),
                variant_name: "Signaled".to_string(),
                payloads: vec![Value::Int(IntegerValue::from_signed(signal as i128))],
            });
        }
    }
    let code = status.code().unwrap_or_default();
    Value::EnumVariant(EnumVariantValue {
        enum_name: "ExitStatus".to_string(),
        variant_name: "Exited".to_string(),
        payloads: vec![Value::Int(IntegerValue::from_signed(code as i128))],
    })
}

pub(crate) fn process_wait_exited(status: StdExitStatus) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Wait".to_string(),
        variant_name: "Exited".to_string(),
        payloads: vec![process_exit_status(status)],
    })
}

pub(crate) fn process_wait_timed_out() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Wait".to_string(),
        variant_name: "TimedOut".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_wait_cancelled() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Wait".to_string(),
        variant_name: "Cancelled".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_wait_failed(error: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Wait".to_string(),
        variant_name: "Failed".to_string(),
        payloads: vec![error],
    })
}

pub(crate) fn process_supervisor_wait_event(event: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SupervisorWait".to_string(),
        variant_name: "Event".to_string(),
        payloads: vec![event],
    })
}

pub(crate) fn process_supervisor_wait_timed_out() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SupervisorWait".to_string(),
        variant_name: "TimedOut".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_supervisor_wait_cancelled() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SupervisorWait".to_string(),
        variant_name: "Cancelled".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_supervisor_event_exited(
    name: String,
    status: StdExitStatus,
    restart_count: IntegerValue,
) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SupervisorEvent".to_string(),
        variant_name: "Exited".to_string(),
        payloads: vec![
            Value::String(name),
            process_exit_status(status),
            Value::Int(restart_count),
        ],
    })
}

pub(crate) fn process_supervisor_event_restarted(
    name: String,
    status: StdExitStatus,
    restart_count: IntegerValue,
) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SupervisorEvent".to_string(),
        variant_name: "Restarted".to_string(),
        payloads: vec![
            Value::String(name),
            process_exit_status(status),
            Value::Int(restart_count),
        ],
    })
}

pub(crate) fn process_supervisor_event_failed(
    name: String,
    error: Value,
    restart_count: IntegerValue,
) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SupervisorEvent".to_string(),
        variant_name: "Failed".to_string(),
        payloads: vec![Value::String(name), error, Value::Int(restart_count)],
    })
}

pub(crate) fn process_error_no_command() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Error".to_string(),
        variant_name: "NoCommand".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_error_timed_out() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Error".to_string(),
        variant_name: "TimedOut".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_error_cancelled() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Error".to_string(),
        variant_name: "Cancelled".to_string(),
        payloads: Vec::new(),
    })
}

pub(crate) fn process_error_io(error: io::Error) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Error".to_string(),
        variant_name: "Io".to_string(),
        payloads: vec![io_error(error)],
    })
}

pub(crate) fn process_error_spawn(message: String) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Error".to_string(),
        variant_name: "Spawn".to_string(),
        payloads: vec![Value::String(message)],
    })
}

pub(crate) fn process_error_other(message: String) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Error".to_string(),
        variant_name: "Other".to_string(),
        payloads: vec![Value::String(message)],
    })
}

pub(crate) fn decode_process_stdio(value: &Value, label: &str) -> Result<ProcessStdioConfig> {
    match value {
        Value::EnumVariant(variant)
            if matches!(variant.enum_name.as_str(), "Stdio" | "process.Stdio") =>
        {
            match variant.variant_name.as_str() {
                "Inherit" => Ok(ProcessStdioConfig::Inherit),
                "Null" => Ok(ProcessStdioConfig::Null),
                "Pipe" => Ok(ProcessStdioConfig::Pipe),
                _ => Err(Diagnostic::new(format!(
                    "`{}` received an unknown `process.Stdio` variant `{}`",
                    label, variant.variant_name
                ))),
            }
        }
        other => Err(Diagnostic::new(format!(
            "`{}` expects `process.Stdio`, found `{}`",
            label,
            other.render()
        ))),
    }
}

pub(crate) fn decode_process_restart_policy(
    value: &Value,
    label: &str,
) -> Result<ProcessRestartPolicy> {
    match value {
        Value::EnumVariant(variant)
            if matches!(
                variant.enum_name.as_str(),
                "RestartPolicy" | "process.RestartPolicy"
            ) =>
        {
            match variant.variant_name.as_str() {
                "Never" => Ok(ProcessRestartPolicy::Never),
                "OnFailure" => Ok(ProcessRestartPolicy::OnFailure),
                "Always" => Ok(ProcessRestartPolicy::Always),
                _ => Err(Diagnostic::new(format!(
                    "`{}` received an unknown `process.RestartPolicy` variant `{}`",
                    label, variant.variant_name
                ))),
            }
        }
        other => Err(Diagnostic::new(format!(
            "`{}` expects `process.RestartPolicy`, found `{}`",
            label,
            other.render()
        ))),
    }
}

pub(crate) fn io_error(error: io::Error) -> Value {
    let (variant_name, payloads) = match error.kind() {
        io::ErrorKind::NotFound => ("NotFound", Vec::new()),
        io::ErrorKind::PermissionDenied => ("PermissionDenied", Vec::new()),
        io::ErrorKind::AlreadyExists => ("AlreadyExists", Vec::new()),
        io::ErrorKind::IsADirectory => ("IsDirectory", Vec::new()),
        io::ErrorKind::ConnectionRefused => ("ConnectionRefused", Vec::new()),
        io::ErrorKind::ConnectionReset => ("ConnectionReset", Vec::new()),
        io::ErrorKind::ConnectionAborted => ("ConnectionAborted", Vec::new()),
        io::ErrorKind::NotConnected => ("NotConnected", Vec::new()),
        io::ErrorKind::AddrInUse => ("AddrInUse", Vec::new()),
        io::ErrorKind::AddrNotAvailable => ("AddrNotAvailable", Vec::new()),
        io::ErrorKind::BrokenPipe if error.to_string() == "resource is closed" => {
            ("Closed", Vec::new())
        }
        io::ErrorKind::Interrupted if error.to_string() == "operation cancelled" => {
            ("Cancelled", Vec::new())
        }
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
