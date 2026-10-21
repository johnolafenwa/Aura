//! Resolved runtime operations (Batch 7 step 8d).
//!
//! MIR lowering resolves a member call on a receiver of concrete built-in
//! type, such as `file.read_all()`, to one `RuntimeOp`. The op names the
//! runtime helper that implements it, the receiver family it applies to, and
//! its result type, so the interpreter and every native emitter dispatch on
//! the op instead of re-resolving the member name.
//!
//! Lowering also resolves a call to a named built-in, such as `range(10)`,
//! `Duration.ms(5)`, `fs.exists(path)`, or `print(value)`, to an op with no
//! receiver. A user function with the same name keeps its `Name` call.
//!
//! Ops are grouped by family. This table holds the resource handle
//! (`fs.File`, `process.*`, `net.*`), `random.Rng`, scalar, value, `str`,
//! `Queue`, `Task`, `TaskGroup`, named function, `print`, and host builtin
//! families. Collections are still `CallTarget::Member` calls.

use serde::{Deserialize, Serialize};

use crate::ast::ReceiverKind;
use crate::builtin_modules::{
    builtin_module_function_metadata, host_builtin_metadata, HostBuiltinMetadata,
};
use crate::call::{
    BuiltinAssociatedFunction, BuiltinClassConstructor, BuiltinFunction, BuiltinMember,
    CallableParam,
};
use crate::native_runtime::HOST_BUILTIN_SYMBOLS;
use crate::runtime_abi::RuntimeFn;
use crate::sema::{optional_type, poll_type, text_convenience_result_type, Type};

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
    /// A method of `str`.
    Str(StrOp),
    /// A method of `Queue[T]`, including the private receives that queue
    /// iteration lowers to.
    Queue(QueueOp),
    /// A method of `Task[T]`.
    Task(TaskOp),
    /// A method of `TaskGroup`.
    TaskGroup(TaskGroupOp),
    /// A named built-in function or associated function, such as `range`,
    /// `Duration.ms`, or `fs.exists`.
    Function(FunctionOp),
    /// `print(value)`, by the layout of its argument.
    Print(PrintKind),
    /// A host builtin, such as `json.parse`, called through its generic
    /// `(args, count) -> value` entry.
    Host(HostBuiltin),
}

/// The runtime helper an op calls. The emitter passes the layout words, the
/// receiver word, the argument words, the immediate words, and then, when
/// `span` is set, a source line and column word. A call names no source
/// span, so both are zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeCall {
    pub(crate) function: RuntimeFn,
    pub(crate) immediates: &'static [i64],
    pub(crate) span: bool,
    /// Words the backend derives from value layout: an `Array` dtype code,
    /// a `select` source tuple, or a fresh collection and its unused index
    /// word, which pass first, or the zero words that fill the unused
    /// argument slots of the `str` text dispatcher.
    pub(crate) layout_words: usize,
}

/// What a resolved op calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeTarget {
    /// A helper in the runtime ABI table.
    Helper(RuntimeCall),
    /// A host builtin's exported entry, which takes an argument buffer and
    /// its length.
    Host(&'static str),
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

macro_rules! str_ops {
    ($( $op:ident => ($member:ident, $runtime:ident, $words:expr, $padding:literal) ),+ $(,)?) => {
        /// A method of `str`. Each variant is named after the method it
        /// resolves from.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
        pub enum StrOp {
            $($op),+
        }

        impl StrOp {
            /// Every `str` op, in declaration order.
            pub const ALL: &'static [StrOp] = &[$(StrOp::$op),+];

            pub const fn builtin_member(self) -> BuiltinMember {
                match self {
                    $(StrOp::$op => BuiltinMember::$member),+
                }
            }

            const fn runtime_call(self) -> RuntimeCall {
                match self {
                    $(StrOp::$op => RuntimeCall {
                        function: RuntimeFn::$runtime,
                        immediates: $words,
                        span: false,
                        layout_words: $padding,
                    }),+
                }
            }

            const fn from_builtin_member(member: BuiltinMember) -> Option<Self> {
                match member {
                    $(BuiltinMember::$member => Some(StrOp::$op),)+
                    _ => None,
                }
            }
        }
    };
}

// A text-convenience method calls the shared text dispatcher with its
// opcode, the receiver, a text argument word, and a count word. The opcode
// is the method's index in `text_methods::TEXT_METHOD_NAMES`, and the
// padding is the number of the two argument words the method leaves zero.
str_ops! {
    Len => (StringLen, StringLen, &[], 0),
    ByteLen => (StringByteLen, StringByteLen, &[], 0),
    Contains => (StringContains, StringContains, &[], 0),
    StartsWith => (StringStartsWith, StringStartsWith, &[], 0),
    EndsWith => (StringEndsWith, StringEndsWith, &[], 0),
    Split => (StringSplit, StringSplit, &[], 0),
    Replace => (StringReplace, StringReplace, &[], 0),
    ToLower => (StringToLower, StringToLower, &[], 0),
    ToUpper => (StringToUpper, StringToUpper, &[], 0),
    StripPrefix => (StringStripPrefix, StringStripPrefix, &[], 0),
    StripSuffix => (StringStripSuffix, StringStripSuffix, &[], 0),
    Trim => (StringTrim, StringTrim, &[], 0),
    Join => (StringJoin, StringJoin, &[], 0),
    TrimStart => (StringTrimStart, StringTextMethod, &[0], 2),
    TrimEnd => (StringTrimEnd, StringTextMethod, &[1], 2),
    SplitOnce => (StringSplitOnce, StringTextMethod, &[2], 1),
    RsplitOnce => (StringRsplitOnce, StringTextMethod, &[3], 1),
    Lines => (StringLines, StringTextMethod, &[4], 2),
    SplitWhitespace => (StringSplitWhitespace, StringTextMethod, &[5], 2),
    Find => (StringFind, StringTextMethod, &[6], 1),
    Count => (StringCount, StringTextMethod, &[7], 1),
    Repeat => (StringRepeat, StringTextMethod, &[8], 1),
}

impl StrOp {
    /// Whether the op calls the shared text dispatcher.
    pub(crate) const fn is_text_method(self) -> bool {
        matches!(self.runtime_call().function, RuntimeFn::StringTextMethod)
    }

    fn result_type(self) -> Option<Type> {
        use StrOp::*;
        let text = || Type::named("str");
        Some(match self {
            Len | ByteLen => Type::named("int64"),
            Contains | StartsWith | EndsWith => Type::named("bool"),
            Split => Type::Named("list".to_string(), vec![text()]),
            Replace | ToLower | ToUpper | Trim | Join => text(),
            StripPrefix | StripSuffix => optional_type(text()),
            TrimStart | TrimEnd | SplitOnce | RsplitOnce | Lines | SplitWhitespace | Find
            | Count | Repeat => return text_convenience_result_type(self.builtin_member().name()),
        })
    }
}

/// A method of `Queue[T]`. A `Timeout` variant is the specialization
/// lowering picks for a call that passes a `timeout`. `GetInTaskGroup` and
/// `GetWithRegisteredProducers` are the private receives that a `for` loop
/// over a queue lowers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum QueueOp {
    Put,
    PutTimeout,
    TryPut,
    Get,
    GetTimeout,
    GetInTaskGroup,
    GetWithRegisteredProducers,
    Poll,
    PollTimeout,
    GetOr,
    GetOrTimeout,
    Close,
}

impl QueueOp {
    pub const ALL: &'static [QueueOp] = &[
        QueueOp::Put,
        QueueOp::PutTimeout,
        QueueOp::TryPut,
        QueueOp::Get,
        QueueOp::GetTimeout,
        QueueOp::GetInTaskGroup,
        QueueOp::GetWithRegisteredProducers,
        QueueOp::Poll,
        QueueOp::PollTimeout,
        QueueOp::GetOr,
        QueueOp::GetOrTimeout,
        QueueOp::Close,
    ];

    /// The member a `for` loop inside a task group receives with.
    pub const GET_IN_TASK_GROUP: &'static str = "__get_in_task_group";
    /// The member a `for` loop outside a task group receives with.
    pub const GET_WITH_REGISTERED_PRODUCERS: &'static str = "__get_with_registered_producers";

    /// The private receive a MIR member call named `field` spells.
    fn from_private_name(field: &str) -> Option<Self> {
        match field {
            QueueOp::GET_IN_TASK_GROUP => Some(QueueOp::GetInTaskGroup),
            QueueOp::GET_WITH_REGISTERED_PRODUCERS => Some(QueueOp::GetWithRegisteredProducers),
            _ => None,
        }
    }

    /// The source member of a public op; a private receive has none.
    const fn builtin_member(self) -> Option<BuiltinMember> {
        use QueueOp::*;
        Some(match self {
            Put | PutTimeout => BuiltinMember::QueuePut,
            TryPut => BuiltinMember::QueueTryPut,
            Get | GetTimeout => BuiltinMember::QueueGet,
            Poll | PollTimeout => BuiltinMember::QueuePoll,
            GetOr | GetOrTimeout => BuiltinMember::QueueGetOr,
            Close => BuiltinMember::QueueClose,
            GetInTaskGroup | GetWithRegisteredProducers => return None,
        })
    }

    const fn unspecialized(self) -> Self {
        match self {
            QueueOp::PutTimeout => QueueOp::Put,
            QueueOp::GetTimeout => QueueOp::Get,
            QueueOp::PollTimeout => QueueOp::Poll,
            QueueOp::GetOrTimeout => QueueOp::GetOr,
            other => other,
        }
    }

    const fn with_timeout(self) -> Self {
        match self {
            QueueOp::Put => QueueOp::PutTimeout,
            QueueOp::Get => QueueOp::GetTimeout,
            QueueOp::Poll => QueueOp::PollTimeout,
            QueueOp::GetOr => QueueOp::GetOrTimeout,
            other => other,
        }
    }

    const fn runtime_fn(self) -> RuntimeFn {
        match self {
            QueueOp::Put => RuntimeFn::ChannelSend,
            QueueOp::PutTimeout => RuntimeFn::ChannelSendTimeoutValue,
            QueueOp::TryPut => RuntimeFn::ChannelTrySend,
            QueueOp::Get => RuntimeFn::ChannelRecv,
            QueueOp::GetTimeout => RuntimeFn::ChannelRecvTimeoutValue,
            QueueOp::GetInTaskGroup => RuntimeFn::ChannelRecvInTaskGroup,
            QueueOp::GetWithRegisteredProducers => RuntimeFn::ChannelRecvWithRegisteredProducers,
            QueueOp::Poll => RuntimeFn::ChannelRecvOrNone,
            QueueOp::PollTimeout => RuntimeFn::ChannelRecvOrNoneTimeoutValue,
            QueueOp::GetOr => RuntimeFn::ChannelRecvOrValue,
            QueueOp::GetOrTimeout => RuntimeFn::ChannelRecvOrValueTimeoutValue,
            QueueOp::Close => RuntimeFn::ChannelClose,
        }
    }

    /// The result type on a queue of payload type `payload`.
    fn result_type(self, payload: Option<Type>) -> Option<Type> {
        use QueueOp::*;
        let payload_or_unknown = || payload.clone().unwrap_or_else(|| Type::named("Unknown"));
        Some(match self {
            Put | PutTimeout | TryPut => Type::Named(
                "Result".to_string(),
                vec![
                    Type::Unit,
                    Type::Named("SendError".to_string(), vec![payload_or_unknown()]),
                ],
            ),
            Get | GetTimeout | GetInTaskGroup | GetWithRegisteredProducers => {
                Type::Named("QueueReceive".to_string(), vec![payload_or_unknown()])
            }
            Poll | PollTimeout => poll_type(payload_or_unknown()),
            GetOr | GetOrTimeout => return payload,
            Close => Type::Unit,
        })
    }
}

/// A method of `Task[T]`. A `Timeout` variant is the specialization
/// lowering picks for a call that passes a `timeout`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum TaskOp {
    Result,
    ResultTimeout,
    Poll,
    PollTimeout,
    ResultOr,
    ResultOrTimeout,
}

impl TaskOp {
    pub const ALL: &'static [TaskOp] = &[
        TaskOp::Result,
        TaskOp::ResultTimeout,
        TaskOp::Poll,
        TaskOp::PollTimeout,
        TaskOp::ResultOr,
        TaskOp::ResultOrTimeout,
    ];

    const fn builtin_member(self) -> BuiltinMember {
        match self {
            TaskOp::Result | TaskOp::ResultTimeout => BuiltinMember::TaskResult,
            TaskOp::Poll | TaskOp::PollTimeout => BuiltinMember::TaskPoll,
            TaskOp::ResultOr | TaskOp::ResultOrTimeout => BuiltinMember::TaskResultOr,
        }
    }

    const fn unspecialized(self) -> Self {
        match self {
            TaskOp::ResultTimeout => TaskOp::Result,
            TaskOp::PollTimeout => TaskOp::Poll,
            TaskOp::ResultOrTimeout => TaskOp::ResultOr,
            other => other,
        }
    }

    const fn with_timeout(self) -> Self {
        match self {
            TaskOp::Result => TaskOp::ResultTimeout,
            TaskOp::Poll => TaskOp::PollTimeout,
            TaskOp::ResultOr => TaskOp::ResultOrTimeout,
            other => other,
        }
    }

    const fn runtime_fn(self) -> RuntimeFn {
        match self {
            TaskOp::Result => RuntimeFn::TaskJoin,
            TaskOp::ResultTimeout => RuntimeFn::TaskJoinTimeoutValue,
            TaskOp::Poll => RuntimeFn::TaskJoinOrNone,
            TaskOp::PollTimeout => RuntimeFn::TaskJoinOrNoneTimeoutValue,
            TaskOp::ResultOr => RuntimeFn::TaskJoinOrValue,
            TaskOp::ResultOrTimeout => RuntimeFn::TaskJoinOrValueTimeoutValue,
        }
    }

    /// The result type on a task whose function returns `result`.
    fn result_type(self, result: Option<Type>) -> Option<Type> {
        Some(match self {
            TaskOp::Result | TaskOp::ResultTimeout => {
                Type::Named("TaskResult".to_string(), vec![result.unwrap_or(Type::Unit)])
            }
            TaskOp::Poll | TaskOp::PollTimeout => poll_type(result.unwrap_or(Type::Unit)),
            // `result_or` yields the task's own result type, so a local
            // bound from a function-valued task result keeps its callable
            // type.
            TaskOp::ResultOr | TaskOp::ResultOrTimeout => return result,
        })
    }
}

/// A method of `TaskGroup`. Starting a task lowers to `StartTask`, so
/// `cancel` is the one member call.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum TaskGroupOp {
    Cancel,
}

impl TaskGroupOp {
    pub const ALL: &'static [TaskGroupOp] = &[TaskGroupOp::Cancel];
}

/// The first type argument of a receiver type, such as `T` of `Queue[T]`.
pub(crate) fn first_type_arg(receiver: Option<&Type>) -> Option<Type> {
    match receiver {
        Some(Type::Named(_, args)) => args.first().cloned(),
        _ => None,
    }
}

macro_rules! function_ops {
    (
        named { $( $op:ident => ($name:literal, $runtime:ident) ),+ $(,)? }
        specialized { $( $special:ident => ($special_name:literal, $special_runtime:ident) ),+ $(,)? }
    ) => {
        /// A named built-in function. Each variant is named after the call
        /// it resolves from, and a specialized variant after its base call
        /// and the argument shape lowering picked it for.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
        pub enum FunctionOp {
            $($op,)+
            $($special,)+
        }

        impl FunctionOp {
            /// Every function op, in declaration order.
            pub const ALL: &'static [FunctionOp] = &[$(FunctionOp::$op,)+ $(FunctionOp::$special,)+];

            /// The callee name a MIR `Name` call spells this op with.
            pub const fn name(self) -> &'static str {
                match self {
                    $(FunctionOp::$op => $name,)+
                    $(FunctionOp::$special => $special_name,)+
                }
            }

            const fn runtime_fn(self) -> RuntimeFn {
                match self {
                    $(FunctionOp::$op => RuntimeFn::$runtime,)+
                    $(FunctionOp::$special => RuntimeFn::$special_runtime,)+
                }
            }

            /// The unspecialized op a call to `name` resolves to.
            pub(crate) fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(FunctionOp::$op),)+
                    _ => None,
                }
            }
        }
    };
}

function_ops! {
    named {
        RngNew => ("random::Rng", RngNew),
        RandomSecureInt => ("random::secure_int", RandomSecureInt),
        RandomSecureBytes => ("random::secure_bytes", RandomSecureBytes),
        ArrayZeros => ("Array.zeros", ArrayZeros),
        ArrayFull => ("Array.full", ArrayFull),
        ArrayFromList => ("Array.from_list", ArrayFromVec),
        DurationMilliseconds => ("Duration.ms", DurationFromI64),
        DurationSeconds => ("Duration.seconds", DurationFromI64),
        DurationMinutes => ("Duration.minutes", DurationFromI64),
        ListWithCapacity => ("list.with_capacity", CollectionOperation),
        DictWithCapacity => ("dict.with_capacity", CollectionOperation),
        SetWithCapacity => ("set.with_capacity", CollectionOperation),
        ListEmpty => ("list", VecEmpty),
        DictEmpty => ("dict", MapEmpty),
        SetEmpty => ("set", SetEmpty),
        Range => ("range", RangeNew),
        QueueNew => ("Queue", ChannelNew),
        TaskGroupNew => ("TaskGroup", TaskGroupNew),
        Cancelled => ("cancelled", Cancelled),
        YieldNow => ("yield_now", YieldNow),
        Sleep => ("sleep", SleepValueVoid),
        Select => ("select", Select),
        WaitAny => ("wait_any", WaitAny),
        WaitAll => ("wait_all", WaitAll),
        Abs => ("abs", AbsValue),
        Min => ("min", MinValue),
        Max => ("max", MaxValue),
        Sqrt => ("sqrt", SqrtValue),
        Round => ("round", RoundValue),
        Divmod => ("divmod", DivmodValue),
        ParseInt32 => ("parse_int32", ParseInt32),
        ParseInt64 => ("parse_int64", ParseInt64),
        ParseFloat64 => ("parse_float64", ParseFloat64),
        MonotonicTimeMs => ("sys::monotonic_time_ms", MonotonicTimeMs),
        IoWrite => ("io::write", IoWrite),
        IoFlush => ("io::flush", IoFlush),
        IoReadLine => ("io::read_line", IoReadLine),
        FsExists => ("fs::exists", FsExists),
        FsReadToString => ("fs::read_to_string", FsReadToString),
        FsReadBytes => ("fs::read_bytes", FsReadBytes),
        FsWriteString => ("fs::write_string", FsWriteString),
        FsWriteBytes => ("fs::write_bytes", FsWriteBytes),
        FsAppendString => ("fs::append_string", FsAppendString),
        FsAppendBytes => ("fs::append_bytes", FsAppendBytes),
        FsCreateDir => ("fs::create_dir", FsCreateDir),
        FsReadDir => ("fs::read_dir", FsReadDir),
        FsRemoveFile => ("fs::remove_file", FsRemoveFile),
        FsRename => ("fs::rename", FsRename),
        FsOpen => ("fs::open", FsOpen),
        FsCreate => ("fs::create", FsCreate),
        FsAppend => ("fs::append", FsAppend),
        ProcessInherit => ("process::inherit", ProcessInherit),
        ProcessNull => ("process::null", ProcessNull),
        ProcessPipe => ("process::pipe", ProcessPipe),
        ProcessSupervisor => ("process::supervisor", ProcessSupervisor),
        ProcessStart => ("process::start", ProcessStart),
        ProcessRun => ("process::run", ProcessRun),
        NetConnect => ("net::connect", NetConnect),
        NetConnectTimeout => ("net::connect_timeout", NetConnectTimeout),
        NetListen => ("net::listen", NetListen),
        NetUdpBind => ("net::udp_bind", NetUdpBind),
        NetUnixListen => ("net::unix_listen", NetUnixListen),
        NetUnixConnect => ("net::unix_connect", NetUnixConnect),
        NetUnixConnectTimeout => ("net::unix_connect_timeout", NetUnixConnectTimeout),
        NetTlsListen => ("net::tls_listen", NetTlsListen),
        NetTlsConnect => ("net::tls_connect", NetTlsConnect),
        NetTlsConnectTimeout => ("net::tls_connect_timeout", NetTlsConnectTimeout),
        NetHttpListen => ("net::http_listen", NetHttpListen),
        NetHttpRequestText => ("net::http_request_text", NetHttpRequestText),
        NetHttpRequestTextTimeout => ("net::http_request_text_timeout", NetHttpRequestTextTimeout),
        NetHttpRequestBytes => ("net::http_request_bytes", NetHttpRequestBytes),
        NetHttpRequestBytesTimeout => ("net::http_request_bytes_timeout", NetHttpRequestBytesTimeout),
        NetWebsocketListen => ("net::websocket_listen", NetWebsocketListen),
        NetWebsocketConnect => ("net::websocket_connect", NetWebsocketConnect),
        NetWebsocketConnectTimeout => ("net::websocket_connect_timeout", NetWebsocketConnectTimeout),
    }
    specialized {
        WaitAnyTimeout => ("wait_any", WaitAnyTimeoutValue),
        WaitAllTimeout => ("wait_all", WaitAllTimeoutValue),
    }
}

const QUEUE_PARAMS: &[CallableParam<'static>] = &[CallableParam::optional("capacity")];
const TASKS_PARAMS: &[CallableParam<'static>] = &[CallableParam::required("tasks")];
const TASKS_TIMEOUT_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("tasks"),
    CallableParam::required("timeout"),
];
/// `aura_direct_collection_operation`'s reserve operation code.
const COLLECTION_RESERVE_WORDS: &[i64] = &[4];
/// The nanoseconds in one unit of a `Duration` conversion.
const NANOS_PER_MILLISECOND_WORDS: &[i64] = &[crate::runtime_value::NANOS_PER_MILLISECOND as i64];
const NANOS_PER_SECOND_WORDS: &[i64] = &[crate::runtime_value::NANOS_PER_SECOND as i64];
const NANOS_PER_MINUTE_WORDS: &[i64] = &[crate::runtime_value::NANOS_PER_MINUTE as i64];

impl FunctionOp {
    /// The op with its specialization removed: the op `name()` resolves to.
    pub const fn unspecialized(self) -> Self {
        match self {
            FunctionOp::WaitAnyTimeout => FunctionOp::WaitAny,
            FunctionOp::WaitAllTimeout => FunctionOp::WaitAll,
            other => other,
        }
    }

    /// The specialization of an unspecialized op for a call that passes a
    /// `timeout`.
    pub const fn with_timeout(self) -> Self {
        match self {
            FunctionOp::WaitAny => FunctionOp::WaitAnyTimeout,
            FunctionOp::WaitAll => FunctionOp::WaitAllTimeout,
            other => other,
        }
    }

    /// The declared signature of a builtin module function op.
    fn module_metadata(self) -> Option<&'static HostBuiltinMetadata> {
        builtin_module_function_metadata(self.name())
    }

    /// The op's parameters, in declaration order. `range` lists both of its
    /// two-argument parameters, and `select` takes its sources variadically.
    fn params(self) -> Vec<CallableParam<'static>> {
        use FunctionOp::*;
        match self {
            QueueNew => QUEUE_PARAMS.to_vec(),
            WaitAny | WaitAll => TASKS_PARAMS.to_vec(),
            WaitAnyTimeout | WaitAllTimeout => TASKS_TIMEOUT_PARAMS.to_vec(),
            TaskGroupNew | ListEmpty | DictEmpty | SetEmpty => Vec::new(),
            RngNew => BuiltinClassConstructor::RandomRng.params().collect(),
            _ => {
                if let Some(builtin) = BuiltinFunction::from_name(self.name()) {
                    return builtin.params().collect();
                }
                if let Some(associated) = self
                    .name()
                    .split_once('.')
                    .and_then(|(owner, member)| BuiltinAssociatedFunction::resolve(owner, member))
                {
                    return associated.params().collect();
                }
                self.module_metadata()
                    .map(metadata_params)
                    .unwrap_or_default()
            }
        }
    }

    fn runtime_call(self) -> RuntimeCall {
        use FunctionOp::*;
        let (immediates, span, layout_words): (&'static [i64], bool, usize) = match self {
            ArrayZeros | ArrayFull | ArrayFromList => (&[], true, 1),
            DurationMilliseconds => (NANOS_PER_MILLISECOND_WORDS, false, 0),
            DurationSeconds => (NANOS_PER_SECOND_WORDS, false, 0),
            DurationMinutes => (NANOS_PER_MINUTE_WORDS, false, 0),
            ListWithCapacity | DictWithCapacity | SetWithCapacity => {
                (COLLECTION_RESERVE_WORDS, false, 2)
            }
            Select => (&[], false, 1),
            _ => (&[], false, 0),
        };
        RuntimeCall {
            function: self.runtime_fn(),
            immediates,
            span,
            layout_words,
        }
    }

    /// The helper that makes the empty collection a collection op returns.
    pub(crate) const fn empty_collection_fn(self) -> Option<RuntimeFn> {
        match self {
            FunctionOp::ListEmpty | FunctionOp::ListWithCapacity => Some(RuntimeFn::VecEmpty),
            FunctionOp::DictEmpty | FunctionOp::DictWithCapacity => Some(RuntimeFn::MapEmpty),
            FunctionOp::SetEmpty | FunctionOp::SetWithCapacity => Some(RuntimeFn::SetEmpty),
            _ => None,
        }
    }

    /// The result type for arguments of types `args`, in call order. An op
    /// whose result type the call's type arguments name, such as `Queue` or
    /// `Array.zeros`, has none here.
    fn result_type(self, args: &[Option<Type>]) -> Option<Type> {
        use FunctionOp::*;
        let first = || args.first().cloned().flatten();
        let named = |name: &str, args: Vec<Type>| Type::Named(name.to_string(), args);
        let parsed = |ok: &str| named("Result", vec![Type::named(ok), Type::named("str")]);
        Some(match self {
            RngNew => Type::named("random.Rng"),
            DurationMilliseconds | DurationSeconds | DurationMinutes => Type::named("Duration"),
            Range => Type::named("Range"),
            TaskGroupNew => Type::named("TaskGroup"),
            Cancelled => Type::named("bool"),
            YieldNow | Sleep => Type::Unit,
            Select => {
                let mut queue_payload = None;
                let mut task_result = None;
                for argument in args.iter().flatten() {
                    match argument {
                        Type::Named(source, payload) if payload.len() == 1 && source == "Queue" => {
                            queue_payload = Some(payload[0].clone());
                        }
                        Type::Named(source, result) if result.len() == 1 && source == "Task" => {
                            task_result = Some(result[0].clone());
                        }
                        _ => {}
                    }
                }
                named(
                    "SelectOutcome",
                    vec![
                        queue_payload.unwrap_or(Type::Unit),
                        task_result.unwrap_or(Type::Unit),
                    ],
                )
            }
            WaitAny | WaitAnyTimeout | WaitAll | WaitAllTimeout => {
                let Some(Type::Named(container, tasks)) = first() else {
                    return None;
                };
                let [Type::Named(task, result)] = tasks.as_slice() else {
                    return None;
                };
                if container != "list" || task != "Task" || result.len() != 1 {
                    return None;
                }
                let outcome = match self.unspecialized() {
                    WaitAny => "WaitAny",
                    _ => "WaitAll",
                };
                named(outcome, vec![result[0].clone()])
            }
            Abs | Min | Max | Sqrt => first()?,
            Round => match first()? {
                Type::Named(name, type_args)
                    if type_args.is_empty() && matches!(name.as_str(), "float32" | "float64") =>
                {
                    Type::named("int64")
                }
                other => other,
            },
            Divmod => {
                let ty = first()?;
                Type::Tuple(vec![ty.clone(), ty])
            }
            ParseInt32 => parsed("int32"),
            ParseInt64 => parsed("int64"),
            ParseFloat64 => parsed("float64"),
            ArrayZeros | ArrayFull | ArrayFromList | ListWithCapacity | DictWithCapacity
            | SetWithCapacity | ListEmpty | DictEmpty | SetEmpty | QueueNew => return None,
            _ => self.module_metadata()?.return_type.clone(),
        })
    }
}

fn metadata_params(metadata: &'static HostBuiltinMetadata) -> Vec<CallableParam<'static>> {
    metadata
        .params
        .iter()
        .map(|param| CallableParam {
            name: param.name.as_str(),
            required: param.required,
            keyword_only: false,
        })
        .collect()
}

/// How `print` writes its argument: a scalar word of one numeric kind or a
/// `bool`, or a boxed value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum PrintKind {
    /// An `int32` or `int64`.
    Int,
    Uint64,
    Float32,
    Float64,
    Bool,
    /// Any other value, boxed.
    Value,
}

impl PrintKind {
    pub const ALL: &'static [PrintKind] = &[
        PrintKind::Int,
        PrintKind::Uint64,
        PrintKind::Float32,
        PrintKind::Float64,
        PrintKind::Bool,
        PrintKind::Value,
    ];

    /// The kind that prints a value of type `ty`, or of unknown type.
    pub fn for_type(ty: Option<&Type>) -> Self {
        let Some(Type::Named(name, args)) = ty else {
            return PrintKind::Value;
        };
        if !args.is_empty() {
            return PrintKind::Value;
        }
        match name.as_str() {
            "int32" | "int64" => PrintKind::Int,
            "uint64" => PrintKind::Uint64,
            "float32" => PrintKind::Float32,
            "float64" => PrintKind::Float64,
            "bool" => PrintKind::Bool,
            _ => PrintKind::Value,
        }
    }

    const fn runtime_fn(self) -> RuntimeFn {
        match self {
            PrintKind::Int => RuntimeFn::PrintI64,
            PrintKind::Uint64 => RuntimeFn::PrintU64,
            PrintKind::Float32 => RuntimeFn::PrintF32,
            PrintKind::Float64 => RuntimeFn::PrintF64,
            PrintKind::Bool => RuntimeFn::PrintBool,
            PrintKind::Value => RuntimeFn::PrintValue,
        }
    }
}

/// A host builtin: an index into the runtime's host builtin table. It
/// serializes as the builtin's qualified name, such as `json::parse`, so
/// serialized MIR does not depend on the table's order, and the native
/// emitter's reachability walk sees the builtin's wrapper name.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct HostBuiltin(u16);

impl HostBuiltin {
    /// Every host builtin a call can resolve to, in table order. A table
    /// entry whose name a `FunctionOp` claims, such as
    /// `sys::monotonic_time_ms`, has a dedicated helper instead.
    pub fn all() -> impl Iterator<Item = HostBuiltin> {
        (0..HOST_BUILTIN_SYMBOLS.len() as u16)
            .map(HostBuiltin)
            .filter(|builtin| FunctionOp::from_name(builtin.name()).is_none())
    }

    /// The host builtin a call to the qualified name `name` resolves to.
    pub fn from_name(name: &str) -> Option<Self> {
        if FunctionOp::from_name(name).is_some() {
            return None;
        }
        HOST_BUILTIN_SYMBOLS
            .iter()
            .position(|(entry, _)| *entry == name)
            .map(|index| HostBuiltin(index as u16))
    }

    /// The builtin's qualified name, such as `json::parse`.
    pub fn name(self) -> &'static str {
        HOST_BUILTIN_SYMBOLS[usize::from(self.0)].0
    }

    /// The exported runtime entry the native emitter calls.
    pub fn symbol(self) -> &'static str {
        HOST_BUILTIN_SYMBOLS[usize::from(self.0)].1
    }

    /// The builtin's declared parameters and result type.
    pub(crate) fn metadata(self) -> &'static HostBuiltinMetadata {
        host_builtin_metadata(self.name()).expect("every host builtin entry has metadata")
    }
}

impl Serialize for HostBuiltin {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for HostBuiltin {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        match HostBuiltin::from_name(&name) {
            Some(builtin) => Ok(builtin),
            None => Err(serde::de::Error::custom(format!(
                "unknown host builtin `{name}`"
            ))),
        }
    }
}

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

/// The op a call to the built-in named `name` resolves to, before lowering
/// specializes it by its arguments: `print` resolves to `Print(Value)` and
/// `wait_any` to `Function(WaitAny)`. A user function named `name` shadows
/// the built-in, as a class method shadows a built-in member. Lowering, the
/// MIR validator, and the interpreter all decide named-call precedence here.
pub(crate) fn builtin_function_for_name(
    name: &str,
    declares_function: impl FnOnce(&str) -> bool,
) -> Option<RuntimeOp> {
    let op = if name == "print" {
        RuntimeOp::Print(PrintKind::Value)
    } else if let Some(function) = FunctionOp::from_name(name) {
        RuntimeOp::Function(function)
    } else {
        RuntimeOp::Host(HostBuiltin::from_name(name)?)
    };
    if declares_function(name) {
        return None;
    }
    Some(op)
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
            .chain(StrOp::ALL.iter().copied().map(RuntimeOp::Str))
            .chain(QueueOp::ALL.iter().copied().map(RuntimeOp::Queue))
            .chain(TaskOp::ALL.iter().copied().map(RuntimeOp::Task))
            .chain(TaskGroupOp::ALL.iter().copied().map(RuntimeOp::TaskGroup))
            .chain(FunctionOp::ALL.iter().copied().map(RuntimeOp::Function))
            .chain(PrintKind::ALL.iter().copied().map(RuntimeOp::Print))
            .chain(HostBuiltin::all().map(RuntimeOp::Host))
    }

    /// The op a call `receiver.field` resolves to, under the member
    /// precedence of `builtin_member_for_receiver`. `class_declares` says
    /// whether the class named by the receiver type declares `field`. A
    /// private queue receive resolves by its MIR spelling. The op is
    /// unspecialized; lowering picks a timeout form from the arguments.
    pub(crate) fn resolve_member(
        receiver: &Type,
        field: &str,
        class_declares: impl FnOnce(&str) -> bool,
    ) -> Option<Self> {
        if let (Type::Named(name, _), Some(op)) = (receiver, QueueOp::from_private_name(field)) {
            return (name == "Queue" && !class_declares(name)).then_some(RuntimeOp::Queue(op));
        }
        builtin_member_for_receiver(receiver, field, class_declares)
            .and_then(Self::from_builtin_member)
    }

    /// The op a resolved built-in member is, when its family is resolved.
    pub fn from_builtin_member(member: BuiltinMember) -> Option<Self> {
        use BuiltinMember as M;
        if let Some(op) = ResourceOp::from_builtin_member(member) {
            return Some(RuntimeOp::Resource(op));
        }
        if let Some(op) = StrOp::from_builtin_member(member) {
            return Some(RuntimeOp::Str(op));
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
            M::QueuePut => RuntimeOp::Queue(QueueOp::Put),
            M::QueueTryPut => RuntimeOp::Queue(QueueOp::TryPut),
            M::QueueGet => RuntimeOp::Queue(QueueOp::Get),
            M::QueuePoll => RuntimeOp::Queue(QueueOp::Poll),
            M::QueueGetOr => RuntimeOp::Queue(QueueOp::GetOr),
            M::QueueClose => RuntimeOp::Queue(QueueOp::Close),
            M::TaskResult => RuntimeOp::Task(TaskOp::Result),
            M::TaskPoll => RuntimeOp::Task(TaskOp::Poll),
            M::TaskResultOr => RuntimeOp::Task(TaskOp::ResultOr),
            M::TaskGroupCancel => RuntimeOp::TaskGroup(TaskGroupOp::Cancel),
            _ => return None,
        })
    }

    /// The built-in member whose call shape and receiver passing this op
    /// has, for an op with a receiver. `copy()` has the same shape on every
    /// collection, so it reports the `list` member, and a timeout form
    /// reports its base member. A private queue receive has none.
    pub fn builtin_member(self) -> Option<BuiltinMember> {
        use BuiltinMember as M;
        Some(match self {
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
            RuntimeOp::Str(op) => op.builtin_member(),
            RuntimeOp::Queue(op) => return op.builtin_member(),
            RuntimeOp::Task(op) => op.builtin_member(),
            RuntimeOp::TaskGroup(TaskGroupOp::Cancel) => M::TaskGroupCancel,
            RuntimeOp::Function(_) | RuntimeOp::Print(_) | RuntimeOp::Host(_) => return None,
        })
    }

    /// The source name: a member name such as `read_all` for an op with a
    /// receiver, or the callee name a MIR `Name` call spells, such as
    /// `print` or `fs::exists`.
    pub fn name(self) -> &'static str {
        match self {
            RuntimeOp::Function(op) => op.name(),
            RuntimeOp::Print(_) => "print",
            RuntimeOp::Host(builtin) => builtin.name(),
            RuntimeOp::Queue(QueueOp::GetInTaskGroup) => QueueOp::GET_IN_TASK_GROUP,
            RuntimeOp::Queue(QueueOp::GetWithRegisteredProducers) => {
                QueueOp::GET_WITH_REGISTERED_PRODUCERS
            }
            _ => self
                .builtin_member()
                .expect("an op with a receiver has a built-in member")
                .name(),
        }
    }

    /// Whether the op is a method call with a receiver operand.
    pub fn has_receiver(self) -> bool {
        match self {
            RuntimeOp::Resource(_)
            | RuntimeOp::Rng(_)
            | RuntimeOp::Scalar(_)
            | RuntimeOp::Value(_)
            | RuntimeOp::Str(_)
            | RuntimeOp::Queue(_)
            | RuntimeOp::Task(_)
            | RuntimeOp::TaskGroup(_) => true,
            RuntimeOp::Function(_) | RuntimeOp::Print(_) | RuntimeOp::Host(_) => false,
        }
    }

    /// The op with any specialization by argument shape removed: the op
    /// `builtin_function_for_name` resolves a named call to, or
    /// `resolve_member` resolves a member call to.
    pub fn unspecialized(self) -> Self {
        match self {
            RuntimeOp::Function(op) => RuntimeOp::Function(op.unspecialized()),
            RuntimeOp::Print(_) => RuntimeOp::Print(PrintKind::Value),
            RuntimeOp::Queue(op) => RuntimeOp::Queue(op.unspecialized()),
            RuntimeOp::Task(op) => RuntimeOp::Task(op.unspecialized()),
            other => other,
        }
    }

    /// The specialization of an unspecialized op for a call that passes a
    /// `timeout`, or the op itself when it has no timeout form.
    pub fn with_timeout(self) -> Self {
        match self {
            RuntimeOp::Function(op) => RuntimeOp::Function(op.with_timeout()),
            RuntimeOp::Queue(op) => RuntimeOp::Queue(op.with_timeout()),
            RuntimeOp::Task(op) => RuntimeOp::Task(op.with_timeout()),
            other => other,
        }
    }

    /// The op's parameters after the receiver, in declaration order. A
    /// member op with a timeout form takes `timeout` only in that form,
    /// where it is required.
    pub fn params(self) -> Vec<CallableParam<'static>> {
        match self {
            RuntimeOp::Function(op) => op.params(),
            RuntimeOp::Print(_) => BuiltinFunction::Print.params().collect(),
            RuntimeOp::Host(builtin) => metadata_params(builtin.metadata()),
            RuntimeOp::Queue(QueueOp::GetInTaskGroup) => vec![CallableParam::required("group")],
            RuntimeOp::Queue(QueueOp::GetWithRegisteredProducers) => Vec::new(),
            _ => {
                let params = self
                    .builtin_member()
                    .expect("an op with a receiver has a built-in member")
                    .params();
                let base = self.unspecialized();
                if base.with_timeout() == base {
                    return params.collect();
                }
                let timeout = self != base;
                params
                    .filter(|param| timeout || param.name != "timeout")
                    .map(|param| CallableParam {
                        required: param.required || param.name == "timeout",
                        ..param
                    })
                    .collect()
            }
        }
    }

    /// How the op passes the argument at `index`: `Value` for an `own`
    /// parameter, which the call takes.
    pub(crate) fn argument_passing(self, index: usize) -> Option<ReceiverKind> {
        match self.builtin_member() {
            Some(member) => member.argument_passing(index),
            None => Some(ReceiverKind::Borrow),
        }
    }

    /// Whether this op applies to a receiver whose type is named `name`.
    /// An op with no receiver applies to none.
    pub fn applies_to(self, name: &str) -> bool {
        self.has_receiver()
            && Self::resolve_member(&Type::named(name), self.name(), |_| false)
                == Some(self.unspecialized())
    }

    /// What the op calls: a runtime helper and the immediate words that
    /// follow its receiver and argument words, or a host builtin's entry.
    pub(crate) fn runtime_target(self) -> RuntimeTarget {
        let call = |function, immediates| {
            RuntimeTarget::Helper(RuntimeCall {
                function,
                immediates,
                span: false,
                layout_words: 0,
            })
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
            ) => RuntimeTarget::Helper(RuntimeCall {
                function: RuntimeFn::IntegerWidthBinary,
                immediates: op
                    .integer_width_words()
                    .expect("fixed-width integer ops name their operation words"),
                span: true,
                layout_words: 0,
            }),
            RuntimeOp::Scalar(ScalarOp::ToFloat) => call(RuntimeFn::IntegerToFloat, &[]),
            RuntimeOp::Scalar(ScalarOp::Sqrt) => call(RuntimeFn::SqrtF64, &[]),
            RuntimeOp::Scalar(ScalarOp::DurationToMilliseconds) => {
                call(RuntimeFn::DurationToFloat, NANOS_PER_MILLISECOND_WORDS)
            }
            RuntimeOp::Scalar(ScalarOp::DurationToSeconds) => {
                call(RuntimeFn::DurationToFloat, NANOS_PER_SECOND_WORDS)
            }
            RuntimeOp::Value(ValueOp::ToString) => call(RuntimeFn::StringifyValue, &[]),
            RuntimeOp::Value(ValueOp::Clone | ValueOp::Copy) => call(RuntimeFn::CloneValue, &[]),
            RuntimeOp::Str(op) => RuntimeTarget::Helper(op.runtime_call()),
            RuntimeOp::Queue(op) => call(op.runtime_fn(), &[]),
            RuntimeOp::Task(op) => call(op.runtime_fn(), &[]),
            RuntimeOp::TaskGroup(TaskGroupOp::Cancel) => call(RuntimeFn::TaskGroupCancel, &[]),
            RuntimeOp::Function(op) => RuntimeTarget::Helper(op.runtime_call()),
            RuntimeOp::Print(kind) => call(kind.runtime_fn(), &[]),
            RuntimeOp::Host(builtin) => RuntimeTarget::Host(builtin.symbol()),
        }
    }

    /// The runtime helper an op that calls one calls. A host builtin calls
    /// its own entry instead; see `runtime_target`.
    pub(crate) fn runtime_call(self) -> RuntimeCall {
        match self.runtime_target() {
            RuntimeTarget::Helper(call) => call,
            RuntimeTarget::Host(symbol) => {
                unreachable!("host builtin `{self}` calls its own entry `{symbol}`")
            }
        }
    }

    /// The type of the op's result. `receiver` is the receiver's type, for
    /// an op with a receiver, and `args` are the argument types in call
    /// order, which only named ops read. This is the one result-type table
    /// for the resolved families. An op whose result type only the call's
    /// type arguments name, such as `Queue[int64]()`, has none.
    pub fn result_type(self, receiver: Option<&Type>, args: &[Option<Type>]) -> Option<Type> {
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
                receiver?.clone()
            }
            RuntimeOp::Value(ValueOp::ToString) => Type::named("str"),
            RuntimeOp::Str(op) => return op.result_type(),
            RuntimeOp::Queue(op) => return op.result_type(first_type_arg(receiver)),
            RuntimeOp::Task(op) => return op.result_type(first_type_arg(receiver)),
            RuntimeOp::TaskGroup(TaskGroupOp::Cancel) => Type::Unit,
            RuntimeOp::Function(op) => return op.result_type(args),
            RuntimeOp::Print(_) => Type::Unit,
            RuntimeOp::Host(builtin) => builtin.metadata().return_type.clone(),
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
            RuntimeOp::Str(op) => write!(formatter, "Str.{op:?}"),
            RuntimeOp::Queue(op) => write!(formatter, "Queue.{op:?}"),
            RuntimeOp::Task(op) => write!(formatter, "Task.{op:?}"),
            RuntimeOp::TaskGroup(op) => write!(formatter, "TaskGroup.{op:?}"),
            RuntimeOp::Function(op) => write!(formatter, "Function.{op:?}"),
            RuntimeOp::Print(kind) => write!(formatter, "Print.{kind:?}"),
            RuntimeOp::Host(builtin) => write!(formatter, "Host.{}", builtin.name()),
        }
    }
}

#[cfg(test)]
#[path = "runtime_ops_tests.rs"]
mod tests;
