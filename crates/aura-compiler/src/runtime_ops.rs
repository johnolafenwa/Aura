//! Resolved runtime operations (Batch 7 step 8d).
//!
//! MIR lowering resolves a member call on a receiver of concrete built-in
//! type, such as `file.read_all()`, to one `RuntimeOp`. The op names the
//! runtime helper that implements it, the receiver family it applies to, and
//! its result type, so the interpreter and every native emitter dispatch on
//! the op instead of re-resolving the member name.
//!
//! Ops are grouped by family. This table holds the resource handle
//! (`fs.File`, `process.*`, `net.*`), `random.Rng`, scalar, and value
//! families. Strings, collections, queues, tasks, named built-ins, and
//! `print` are still `CallTarget::Member` or `CallTarget::Name` calls.

use serde::{Deserialize, Serialize};

use crate::call::{BuiltinMember, CallableParam};
use crate::runtime_abi::RuntimeFn;
use crate::sema::{optional_type, Type};

/// One resolved built-in operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum RuntimeOp {
    /// A method of a host resource handle: `fs.File`, `process.*`, `net.*`.
    Resource(ResourceOp),
    /// A method of `random.Rng`.
    Rng(RngOp),
    /// A method of an integer, `float64`, or `Duration` value.
    Scalar(ScalarOp),
    /// `to_string`, `clone`, or `copy` on a concrete built-in value.
    Value(ValueOp),
}

/// The runtime helper an op calls. The emitter passes the receiver word,
/// the argument words, the immediate words, and then, when `span` is set,
/// a source line and column word. A member call names no source span, so
/// both are zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeCall {
    pub(crate) function: RuntimeFn,
    pub(crate) immediates: &'static [i64],
    pub(crate) span: bool,
}

macro_rules! resource_ops {
    ($( $op:ident => ($receiver:literal, $runtime:ident) ),+ $(,)?) => {
        /// A method of a host resource handle. Each variant is named after
        /// the `BuiltinMember` it resolves from.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
        pub enum ResourceOp {
            $($op),+
        }

        impl ResourceOp {
            /// Every resource op, in declaration order.
            pub const ALL: &'static [ResourceOp] = &[$(ResourceOp::$op),+];

            /// The built-in type whose method this is.
            pub const fn receiver_type_name(self) -> &'static str {
                match self {
                    $(ResourceOp::$op => $receiver),+
                }
            }

            pub const fn builtin_member(self) -> BuiltinMember {
                match self {
                    $(ResourceOp::$op => BuiltinMember::$op),+
                }
            }

            pub(crate) const fn runtime_fn(self) -> RuntimeFn {
                match self {
                    $(ResourceOp::$op => RuntimeFn::$runtime),+
                }
            }

            const fn from_builtin_member(member: BuiltinMember) -> Option<Self> {
                match member {
                    $(BuiltinMember::$op => Some(ResourceOp::$op),)+
                    _ => None,
                }
            }
        }
    };
}

resource_ops! {
    FileReadAll => ("fs.File", FileReadAll),
    FileReadBytes => ("fs.File", FileReadBytes),
    FileWriteAll => ("fs.File", FileWriteAll),
    FileWriteBytes => ("fs.File", FileWriteBytes),
    FileFlush => ("fs.File", FileFlush),
    FileSync => ("fs.File", FileSync),
    FileClose => ("fs.File", FileClose),
    TcpListenerAccept => ("net.TcpListener", TcpListenerAccept),
    TcpListenerLocalAddr => ("net.TcpListener", TcpListenerLocalAddr),
    TcpListenerClose => ("net.TcpListener", TcpListenerClose),
    TcpStreamReadAll => ("net.TcpStream", TcpStreamReadAll),
    TcpStreamReadLine => ("net.TcpStream", TcpStreamReadLine),
    TcpStreamReadBytes => ("net.TcpStream", TcpStreamReadBytes),
    TcpStreamReadExact => ("net.TcpStream", TcpStreamReadExact),
    TcpStreamWriteAll => ("net.TcpStream", TcpStreamWriteAll),
    TcpStreamWriteBytes => ("net.TcpStream", TcpStreamWriteBytes),
    TcpStreamFlush => ("net.TcpStream", TcpStreamFlush),
    TcpStreamLocalAddr => ("net.TcpStream", TcpStreamLocalAddr),
    TcpStreamPeerAddr => ("net.TcpStream", TcpStreamPeerAddr),
    TcpStreamShutdownRead => ("net.TcpStream", TcpStreamShutdownRead),
    TcpStreamShutdownWrite => ("net.TcpStream", TcpStreamShutdownWrite),
    TcpStreamShutdownBoth => ("net.TcpStream", TcpStreamShutdownBoth),
    TcpStreamClose => ("net.TcpStream", TcpStreamClose),
    UdpSocketSendText => ("net.UdpSocket", UdpSocketSendText),
    UdpSocketSendBytes => ("net.UdpSocket", UdpSocketSendBytes),
    UdpSocketRecv => ("net.UdpSocket", UdpSocketRecv),
    UdpSocketRecvFrom => ("net.UdpSocket", UdpSocketRecvFrom),
    UdpSocketLocalAddr => ("net.UdpSocket", UdpSocketLocalAddr),
    UdpSocketPeerAddr => ("net.UdpSocket", UdpSocketPeerAddr),
    UdpSocketClose => ("net.UdpSocket", UdpSocketClose),
    UdpDatagramAddress => ("net.UdpDatagram", UdpDatagramAddress),
    UdpDatagramBytes => ("net.UdpDatagram", UdpDatagramBytes),
    UdpDatagramText => ("net.UdpDatagram", UdpDatagramText),
    HttpListenerAccept => ("net.HttpListener", HttpListenerAccept),
    HttpListenerLocalAddr => ("net.HttpListener", HttpListenerLocalAddr),
    HttpListenerClose => ("net.HttpListener", HttpListenerClose),
    HttpExchangeMethod => ("net.HttpExchange", HttpExchangeMethod),
    HttpExchangePath => ("net.HttpExchange", HttpExchangePath),
    HttpExchangeHeaders => ("net.HttpExchange", HttpExchangeHeaders),
    HttpExchangeBodyText => ("net.HttpExchange", HttpExchangeBodyText),
    HttpExchangeBodyBytes => ("net.HttpExchange", HttpExchangeBodyBytes),
    HttpExchangeRespondText => ("net.HttpExchange", HttpExchangeRespondText),
    HttpExchangeRespondBytes => ("net.HttpExchange", HttpExchangeRespondBytes),
    HttpResponseStatus => ("net.HttpResponse", HttpResponseStatus),
    HttpResponseReason => ("net.HttpResponse", HttpResponseReason),
    HttpResponseHeaders => ("net.HttpResponse", HttpResponseHeaders),
    HttpResponseText => ("net.HttpResponse", HttpResponseText),
    HttpResponseBytes => ("net.HttpResponse", HttpResponseBytes),
    WebSocketListenerAccept => ("net.WebSocketListener", WebsocketListenerAccept),
    WebSocketListenerLocalAddr => ("net.WebSocketListener", WebsocketListenerLocalAddr),
    WebSocketSendText => ("net.WebSocket", WebsocketSendText),
    WebSocketSendBytes => ("net.WebSocket", WebsocketSendBytes),
    WebSocketRecvText => ("net.WebSocket", WebsocketRecvText),
    WebSocketRecvBytes => ("net.WebSocket", WebsocketRecvBytes),
    WebSocketClose => ("net.WebSocket", WebsocketClose),
    UnixListenerAccept => ("net.UnixListener", UnixListenerAccept),
    UnixListenerClose => ("net.UnixListener", UnixListenerClose),
    UnixStreamReadLine => ("net.UnixStream", UnixStreamReadLine),
    UnixStreamReadExact => ("net.UnixStream", UnixStreamReadExact),
    UnixStreamWriteAll => ("net.UnixStream", UnixStreamWriteAll),
    UnixStreamClose => ("net.UnixStream", UnixStreamClose),
    TlsListenerAccept => ("net.TlsListener", TlsListenerAccept),
    TlsListenerLocalAddr => ("net.TlsListener", TlsListenerLocalAddr),
    TlsListenerClose => ("net.TlsListener", TlsListenerClose),
    TlsStreamReadLine => ("net.TlsStream", TlsStreamReadLine),
    TlsStreamReadExact => ("net.TlsStream", TlsStreamReadExact),
    TlsStreamWriteAll => ("net.TlsStream", TlsStreamWriteAll),
    TlsStreamClose => ("net.TlsStream", TlsStreamClose),
    ProcessChildStdin => ("process.Child", ProcessChildStdin),
    ProcessChildStdout => ("process.Child", ProcessChildStdout),
    ProcessChildStderr => ("process.Child", ProcessChildStderr),
    ProcessChildWait => ("process.Child", ProcessChildWait),
    ProcessChildWaitOrNone => ("process.Child", ProcessChildWaitOrNone),
    ProcessChildWaitOk => ("process.Child", ProcessChildWaitOk),
    ProcessChildKill => ("process.Child", ProcessChildKill),
    ProcessChildTerminate => ("process.Child", ProcessChildTerminate),
    ProcessChildClose => ("process.Child", ProcessChildClose),
    ProcessPipeReadAll => ("process.Pipe", ProcessPipeReadAll),
    ProcessPipeReadLine => ("process.Pipe", ProcessPipeReadLine),
    ProcessPipeReadBytes => ("process.Pipe", ProcessPipeReadBytes),
    ProcessPipeWriteAll => ("process.Pipe", ProcessPipeWriteAll),
    ProcessPipeWriteBytes => ("process.Pipe", ProcessPipeWriteBytes),
    ProcessPipeFlush => ("process.Pipe", ProcessPipeFlush),
    ProcessPipeClose => ("process.Pipe", ProcessPipeClose),
    ProcessCompletedStatus => ("process.Completed", ProcessCompletedStatus),
    ProcessCompletedSuccess => ("process.Completed", ProcessCompletedSuccess),
    ProcessCompletedStdout => ("process.Completed", ProcessCompletedStdout),
    ProcessCompletedStdoutBytes => ("process.Completed", ProcessCompletedStdoutBytes),
    ProcessCompletedStderr => ("process.Completed", ProcessCompletedStderr),
    ProcessCompletedStderrBytes => ("process.Completed", ProcessCompletedStderrBytes),
    ProcessCompletedCheck => ("process.Completed", ProcessCompletedCheck),
    ProcessSupervisorStart => ("process.Supervisor", ProcessSupervisorStart),
    ProcessSupervisorWait => ("process.Supervisor", ProcessSupervisorWait),
    ProcessSupervisorWaitOrNone => ("process.Supervisor", ProcessSupervisorWaitOrNone),
    ProcessSupervisorStop => ("process.Supervisor", ProcessSupervisorStop),
    ProcessSupervisorIsEmpty => ("process.Supervisor", ProcessSupervisorIsEmpty),
    ProcessSupervisorClose => ("process.Supervisor", ProcessSupervisorClose),
}

impl ResourceOp {
    fn result_type(self) -> Type {
        use ResourceOp::*;
        let bytes = || Type::Named("list".to_string(), vec![Type::named("uint8")]);
        let headers = || {
            Type::Named(
                "dict".to_string(),
                vec![Type::named("str"), Type::named("str")],
            )
        };
        let io = |ok: Type| Type::Named("Result".to_string(), vec![ok, Type::named("io.Error")]);
        let process =
            |ok: Type| Type::Named("Result".to_string(), vec![ok, Type::named("process.Error")]);
        let str = || Type::named("str");
        match self {
            FileClose
            | TcpListenerClose
            | TcpStreamClose
            | UdpSocketClose
            | HttpListenerClose
            | WebSocketClose
            | UnixListenerClose
            | UnixStreamClose
            | TlsListenerClose
            | TlsStreamClose
            | ProcessChildClose
            | ProcessPipeClose
            | ProcessSupervisorClose => Type::Unit,
            FileReadAll
            | TcpListenerLocalAddr
            | TcpStreamReadAll
            | TcpStreamLocalAddr
            | TcpStreamPeerAddr
            | UdpSocketLocalAddr
            | UdpSocketPeerAddr
            | UdpDatagramText
            | HttpListenerLocalAddr
            | HttpExchangeBodyText
            | HttpResponseText
            | WebSocketListenerLocalAddr
            | TlsListenerLocalAddr => io(str()),
            FileReadBytes | TcpStreamReadExact | UnixStreamReadExact | TlsStreamReadExact => {
                io(bytes())
            }
            FileWriteAll
            | FileWriteBytes
            | FileFlush
            | FileSync
            | TcpStreamWriteAll
            | TcpStreamWriteBytes
            | TcpStreamFlush
            | TcpStreamShutdownRead
            | TcpStreamShutdownWrite
            | TcpStreamShutdownBoth
            | UdpSocketSendText
            | UdpSocketSendBytes
            | HttpExchangeRespondText
            | HttpExchangeRespondBytes
            | WebSocketSendText
            | WebSocketSendBytes
            | UnixStreamWriteAll
            | TlsStreamWriteAll => io(Type::Unit),
            TcpStreamReadLine | WebSocketRecvText | UnixStreamReadLine | TlsStreamReadLine => {
                io(optional_type(str()))
            }
            TcpStreamReadBytes | UdpSocketRecv | WebSocketRecvBytes => io(optional_type(bytes())),
            TcpListenerAccept => io(Type::named("net.TcpStream")),
            UdpSocketRecvFrom => io(optional_type(Type::named("net.UdpDatagram"))),
            UdpDatagramAddress
            | HttpExchangeMethod
            | HttpExchangePath
            | HttpResponseReason
            | ProcessCompletedStdout
            | ProcessCompletedStderr => str(),
            UdpDatagramBytes
            | HttpExchangeBodyBytes
            | HttpResponseBytes
            | ProcessCompletedStdoutBytes
            | ProcessCompletedStderrBytes => bytes(),
            HttpListenerAccept => io(Type::named("net.HttpExchange")),
            HttpExchangeHeaders | HttpResponseHeaders => headers(),
            HttpResponseStatus => Type::named("int32"),
            WebSocketListenerAccept => io(Type::named("net.WebSocket")),
            UnixListenerAccept => io(Type::named("net.UnixStream")),
            TlsListenerAccept => io(Type::named("net.TlsStream")),
            ProcessChildStdin | ProcessChildStdout | ProcessChildStderr => {
                optional_type(Type::named("process.Pipe"))
            }
            ProcessChildWait => Type::named("process.Wait"),
            ProcessChildWaitOrNone => process(optional_type(Type::named("process.ExitStatus"))),
            ProcessChildWaitOk => process(Type::named("process.ExitStatus")),
            ProcessChildKill
            | ProcessChildTerminate
            | ProcessPipeWriteAll
            | ProcessPipeWriteBytes
            | ProcessPipeFlush
            | ProcessCompletedCheck
            | ProcessSupervisorStart
            | ProcessSupervisorStop => process(Type::Unit),
            ProcessPipeReadAll => process(str()),
            ProcessPipeReadLine => process(optional_type(str())),
            ProcessPipeReadBytes => process(optional_type(bytes())),
            ProcessCompletedStatus => Type::named("process.ExitStatus"),
            ProcessCompletedSuccess | ProcessSupervisorIsEmpty => Type::named("bool"),
            ProcessSupervisorWait => Type::named("process.SupervisorWait"),
            ProcessSupervisorWaitOrNone => {
                process(optional_type(Type::named("process.SupervisorEvent")))
            }
        }
    }
}

/// A method of `random.Rng`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum RngOp {
    NextInt,
    NextFloat,
    Shuffle,
}

impl RngOp {
    pub const ALL: &'static [RngOp] = &[RngOp::NextInt, RngOp::NextFloat, RngOp::Shuffle];
}

/// A method of an integer, `float64`, or `Duration` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ScalarOp {
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    SaturatingAdd,
    SaturatingSub,
    SaturatingMul,
    WrappingShl,
    WrappingShr,
    SaturatingShl,
    SaturatingShr,
    /// `to_float()` on an integer.
    ToFloat,
    /// `sqrt()` on a `float64`.
    Sqrt,
    /// `to_ms()` on a `Duration`.
    DurationToMilliseconds,
    /// `to_seconds()` on a `Duration`.
    DurationToSeconds,
}

impl ScalarOp {
    pub const ALL: &'static [ScalarOp] = &[
        ScalarOp::WrappingAdd,
        ScalarOp::WrappingSub,
        ScalarOp::WrappingMul,
        ScalarOp::SaturatingAdd,
        ScalarOp::SaturatingSub,
        ScalarOp::SaturatingMul,
        ScalarOp::WrappingShl,
        ScalarOp::WrappingShr,
        ScalarOp::SaturatingShl,
        ScalarOp::SaturatingShr,
        ScalarOp::ToFloat,
        ScalarOp::Sqrt,
        ScalarOp::DurationToMilliseconds,
        ScalarOp::DurationToSeconds,
    ];

    /// The `aura_direct_integer_width_binary` operation and arithmetic
    /// mode words of a fixed-width integer op.
    const fn integer_width_words(self) -> Option<&'static [i64]> {
        // Operation: 0 add, 1 sub, 2 mul, 3 shl, 4 shr. Mode: 1 wrapping,
        // 2 saturating.
        Some(match self {
            ScalarOp::WrappingAdd => &[0, 1],
            ScalarOp::WrappingSub => &[1, 1],
            ScalarOp::WrappingMul => &[2, 1],
            ScalarOp::WrappingShl => &[3, 1],
            ScalarOp::WrappingShr => &[4, 1],
            ScalarOp::SaturatingAdd => &[0, 2],
            ScalarOp::SaturatingSub => &[1, 2],
            ScalarOp::SaturatingMul => &[2, 2],
            ScalarOp::SaturatingShl => &[3, 2],
            ScalarOp::SaturatingShr => &[4, 2],
            ScalarOp::ToFloat
            | ScalarOp::Sqrt
            | ScalarOp::DurationToMilliseconds
            | ScalarOp::DurationToSeconds => return None,
        })
    }
}

/// `to_string`, `clone`, or `copy` on a value of concrete built-in type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ValueOp {
    /// `to_string()` on a `bool` or number.
    ToString,
    /// `clone()` on a `str`.
    Clone,
    /// `copy()` on a `list`, `dict`, or `set`.
    Copy,
}

impl ValueOp {
    pub const ALL: &'static [ValueOp] = &[ValueOp::ToString, ValueOp::Clone, ValueOp::Copy];
}

const DURATION_TO_MILLISECONDS_WORDS: &[i64] =
    &[crate::runtime_value::NANOS_PER_MILLISECOND as i64];
const DURATION_TO_SECONDS_WORDS: &[i64] = &[crate::runtime_value::NANOS_PER_SECOND as i64];

/// The built-in member that `receiver.field` names, if it names one. The
/// receiver must have a concrete named type: a type parameter, union, or
/// callable receiver names none. A class method of the receiver's type
/// shadows a built-in member, and a built-in member shadows a trait method
/// of the same name. Lowering, the MIR validator, the interpreter, and the
/// emitters all decide member precedence here.
pub(crate) fn builtin_member_for_receiver(
    receiver: &Type,
    field: &str,
    class_declares: impl FnOnce(&str) -> bool,
) -> Option<BuiltinMember> {
    let Type::Named(name, _) = receiver else {
        return None;
    };
    if class_declares(name) {
        return None;
    }
    BuiltinMember::resolve_runtime(name, field)
}

impl RuntimeOp {
    /// Every op, grouped by family.
    pub fn all() -> impl Iterator<Item = RuntimeOp> {
        ResourceOp::ALL
            .iter()
            .copied()
            .map(RuntimeOp::Resource)
            .chain(RngOp::ALL.iter().copied().map(RuntimeOp::Rng))
            .chain(ScalarOp::ALL.iter().copied().map(RuntimeOp::Scalar))
            .chain(ValueOp::ALL.iter().copied().map(RuntimeOp::Value))
    }

    /// The op a call `receiver.field` resolves to, under the member
    /// precedence of `builtin_member_for_receiver`. `class_declares` says
    /// whether the class named by the receiver type declares `field`.
    pub(crate) fn resolve_member(
        receiver: &Type,
        field: &str,
        class_declares: impl FnOnce(&str) -> bool,
    ) -> Option<Self> {
        builtin_member_for_receiver(receiver, field, class_declares)
            .and_then(Self::from_builtin_member)
    }

    /// The op a resolved built-in member is, when its family is resolved.
    pub fn from_builtin_member(member: BuiltinMember) -> Option<Self> {
        use BuiltinMember as M;
        if let Some(op) = ResourceOp::from_builtin_member(member) {
            return Some(RuntimeOp::Resource(op));
        }
        Some(match member {
            M::RngNextInt => RuntimeOp::Rng(RngOp::NextInt),
            M::RngNextFloat => RuntimeOp::Rng(RngOp::NextFloat),
            M::RngShuffle => RuntimeOp::Rng(RngOp::Shuffle),
            M::IntegerWrappingAdd => RuntimeOp::Scalar(ScalarOp::WrappingAdd),
            M::IntegerWrappingSub => RuntimeOp::Scalar(ScalarOp::WrappingSub),
            M::IntegerWrappingMul => RuntimeOp::Scalar(ScalarOp::WrappingMul),
            M::IntegerSaturatingAdd => RuntimeOp::Scalar(ScalarOp::SaturatingAdd),
            M::IntegerSaturatingSub => RuntimeOp::Scalar(ScalarOp::SaturatingSub),
            M::IntegerSaturatingMul => RuntimeOp::Scalar(ScalarOp::SaturatingMul),
            M::IntegerWrappingShl => RuntimeOp::Scalar(ScalarOp::WrappingShl),
            M::IntegerWrappingShr => RuntimeOp::Scalar(ScalarOp::WrappingShr),
            M::IntegerSaturatingShl => RuntimeOp::Scalar(ScalarOp::SaturatingShl),
            M::IntegerSaturatingShr => RuntimeOp::Scalar(ScalarOp::SaturatingShr),
            M::IntegerToFloat => RuntimeOp::Scalar(ScalarOp::ToFloat),
            M::FloatSqrt => RuntimeOp::Scalar(ScalarOp::Sqrt),
            M::DurationToMilliseconds => RuntimeOp::Scalar(ScalarOp::DurationToMilliseconds),
            M::DurationToSeconds => RuntimeOp::Scalar(ScalarOp::DurationToSeconds),
            M::ScalarToString => RuntimeOp::Value(ValueOp::ToString),
            M::StringClone => RuntimeOp::Value(ValueOp::Clone),
            M::VecClone | M::MapClone | M::SetClone => RuntimeOp::Value(ValueOp::Copy),
            _ => return None,
        })
    }

    /// The built-in member whose call shape and receiver passing this op
    /// has. `copy()` has the same shape on every collection, so it reports
    /// the `list` member.
    pub fn builtin_member(self) -> BuiltinMember {
        use BuiltinMember as M;
        match self {
            RuntimeOp::Resource(op) => op.builtin_member(),
            RuntimeOp::Rng(RngOp::NextInt) => M::RngNextInt,
            RuntimeOp::Rng(RngOp::NextFloat) => M::RngNextFloat,
            RuntimeOp::Rng(RngOp::Shuffle) => M::RngShuffle,
            RuntimeOp::Scalar(ScalarOp::WrappingAdd) => M::IntegerWrappingAdd,
            RuntimeOp::Scalar(ScalarOp::WrappingSub) => M::IntegerWrappingSub,
            RuntimeOp::Scalar(ScalarOp::WrappingMul) => M::IntegerWrappingMul,
            RuntimeOp::Scalar(ScalarOp::SaturatingAdd) => M::IntegerSaturatingAdd,
            RuntimeOp::Scalar(ScalarOp::SaturatingSub) => M::IntegerSaturatingSub,
            RuntimeOp::Scalar(ScalarOp::SaturatingMul) => M::IntegerSaturatingMul,
            RuntimeOp::Scalar(ScalarOp::WrappingShl) => M::IntegerWrappingShl,
            RuntimeOp::Scalar(ScalarOp::WrappingShr) => M::IntegerWrappingShr,
            RuntimeOp::Scalar(ScalarOp::SaturatingShl) => M::IntegerSaturatingShl,
            RuntimeOp::Scalar(ScalarOp::SaturatingShr) => M::IntegerSaturatingShr,
            RuntimeOp::Scalar(ScalarOp::ToFloat) => M::IntegerToFloat,
            RuntimeOp::Scalar(ScalarOp::Sqrt) => M::FloatSqrt,
            RuntimeOp::Scalar(ScalarOp::DurationToMilliseconds) => M::DurationToMilliseconds,
            RuntimeOp::Scalar(ScalarOp::DurationToSeconds) => M::DurationToSeconds,
            RuntimeOp::Value(ValueOp::ToString) => M::ScalarToString,
            RuntimeOp::Value(ValueOp::Clone) => M::StringClone,
            RuntimeOp::Value(ValueOp::Copy) => M::VecClone,
        }
    }

    /// The source member name, such as `read_all`.
    pub fn member_name(self) -> &'static str {
        self.builtin_member().name()
    }

    /// Whether the op is a method call with a receiver operand.
    pub fn has_receiver(self) -> bool {
        match self {
            RuntimeOp::Resource(_)
            | RuntimeOp::Rng(_)
            | RuntimeOp::Scalar(_)
            | RuntimeOp::Value(_) => true,
        }
    }

    /// The op's parameters after the receiver, in declaration order.
    pub fn params(self) -> impl Iterator<Item = CallableParam<'static>> {
        self.builtin_member().params()
    }

    /// Whether this op applies to a receiver whose type is named `name`.
    pub fn applies_to(self, name: &str) -> bool {
        BuiltinMember::resolve_runtime(name, self.member_name()).and_then(Self::from_builtin_member)
            == Some(self)
    }

    /// The runtime helper the op calls and the immediate words that follow
    /// its receiver and argument words.
    pub(crate) fn runtime_call(self) -> RuntimeCall {
        let call = |function, immediates| RuntimeCall {
            function,
            immediates,
            span: false,
        };
        match self {
            RuntimeOp::Resource(op) => call(op.runtime_fn(), &[]),
            RuntimeOp::Rng(RngOp::NextInt) => call(RuntimeFn::RngNextInt, &[]),
            RuntimeOp::Rng(RngOp::NextFloat) => call(RuntimeFn::RngNextFloat, &[]),
            RuntimeOp::Rng(RngOp::Shuffle) => call(RuntimeFn::RngShuffle, &[]),
            RuntimeOp::Scalar(
                op @ (ScalarOp::WrappingAdd
                | ScalarOp::WrappingSub
                | ScalarOp::WrappingMul
                | ScalarOp::SaturatingAdd
                | ScalarOp::SaturatingSub
                | ScalarOp::SaturatingMul
                | ScalarOp::WrappingShl
                | ScalarOp::WrappingShr
                | ScalarOp::SaturatingShl
                | ScalarOp::SaturatingShr),
            ) => RuntimeCall {
                function: RuntimeFn::IntegerWidthBinary,
                immediates: op
                    .integer_width_words()
                    .expect("fixed-width integer ops name their operation words"),
                span: true,
            },
            RuntimeOp::Scalar(ScalarOp::ToFloat) => call(RuntimeFn::IntegerToFloat, &[]),
            RuntimeOp::Scalar(ScalarOp::Sqrt) => call(RuntimeFn::SqrtF64, &[]),
            RuntimeOp::Scalar(ScalarOp::DurationToMilliseconds) => {
                call(RuntimeFn::DurationToFloat, DURATION_TO_MILLISECONDS_WORDS)
            }
            RuntimeOp::Scalar(ScalarOp::DurationToSeconds) => {
                call(RuntimeFn::DurationToFloat, DURATION_TO_SECONDS_WORDS)
            }
            RuntimeOp::Value(ValueOp::ToString) => call(RuntimeFn::StringifyValue, &[]),
            RuntimeOp::Value(ValueOp::Clone | ValueOp::Copy) => call(RuntimeFn::CloneValue, &[]),
        }
    }

    /// The type of the op's result for a receiver of type `receiver`. This
    /// is the one result-type table for the resolved families.
    pub fn result_type(self, receiver: &Type) -> Option<Type> {
        Some(match self {
            RuntimeOp::Resource(op) => op.result_type(),
            RuntimeOp::Rng(RngOp::NextInt) => Type::named("int64"),
            RuntimeOp::Rng(RngOp::NextFloat) => Type::named("float64"),
            RuntimeOp::Rng(RngOp::Shuffle) => Type::Unit,
            RuntimeOp::Scalar(
                ScalarOp::ToFloat
                | ScalarOp::Sqrt
                | ScalarOp::DurationToMilliseconds
                | ScalarOp::DurationToSeconds,
            ) => Type::named("float64"),
            RuntimeOp::Scalar(_) | RuntimeOp::Value(ValueOp::Clone | ValueOp::Copy) => {
                receiver.clone()
            }
            RuntimeOp::Value(ValueOp::ToString) => Type::named("str"),
        })
    }
}

impl std::fmt::Display for RuntimeOp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeOp::Resource(op) => write!(formatter, "Resource.{op:?}"),
            RuntimeOp::Rng(op) => write!(formatter, "Rng.{op:?}"),
            RuntimeOp::Scalar(op) => write!(formatter, "Scalar.{op:?}"),
            RuntimeOp::Value(op) => write!(formatter, "Value.{op:?}"),
        }
    }
}

#[cfg(test)]
#[path = "runtime_ops_tests.rs"]
mod tests;
