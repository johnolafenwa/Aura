use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::fs::{self, File as StdFile, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::net::{
    Shutdown, TcpListener as StdTcpListener, TcpStream as StdTcpStream, ToSocketAddrs,
    UdpSocket as StdUdpSocket,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Once};
use std::thread::JoinHandle;
use std::time::{Duration as StdDuration, Instant};

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection};
use rustls_pemfile::{certs, private_key};
use tiny_http::{
    Method as TinyHttpMethod, Request as TinyHttpRequest, Response as TinyHttpResponse,
};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{accept as websocket_accept, client_tls_with_config, Message, WebSocket};
use ureq::AgentBuilder;

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
    server: Mutex<Option<Arc<tiny_http::Server>>>,
}

struct HttpExchangeState {
    request: Mutex<Option<TinyHttpRequest>>,
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

    pub(crate) fn recv_timeout(&self, timeout: StdDuration) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        let mut state = lock_mutex(&self.inner.state);
        loop {
            if let Some(value) = state.queue.pop_front() {
                return Some(value);
            }
            if state.closed {
                return None;
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next_state, timed_out) = wait_timeout_condvar(&self.inner.ready, state, remaining);
            state = next_state;
            if timed_out && state.queue.is_empty() {
                return None;
            }
        }
    }

    pub(crate) fn close(&self) {
        let mut state = lock_mutex(&self.inner.state);
        state.closed = true;
        drop(state);
        self.inner.ready.notify_all();
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
    loop {
        let slice = next_wait_slice(deadline, cancellation)?;
        let timeout_ms = match slice {
            Some(slice) => {
                i32::try_from(slice.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX)
            }
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

fn tiny_http_method_name(method: &TinyHttpMethod) -> String {
    match method {
        TinyHttpMethod::Get => "GET",
        TinyHttpMethod::Post => "POST",
        TinyHttpMethod::Put => "PUT",
        TinyHttpMethod::Delete => "DELETE",
        TinyHttpMethod::Head => "HEAD",
        TinyHttpMethod::Options => "OPTIONS",
        TinyHttpMethod::Connect => "CONNECT",
        TinyHttpMethod::Patch => "PATCH",
        TinyHttpMethod::Trace => "TRACE",
        TinyHttpMethod::NonStandard(name) => name.as_str(),
    }
    .to_string()
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
        let server = tiny_http::Server::http(address).map_err(io::Error::other)?;
        Ok(Self {
            inner: Arc::new(HttpListenerState {
                server: Mutex::new(Some(Arc::new(server))),
            }),
        })
    }

    pub(crate) fn accept(&self, timeout: Option<StdDuration>) -> io::Result<HttpExchangeValue> {
        let server = lock_mutex(&self.inner.server);
        let Some(server) = server.as_ref() else {
            return Err(closed_resource_error());
        };
        let mut request = match timeout {
            Some(timeout) => server
                .recv_timeout(timeout)
                .map_err(io::Error::other)?
                .ok_or_else(timeout_resource_error)?,
            None => server.recv().map_err(io::Error::other)?,
        };
        let method = tiny_http_method_name(request.method());
        let path = request.url().to_string();
        let headers = request
            .headers()
            .iter()
            .map(|header| (header.field.to_string(), header.value.to_string()))
            .collect::<Vec<_>>();
        let mut body = Vec::new();
        request.as_reader().read_to_end(&mut body)?;
        Ok(HttpExchangeValue {
            inner: Arc::new(HttpExchangeState {
                request: Mutex::new(Some(request)),
                method,
                path,
                headers,
                body,
            }),
        })
    }

    pub(crate) fn local_addr(&self) -> io::Result<String> {
        let server = lock_mutex(&self.inner.server);
        let Some(server) = server.as_ref() else {
            return Err(closed_resource_error());
        };
        Ok(server.server_addr().to_string())
    }

    pub(crate) fn close(&self) {
        let mut server = lock_mutex(&self.inner.server);
        *server = None;
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
        let mut request = lock_mutex(&self.inner.request);
        let Some(request) = request.take() else {
            return Err(closed_resource_error());
        };
        let mut response =
            TinyHttpResponse::from_data(body.to_vec()).with_status_code(status as u16);
        for (name, value) in headers {
            response = response.with_header(
                tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid HTTP header")
                })?,
            );
        }
        request.respond(response).map_err(io::Error::other)
    }
}

impl HttpResponseValue {
    pub(crate) fn request_text(
        method: &str,
        url: &str,
        body: &str,
        headers: Vec<(String, String)>,
        timeout: Option<StdDuration>,
    ) -> io::Result<Self> {
        Self::request_bytes(method, url, body.as_bytes(), headers, timeout)
    }

    pub(crate) fn request_bytes(
        method: &str,
        url: &str,
        body: &[u8],
        headers: Vec<(String, String)>,
        timeout: Option<StdDuration>,
    ) -> io::Result<Self> {
        let mut agent = AgentBuilder::new();
        if let Some(timeout) = timeout {
            agent = agent.timeout(timeout);
        }
        let agent = agent.build();
        let mut request = agent.request(method, url);
        for (name, value) in headers {
            request = request.set(&name, &value);
        }
        let response = match request.send_bytes(body) {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            Err(ureq::Error::Transport(error)) => {
                return Err(io::Error::new(io::ErrorKind::Other, error.to_string()))
            }
        };
        let status = i32::from(response.status());
        let reason = response.status_text().to_string();
        let headers = response
            .headers_names()
            .into_iter()
            .filter_map(|name| {
                response
                    .header(&name)
                    .map(|value| (name.to_string(), value.to_string()))
            })
            .collect::<Vec<_>>();
        let mut reader = response.into_reader();
        let mut body = Vec::new();
        reader.read_to_end(&mut body)?;
        Ok(parse_http_response(status, reason, headers, body))
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
            return Err("task result handle was not available".to_string());
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
