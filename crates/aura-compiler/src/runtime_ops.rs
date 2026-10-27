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
//! Ops are grouped by family: resource handles (`fs.File`, `process.*`,
//! `net.*`), `random.Rng`, scalars, values, `str`, `list`, `dict`, `set`,
//! `Array`, span ranges, `Queue`, `Task`, `TaskGroup`, named functions,
//! `print`, and host builtins. The private members that indexing, slicing,
//! iteration, and spans lower to, such as `__index`, resolve by their MIR
//! spelling.

use serde::{Deserialize, Serialize};

use crate::ast::ReceiverKind;
use crate::builtin_modules::{
    builtin_module_function_metadata, host_builtin_metadata, HostBuiltinMetadata,
};
use crate::call::{
    BuiltinAssociatedFunction, BuiltinClassConstructor, BuiltinFunction, BuiltinMember,
    CallableParam,
};
use crate::mir::ownership::OperandRole;
use crate::native_runtime::HOST_BUILTIN_SYMBOLS;
use crate::runtime_abi::RuntimeFn;
use crate::sema::{lookup_type, optional_type, poll_type, text_convenience_result_type, Type};

/// A default argument value that has no constant operand spelling. MIR
/// writes it as `Operand::Default` in the slot of the optional parameter a
/// runtime op call leaves out (see `RuntimeOp::default_arg`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ArgDefault {
    /// The helper applies the parameter's default itself, such as no
    /// deadline for an omitted `timeout`. A backend passes a zero word.
    Omitted,
    /// An `int64` constant.
    Int64(i64),
    /// An `int32` constant.
    Int32(i32),
    /// An empty `dict[str, str]`.
    EmptyStrDict,
    /// `process.null()`.
    StdioNull,
    /// `process.inherit()`.
    StdioInherit,
    /// `process.RestartPolicy.OnFailure`.
    RestartOnFailure,
    /// A `Duration` of this many milliseconds.
    Milliseconds(i64),
}

impl ArgDefault {
    /// The value's type. An omitted value has none.
    pub fn ty(self) -> Option<Type> {
        let named = |name: &str| Type::Named(name.to_string(), Vec::new());
        Some(match self {
            ArgDefault::Omitted => return None,
            ArgDefault::Int64(_) => named("int64"),
            ArgDefault::Int32(_) => named("int32"),
            ArgDefault::EmptyStrDict => {
                Type::Named("dict".to_string(), vec![named("str"), named("str")])
            }
            ArgDefault::StdioNull | ArgDefault::StdioInherit => named("process.Stdio"),
            ArgDefault::RestartOnFailure => named("process.RestartPolicy"),
            ArgDefault::Milliseconds(_) => named("Duration"),
        })
    }
}

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
    /// A method of `list[T]`, or a private list read, write, or slice.
    List(ListOp),
    /// A method of `dict[K, V]`, or a private entry read or write.
    Dict(DictOp),
    /// A method of `set[T]`, or a private iteration read.
    Set(SetOp),
    /// A method of `Array[T]`, or a private element read, write, or slice.
    Array(ArrayOp),
    /// A private range operation of a `Span[T]` view on its base list.
    Span(SpanOp),
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
    (
        public { $( $op:ident => ($member:ident, $runtime:ident, $words:expr, $padding:literal) ),+ $(,)? }
        private { $( $private:ident => ($name:expr, $private_runtime:ident) ),+ $(,)? }
    ) => {
        /// A method of `str`. Each variant is named after the method it
        /// resolves from, and a private variant after its MIR spelling.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
        pub enum StrOp {
            $($op,)+
            $($private,)+
        }

        impl StrOp {
            /// Every `str` op, in declaration order.
            pub const ALL: &'static [StrOp] = &[$(StrOp::$op,)+ $(StrOp::$private,)+];

            /// The source method of a public op; a private op has none.
            pub const fn builtin_member(self) -> Option<BuiltinMember> {
                match self {
                    $(StrOp::$op => Some(BuiltinMember::$member),)+
                    $(StrOp::$private => None,)+
                }
            }

            const fn private_name(self) -> Option<&'static str> {
                match self {
                    $(StrOp::$private => Some($name),)+
                    _ => None,
                }
            }

            const fn runtime_call(self) -> RuntimeCall {
                match self {
                    $(StrOp::$op => RuntimeCall {
                        function: RuntimeFn::$runtime,
                        immediates: $words,
                        span: false,
                        layout_words: $padding,
                    },)+
                    $(StrOp::$private => RuntimeCall {
                        function: RuntimeFn::$private_runtime,
                        immediates: &[],
                        span: false,
                        layout_words: 0,
                    },)+
                }
            }

            const fn from_builtin_member(member: BuiltinMember) -> Option<Self> {
                match member {
                    $(BuiltinMember::$member => Some(StrOp::$op),)+
                    _ => None,
                }
            }

            fn from_private_name(field: &str) -> Option<Self> {
                $(if field == $name {
                    return Some(StrOp::$private);
                })+
                None
            }
        }
    };
}

// A text-convenience method calls the shared text dispatcher with its
// opcode, the receiver, a text argument word, and a count word. The opcode
// is the method's index in `text_methods::TEXT_METHOD_NAMES`, and the
// padding is the number of the two argument words the method leaves zero.
str_ops! {
    public {
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
    private {
        // `text[a:b]`.
        Slice => (SLICE_MEMBER, StringSlice),
    }
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
            | Count | Repeat => return text_convenience_result_type(self.builtin_member()?.name()),
            Slice => text(),
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

/// The private member a checked element read `items[i]` lowers to.
pub const INDEX_MEMBER: &str = "__index";
/// The private member an element write `items[i] = value` lowers to.
pub const SET_INDEX_MEMBER: &str = "__set_index";
/// The private member a slice `items[a:b]` lowers to.
pub const SLICE_MEMBER: &str = "__slice";
/// The private optional read a `for` loop over a borrowed collection lowers
/// to.
pub const INDEX_OPTION_MEMBER: &str = "__index_option";
/// The private consuming read a `for` loop over an owned collection lowers
/// to.
pub const TAKE_INDEX_OPTION_MEMBER: &str = "__take_index_option";
/// The private natural sort `list.sort()` lowers to when its elements have
/// a built-in order.
pub const SORT_MEMBER: &str = "__sort";
/// The private key sort `list.sort(key=...)` lowers to when its keys have a
/// built-in order.
pub const SORT_BY_KEYS_MEMBER: &str = "__sort_by_keys";
/// The private reorder that ends a sort through a user `Ord`
/// implementation.
pub const PERMUTE_MEMBER: &str = "__permute";

/// Whether `field` is one of the private sort members, which reorder the
/// list they are called on.
pub(crate) fn is_list_sort_member(field: &str) -> bool {
    matches!(field, SORT_MEMBER | SORT_BY_KEYS_MEMBER | PERMUTE_MEMBER)
}

/// How a list stores an element without a box. Lowering picks a scalar
/// specialization, such as `ListOp::PushScalar`, from the list's element
/// type, and the op passes the kind's word to the runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ScalarElement {
    Int64,
    Float64,
    Bool,
}

impl ScalarElement {
    pub const ALL: &'static [ScalarElement] = &[
        ScalarElement::Int64,
        ScalarElement::Float64,
        ScalarElement::Bool,
    ];

    /// The kind a list of `element` stores unboxed, if any.
    pub fn of(element: &Type) -> Option<Self> {
        let Type::Named(name, args) = element else {
            return None;
        };
        if !args.is_empty() {
            return None;
        }
        match name.as_str() {
            "int64" => Some(ScalarElement::Int64),
            "float64" => Some(ScalarElement::Float64),
            "bool" => Some(ScalarElement::Bool),
            _ => None,
        }
    }

    /// The runtime's kind word: 0 for `int64`, 1 for `float64`, 2 for
    /// `bool`.
    const fn words(self) -> &'static [i64] {
        match self {
            ScalarElement::Int64 => &[0],
            ScalarElement::Float64 => &[1],
            ScalarElement::Bool => &[2],
        }
    }
}

/// `aura_direct_collection_operation`'s operation words.
const COLLECTION_POP_WORDS: &[i64] = &[0];
const COLLECTION_REMOVE_WORDS: &[i64] = &[1];
const COLLECTION_INDEX_WORDS: &[i64] = &[2];
const COLLECTION_COUNT_WORDS: &[i64] = &[3];
const COLLECTION_SET_REMOVE_WORDS: &[i64] = &[5];
const COLLECTION_SET_DISCARD_WORDS: &[i64] = &[6];
const COLLECTION_SET_CLEAR_WORDS: &[i64] = &[7];

const INDEX_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("index"),
    CallableParam::required("line"),
    CallableParam::required("column"),
];
const SET_INDEX_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("index"),
    CallableParam::required("value"),
    CallableParam::required("line"),
    CallableParam::required("column"),
];
const KEY_INDEX_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("key"),
    CallableParam::required("line"),
    CallableParam::required("column"),
];
const KEY_SET_INDEX_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("key"),
    CallableParam::required("value"),
    CallableParam::required("line"),
    CallableParam::required("column"),
];
const SLICE_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("start"),
    CallableParam::required("has_start"),
    CallableParam::required("end"),
    CallableParam::required("has_end"),
    CallableParam::required("line"),
    CallableParam::required("column"),
];
const INDEX_OPTION_PARAMS: &[CallableParam<'static>] = &[CallableParam::required("index")];
const SORT_PARAMS: &[CallableParam<'static>] = &[CallableParam::required("reverse")];
const SORT_BY_KEYS_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("keys"),
    CallableParam::required("reverse"),
];
const PERMUTE_PARAMS: &[CallableParam<'static>] = &[CallableParam::required("order")];

/// A call to `function` with no immediate or span words.
const fn helper(function: RuntimeFn) -> RuntimeCall {
    RuntimeCall {
        function,
        immediates: &[],
        span: false,
        layout_words: 0,
    }
}

/// A call to `function` with immediate words and layout words.
const fn helper_with(
    function: RuntimeFn,
    immediates: &'static [i64],
    layout_words: usize,
) -> RuntimeCall {
    RuntimeCall {
        function,
        immediates,
        span: false,
        layout_words,
    }
}

/// A call to `function` with immediate words and a zero source line and
/// column.
const fn spanned(function: RuntimeFn, immediates: &'static [i64]) -> RuntimeCall {
    RuntimeCall {
        function,
        immediates,
        span: true,
        layout_words: 0,
    }
}

/// A method of `list[T]`, including the private reads, writes, and slices
/// that indexing and iteration lower to. `PushScalar`, `IndexScalar`, and
/// `SetIndexScalar` are the specializations lowering picks for a list of
/// `int64`, `float64`, or `bool`, which the runtime stores without a box.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ListOp {
    Len,
    IsEmpty,
    /// `append(value)`.
    Push,
    PushScalar(ScalarElement),
    Pop,
    Get,
    Set,
    Remove,
    /// `index(value)`.
    IndexOf,
    Count,
    Swap,
    Contains,
    Extend,
    Insert,
    Clear,
    Reverse,
    Reserve,
    /// `__index(index, line, column)`: a checked element read.
    Index,
    IndexScalar(ScalarElement),
    /// The element read an `IndexInRange` fact covers: `Index` with no
    /// bounds check. Only `mir::checks` makes it, and its validator
    /// refuses one no fact covers.
    IndexUnchecked,
    IndexScalarUnchecked(ScalarElement),
    /// `__set_index(index, value, line, column)`: a checked element write.
    SetIndex,
    SetIndexScalar(ScalarElement),
    /// `__slice(start, has_start, end, has_end, line, column)`.
    Slice,
    /// `__index_option(index)`: a borrowed iteration read.
    IndexOption,
    /// `__take_index_option(index)`: a consuming iteration read.
    TakeIndexOption,
    /// `__sort(reverse)`: a stable sort of elements with a built-in order.
    Sort,
    /// `__sort_by_keys(keys, reverse)`: a stable sort by keys with a
    /// built-in order, one key per element.
    SortByKeys,
    /// `__permute(order)`: moves the element at `order[k]` to position `k`.
    Permute,
}

impl ListOp {
    pub const ALL: &'static [ListOp] = &[
        ListOp::Len,
        ListOp::IsEmpty,
        ListOp::Push,
        ListOp::PushScalar(ScalarElement::Int64),
        ListOp::PushScalar(ScalarElement::Float64),
        ListOp::PushScalar(ScalarElement::Bool),
        ListOp::Pop,
        ListOp::Get,
        ListOp::Set,
        ListOp::Remove,
        ListOp::IndexOf,
        ListOp::Count,
        ListOp::Swap,
        ListOp::Contains,
        ListOp::Extend,
        ListOp::Insert,
        ListOp::Clear,
        ListOp::Reverse,
        ListOp::Reserve,
        ListOp::Index,
        ListOp::IndexScalar(ScalarElement::Int64),
        ListOp::IndexScalar(ScalarElement::Float64),
        ListOp::IndexScalar(ScalarElement::Bool),
        ListOp::IndexUnchecked,
        ListOp::IndexScalarUnchecked(ScalarElement::Int64),
        ListOp::IndexScalarUnchecked(ScalarElement::Float64),
        ListOp::IndexScalarUnchecked(ScalarElement::Bool),
        ListOp::SetIndex,
        ListOp::SetIndexScalar(ScalarElement::Int64),
        ListOp::SetIndexScalar(ScalarElement::Float64),
        ListOp::SetIndexScalar(ScalarElement::Bool),
        ListOp::Slice,
        ListOp::IndexOption,
        ListOp::TakeIndexOption,
        ListOp::Sort,
        ListOp::SortByKeys,
        ListOp::Permute,
    ];

    const fn builtin_member(self) -> Option<BuiltinMember> {
        use BuiltinMember as M;
        Some(match self {
            ListOp::Len => M::VecLen,
            ListOp::IsEmpty => M::VecIsEmpty,
            ListOp::Push | ListOp::PushScalar(_) => M::VecPush,
            ListOp::Pop => M::VecPop,
            ListOp::Get => M::VecGet,
            ListOp::Set => M::VecSet,
            ListOp::Remove => M::VecRemove,
            ListOp::IndexOf => M::VecIndex,
            ListOp::Count => M::VecCount,
            ListOp::Swap => M::VecSwap,
            ListOp::Contains => M::VecContains,
            ListOp::Extend => M::VecExtend,
            ListOp::Insert => M::VecInsert,
            ListOp::Clear => M::VecClear,
            ListOp::Reverse => M::VecReverse,
            ListOp::Reserve => M::VecReserve,
            ListOp::Index
            | ListOp::IndexScalar(_)
            | ListOp::IndexUnchecked
            | ListOp::IndexScalarUnchecked(_)
            | ListOp::SetIndex
            | ListOp::SetIndexScalar(_)
            | ListOp::Slice
            | ListOp::IndexOption
            | ListOp::TakeIndexOption
            | ListOp::Sort
            | ListOp::SortByKeys
            | ListOp::Permute => return None,
        })
    }

    const fn from_builtin_member(member: BuiltinMember) -> Option<Self> {
        use BuiltinMember as M;
        Some(match member {
            M::VecLen => ListOp::Len,
            M::VecIsEmpty => ListOp::IsEmpty,
            M::VecPush => ListOp::Push,
            M::VecPop => ListOp::Pop,
            M::VecGet => ListOp::Get,
            M::VecSet => ListOp::Set,
            M::VecRemove => ListOp::Remove,
            M::VecIndex => ListOp::IndexOf,
            M::VecCount => ListOp::Count,
            M::VecSwap => ListOp::Swap,
            M::VecContains => ListOp::Contains,
            M::VecExtend => ListOp::Extend,
            M::VecInsert => ListOp::Insert,
            M::VecClear => ListOp::Clear,
            M::VecReverse => ListOp::Reverse,
            M::VecReserve => ListOp::Reserve,
            _ => return None,
        })
    }

    const fn private_name(self) -> Option<&'static str> {
        Some(match self {
            ListOp::Index
            | ListOp::IndexScalar(_)
            | ListOp::IndexUnchecked
            | ListOp::IndexScalarUnchecked(_) => INDEX_MEMBER,
            ListOp::SetIndex | ListOp::SetIndexScalar(_) => SET_INDEX_MEMBER,
            ListOp::Slice => SLICE_MEMBER,
            ListOp::IndexOption => INDEX_OPTION_MEMBER,
            ListOp::TakeIndexOption => TAKE_INDEX_OPTION_MEMBER,
            ListOp::Sort => SORT_MEMBER,
            ListOp::SortByKeys => SORT_BY_KEYS_MEMBER,
            ListOp::Permute => PERMUTE_MEMBER,
            _ => return None,
        })
    }

    fn from_private_name(field: &str) -> Option<Self> {
        Some(match field {
            INDEX_MEMBER => ListOp::Index,
            SET_INDEX_MEMBER => ListOp::SetIndex,
            SLICE_MEMBER => ListOp::Slice,
            INDEX_OPTION_MEMBER => ListOp::IndexOption,
            TAKE_INDEX_OPTION_MEMBER => ListOp::TakeIndexOption,
            SORT_MEMBER => ListOp::Sort,
            SORT_BY_KEYS_MEMBER => ListOp::SortByKeys,
            PERMUTE_MEMBER => ListOp::Permute,
            _ => return None,
        })
    }

    const fn private_params(self) -> Option<&'static [CallableParam<'static>]> {
        Some(match self {
            ListOp::Index
            | ListOp::IndexScalar(_)
            | ListOp::IndexUnchecked
            | ListOp::IndexScalarUnchecked(_) => INDEX_PARAMS,
            ListOp::SetIndex | ListOp::SetIndexScalar(_) => SET_INDEX_PARAMS,
            ListOp::Slice => SLICE_PARAMS,
            ListOp::IndexOption | ListOp::TakeIndexOption => INDEX_OPTION_PARAMS,
            ListOp::Sort => SORT_PARAMS,
            ListOp::SortByKeys => SORT_BY_KEYS_PARAMS,
            ListOp::Permute => PERMUTE_PARAMS,
            _ => return None,
        })
    }

    const fn unspecialized(self) -> Self {
        match self {
            ListOp::PushScalar(_) => ListOp::Push,
            ListOp::IndexScalar(_) | ListOp::IndexUnchecked | ListOp::IndexScalarUnchecked(_) => {
                ListOp::Index
            }
            ListOp::SetIndexScalar(_) => ListOp::SetIndex,
            other => other,
        }
    }

    /// The checked form of an unchecked read, and any other op itself.
    pub const fn checked(self) -> Self {
        match self {
            ListOp::IndexUnchecked => ListOp::Index,
            ListOp::IndexScalarUnchecked(kind) => ListOp::IndexScalar(kind),
            other => other,
        }
    }

    /// The unchecked form of an element read, if the op is one.
    pub const fn unchecked(self) -> Option<Self> {
        match self {
            ListOp::Index | ListOp::IndexUnchecked => Some(ListOp::IndexUnchecked),
            ListOp::IndexScalar(kind) | ListOp::IndexScalarUnchecked(kind) => {
                Some(ListOp::IndexScalarUnchecked(kind))
            }
            _ => None,
        }
    }

    /// Whether the op is an element read with no bounds check.
    pub const fn is_unchecked(self) -> bool {
        matches!(
            self,
            ListOp::IndexUnchecked | ListOp::IndexScalarUnchecked(_)
        )
    }

    /// The helper call. A collection operation passes the receiver, a value
    /// word, an index word, and its operation word, so a call that uses one
    /// of the two leaves the other zero.
    const fn runtime_call(self) -> RuntimeCall {
        match self {
            ListOp::Len => helper(RuntimeFn::VecLen),
            ListOp::IsEmpty => helper(RuntimeFn::VecIsEmpty),
            ListOp::Push => helper(RuntimeFn::VecPushInPlace),
            ListOp::PushScalar(kind) => helper_with(RuntimeFn::VecPushScalar, kind.words(), 0),
            ListOp::Pop => helper_with(RuntimeFn::CollectionOperation, COLLECTION_POP_WORDS, 1),
            ListOp::Get => helper(RuntimeFn::VecGet),
            ListOp::Set => helper(RuntimeFn::VecSetInPlace),
            ListOp::Remove => {
                helper_with(RuntimeFn::CollectionOperation, COLLECTION_REMOVE_WORDS, 1)
            }
            ListOp::IndexOf => {
                helper_with(RuntimeFn::CollectionOperation, COLLECTION_INDEX_WORDS, 1)
            }
            ListOp::Count => helper_with(RuntimeFn::CollectionOperation, COLLECTION_COUNT_WORDS, 1),
            ListOp::Swap => helper(RuntimeFn::VecSwapInPlace),
            ListOp::Contains => helper(RuntimeFn::VecContains),
            ListOp::Extend => helper(RuntimeFn::VecExtendInPlace),
            ListOp::Insert => helper(RuntimeFn::VecInsertInPlace),
            ListOp::Clear => helper(RuntimeFn::VecClearInPlace),
            ListOp::Reverse => helper(RuntimeFn::VecReverseInPlace),
            ListOp::Reserve => {
                helper_with(RuntimeFn::CollectionOperation, COLLECTION_RESERVE_WORDS, 1)
            }
            ListOp::Index | ListOp::IndexUnchecked => helper(RuntimeFn::VecIndex),
            // The line and column reach only the bounds failure's call.
            ListOp::IndexScalar(kind) | ListOp::IndexScalarUnchecked(kind) => {
                helper_with(RuntimeFn::VecIndexScalar, kind.words(), 0)
            }
            ListOp::SetIndex => helper(RuntimeFn::VecSetIndexInPlace),
            ListOp::SetIndexScalar(kind) => {
                helper_with(RuntimeFn::VecSetIndexScalar, kind.words(), 0)
            }
            ListOp::Slice => helper(RuntimeFn::VecSlice),
            ListOp::IndexOption => helper(RuntimeFn::VecIndexOption),
            ListOp::TakeIndexOption => helper(RuntimeFn::VecTakeIndexInPlace),
            ListOp::Sort => helper(RuntimeFn::VecSortInPlace),
            ListOp::SortByKeys => helper(RuntimeFn::VecSortByKeysInPlace),
            ListOp::Permute => helper(RuntimeFn::VecPermuteInPlace),
        }
    }

    /// The result type on a list of type `receiver`.
    fn result_type(self, receiver: Option<&Type>) -> Option<Type> {
        let element = first_type_arg(receiver);
        Some(match self {
            ListOp::Len | ListOp::IndexOf | ListOp::Count => Type::named("int64"),
            ListOp::IsEmpty | ListOp::Contains => Type::named("bool"),
            ListOp::Push
            | ListOp::PushScalar(_)
            | ListOp::Remove
            | ListOp::Swap
            | ListOp::Extend
            | ListOp::Insert
            | ListOp::Clear
            | ListOp::Reverse
            | ListOp::Reserve
            | ListOp::SetIndex
            | ListOp::SetIndexScalar(_)
            | ListOp::Sort
            | ListOp::SortByKeys
            | ListOp::Permute => Type::Unit,
            ListOp::Pop
            | ListOp::Set
            | ListOp::Index
            | ListOp::IndexScalar(_)
            | ListOp::IndexUnchecked
            | ListOp::IndexScalarUnchecked(_) => return element,
            ListOp::Get | ListOp::IndexOption | ListOp::TakeIndexOption => {
                lookup_type(element.unwrap_or_else(|| Type::named("Unknown")))
            }
            ListOp::Slice => receiver?.clone(),
        })
    }
}

/// A method of `dict[K, V]`, including the private `contains_key` and `set`
/// that `in` and literal construction lower to, and the private element
/// read and write. `GetStr` is the specialization lowering picks for a
/// `get` with a literal `str` key, which the runtime looks up without a
/// box. `GetInt64` and `ContainsKeyInt64` are the specializations for a
/// dict with `int64` keys, which the runtime searches without boxing the
/// key.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum DictOp {
    Len,
    IsEmpty,
    Get,
    GetStr,
    GetInt64,
    Set,
    Remove,
    ContainsKey,
    ContainsKeyInt64,
    Keys,
    Values,
    Items,
    Clear,
    /// `update(other)`.
    Update,
    Reserve,
    /// `__index(key, line, column)`.
    Index,
    /// `__set_index(key, value, line, column)`.
    SetIndex,
}

impl DictOp {
    pub const ALL: &'static [DictOp] = &[
        DictOp::Len,
        DictOp::IsEmpty,
        DictOp::Get,
        DictOp::GetStr,
        DictOp::GetInt64,
        DictOp::Set,
        DictOp::Remove,
        DictOp::ContainsKey,
        DictOp::ContainsKeyInt64,
        DictOp::Keys,
        DictOp::Values,
        DictOp::Items,
        DictOp::Clear,
        DictOp::Update,
        DictOp::Reserve,
        DictOp::Index,
        DictOp::SetIndex,
    ];

    const fn builtin_member(self) -> Option<BuiltinMember> {
        use BuiltinMember as M;
        Some(match self {
            DictOp::Len => M::MapLen,
            DictOp::IsEmpty => M::MapIsEmpty,
            DictOp::Get | DictOp::GetStr | DictOp::GetInt64 => M::MapGet,
            DictOp::Set => M::MapSet,
            DictOp::Remove => M::MapRemove,
            DictOp::ContainsKey | DictOp::ContainsKeyInt64 => M::MapContainsKey,
            DictOp::Keys => M::MapKeys,
            DictOp::Values => M::MapValues,
            DictOp::Items => M::MapItems,
            DictOp::Clear => M::MapClear,
            DictOp::Update => M::MapExtend,
            DictOp::Reserve => M::MapReserve,
            DictOp::Index | DictOp::SetIndex => return None,
        })
    }

    const fn from_builtin_member(member: BuiltinMember) -> Option<Self> {
        use BuiltinMember as M;
        Some(match member {
            M::MapLen => DictOp::Len,
            M::MapIsEmpty => DictOp::IsEmpty,
            M::MapGet => DictOp::Get,
            M::MapSet => DictOp::Set,
            M::MapRemove => DictOp::Remove,
            M::MapContainsKey => DictOp::ContainsKey,
            M::MapKeys => DictOp::Keys,
            M::MapValues => DictOp::Values,
            M::MapItems => DictOp::Items,
            M::MapClear => DictOp::Clear,
            M::MapExtend => DictOp::Update,
            M::MapReserve => DictOp::Reserve,
            _ => return None,
        })
    }

    /// The MIR spelling of an op that has no public member of its name.
    /// `contains_key` resolves from its private spelling, though its member
    /// is named `contains`.
    const fn private_name(self) -> Option<&'static str> {
        Some(match self {
            DictOp::ContainsKey | DictOp::ContainsKeyInt64 => "contains_key",
            DictOp::Index => INDEX_MEMBER,
            DictOp::SetIndex => SET_INDEX_MEMBER,
            _ => return None,
        })
    }

    fn from_private_name(field: &str) -> Option<Self> {
        Some(match field {
            INDEX_MEMBER => DictOp::Index,
            SET_INDEX_MEMBER => DictOp::SetIndex,
            _ => return None,
        })
    }

    const fn private_params(self) -> Option<&'static [CallableParam<'static>]> {
        Some(match self {
            DictOp::Index => KEY_INDEX_PARAMS,
            DictOp::SetIndex => KEY_SET_INDEX_PARAMS,
            _ => return None,
        })
    }

    const fn unspecialized(self) -> Self {
        match self {
            DictOp::GetStr | DictOp::GetInt64 => DictOp::Get,
            DictOp::ContainsKeyInt64 => DictOp::ContainsKey,
            other => other,
        }
    }

    /// The helper call. `GetStr` passes its literal key as a pointer and a
    /// length.
    const fn runtime_call(self) -> RuntimeCall {
        match self {
            DictOp::Len => helper(RuntimeFn::MapLen),
            DictOp::IsEmpty => helper(RuntimeFn::MapIsEmpty),
            DictOp::Get => helper(RuntimeFn::MapGet),
            DictOp::GetStr => helper_with(RuntimeFn::MapGetStr, &[], 1),
            DictOp::GetInt64 => helper(RuntimeFn::MapGetInt64),
            DictOp::Set => helper(RuntimeFn::MapSetInPlace),
            DictOp::Remove => helper(RuntimeFn::MapRemoveInPlace),
            DictOp::ContainsKey => helper(RuntimeFn::MapContainsKey),
            DictOp::ContainsKeyInt64 => helper(RuntimeFn::MapContainsInt64),
            DictOp::Keys => helper(RuntimeFn::MapKeys),
            DictOp::Values => helper(RuntimeFn::MapValues),
            DictOp::Items => helper(RuntimeFn::MapItems),
            DictOp::Clear => helper(RuntimeFn::MapClearInPlace),
            DictOp::Update => helper(RuntimeFn::MapExtendInPlace),
            DictOp::Reserve => {
                helper_with(RuntimeFn::CollectionOperation, COLLECTION_RESERVE_WORDS, 1)
            }
            DictOp::Index => helper(RuntimeFn::MapIndex),
            DictOp::SetIndex => helper(RuntimeFn::MapSetIndexInPlace),
        }
    }

    /// The result type on a dict of type `receiver`.
    fn result_type(self, receiver: Option<&Type>) -> Option<Type> {
        let arg = |index: usize| match receiver {
            Some(Type::Named(_, args)) => args.get(index).cloned(),
            _ => None,
        };
        let key = || arg(0).unwrap_or_else(|| Type::named("Unknown"));
        let value = || arg(1).unwrap_or_else(|| Type::named("Unknown"));
        let list = |element: Type| Type::Named("list".to_string(), vec![element]);
        Some(match self {
            DictOp::Len => Type::named("int64"),
            DictOp::IsEmpty | DictOp::ContainsKey | DictOp::ContainsKeyInt64 => Type::named("bool"),
            DictOp::Get | DictOp::GetStr | DictOp::GetInt64 | DictOp::Set | DictOp::Remove => {
                lookup_type(value())
            }
            DictOp::Keys => list(key()),
            DictOp::Values => list(value()),
            DictOp::Items => list(Type::Tuple(vec![key(), value()])),
            DictOp::Clear | DictOp::Update | DictOp::Reserve | DictOp::SetIndex => Type::Unit,
            DictOp::Index => return arg(1),
        })
    }
}

/// A method of `set[T]`, including the private `contains` that `in` lowers
/// to and the private iteration reads. `ContainsInt64` and `AddInt64` are
/// the specializations lowering picks for a set of `int64`, which the
/// runtime searches and fills without boxing the element.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum SetOp {
    Len,
    IsEmpty,
    Contains,
    ContainsInt64,
    Add,
    AddInt64,
    Remove,
    Discard,
    Clear,
    Reserve,
    /// `__index_option(index)`: a borrowed iteration read.
    IndexOption,
    /// `__take_index_option(index)`: a consuming iteration read.
    TakeIndexOption,
}

impl SetOp {
    pub const ALL: &'static [SetOp] = &[
        SetOp::Len,
        SetOp::IsEmpty,
        SetOp::Contains,
        SetOp::ContainsInt64,
        SetOp::Add,
        SetOp::AddInt64,
        SetOp::Remove,
        SetOp::Discard,
        SetOp::Clear,
        SetOp::Reserve,
        SetOp::IndexOption,
        SetOp::TakeIndexOption,
    ];

    const fn builtin_member(self) -> Option<BuiltinMember> {
        use BuiltinMember as M;
        Some(match self {
            SetOp::Len => M::SetLen,
            SetOp::IsEmpty => M::SetIsEmpty,
            SetOp::Contains | SetOp::ContainsInt64 => M::SetContains,
            SetOp::Add | SetOp::AddInt64 => M::SetInsert,
            SetOp::Remove => M::SetRemove,
            SetOp::Discard => M::SetDiscard,
            SetOp::Clear => M::SetClear,
            SetOp::Reserve => M::SetReserve,
            SetOp::IndexOption | SetOp::TakeIndexOption => return None,
        })
    }

    const fn from_builtin_member(member: BuiltinMember) -> Option<Self> {
        use BuiltinMember as M;
        Some(match member {
            M::SetLen => SetOp::Len,
            M::SetIsEmpty => SetOp::IsEmpty,
            M::SetContains => SetOp::Contains,
            M::SetInsert => SetOp::Add,
            M::SetRemove => SetOp::Remove,
            M::SetDiscard => SetOp::Discard,
            M::SetClear => SetOp::Clear,
            M::SetReserve => SetOp::Reserve,
            _ => return None,
        })
    }

    const fn private_name(self) -> Option<&'static str> {
        Some(match self {
            SetOp::IndexOption => INDEX_OPTION_MEMBER,
            SetOp::TakeIndexOption => TAKE_INDEX_OPTION_MEMBER,
            _ => return None,
        })
    }

    fn from_private_name(field: &str) -> Option<Self> {
        Some(match field {
            INDEX_OPTION_MEMBER => SetOp::IndexOption,
            TAKE_INDEX_OPTION_MEMBER => SetOp::TakeIndexOption,
            _ => return None,
        })
    }

    const fn private_params(self) -> Option<&'static [CallableParam<'static>]> {
        match self {
            SetOp::IndexOption | SetOp::TakeIndexOption => Some(INDEX_OPTION_PARAMS),
            _ => None,
        }
    }

    const fn unspecialized(self) -> Self {
        match self {
            SetOp::ContainsInt64 => SetOp::Contains,
            SetOp::AddInt64 => SetOp::Add,
            other => other,
        }
    }

    const fn runtime_call(self) -> RuntimeCall {
        match self {
            SetOp::Len => helper(RuntimeFn::SetLen),
            SetOp::IsEmpty => helper(RuntimeFn::SetIsEmpty),
            SetOp::Contains => helper(RuntimeFn::SetContains),
            SetOp::ContainsInt64 => helper(RuntimeFn::SetContainsInt64),
            SetOp::Add => helper(RuntimeFn::SetInsertInPlace),
            SetOp::AddInt64 => helper(RuntimeFn::SetInsertInt64InPlace),
            SetOp::Remove => helper_with(
                RuntimeFn::CollectionOperation,
                COLLECTION_SET_REMOVE_WORDS,
                1,
            ),
            SetOp::Discard => helper_with(
                RuntimeFn::CollectionOperation,
                COLLECTION_SET_DISCARD_WORDS,
                1,
            ),
            // A clear passes neither a value nor an index.
            SetOp::Clear => helper_with(
                RuntimeFn::CollectionOperation,
                COLLECTION_SET_CLEAR_WORDS,
                2,
            ),
            SetOp::Reserve => {
                helper_with(RuntimeFn::CollectionOperation, COLLECTION_RESERVE_WORDS, 1)
            }
            SetOp::IndexOption => helper(RuntimeFn::SetIndexOption),
            SetOp::TakeIndexOption => helper(RuntimeFn::SetTakeIndexInPlace),
        }
    }

    /// The result type on a set of element type `element`.
    fn result_type(self, element: Option<Type>) -> Type {
        match self {
            SetOp::Len => Type::named("int64"),
            SetOp::IsEmpty | SetOp::Contains | SetOp::ContainsInt64 => Type::named("bool"),
            SetOp::Add
            | SetOp::AddInt64
            | SetOp::Remove
            | SetOp::Discard
            | SetOp::Clear
            | SetOp::Reserve => Type::Unit,
            SetOp::IndexOption | SetOp::TakeIndexOption => {
                lookup_type(element.unwrap_or_else(|| Type::named("Unknown")))
            }
        }
    }
}

/// A method of `Array[T]`, including the private element read and write
/// and the slice that indexing lowers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ArrayOp {
    Shape,
    Len,
    Clone,
    Get,
    Set,
    Fill,
    Map,
    Sum,
    Min,
    Max,
    Mean,
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    SaturatingAdd,
    SaturatingSub,
    SaturatingMul,
    /// `__index(index, line, column)`.
    Index,
    /// `__set_index(index, value, line, column)`.
    SetIndex,
    /// `__slice(start, has_start, end, has_end, line, column)`.
    Slice,
}

impl ArrayOp {
    pub const ALL: &'static [ArrayOp] = &[
        ArrayOp::Shape,
        ArrayOp::Len,
        ArrayOp::Clone,
        ArrayOp::Get,
        ArrayOp::Set,
        ArrayOp::Fill,
        ArrayOp::Map,
        ArrayOp::Sum,
        ArrayOp::Min,
        ArrayOp::Max,
        ArrayOp::Mean,
        ArrayOp::WrappingAdd,
        ArrayOp::WrappingSub,
        ArrayOp::WrappingMul,
        ArrayOp::SaturatingAdd,
        ArrayOp::SaturatingSub,
        ArrayOp::SaturatingMul,
        ArrayOp::Index,
        ArrayOp::SetIndex,
        ArrayOp::Slice,
    ];

    const fn builtin_member(self) -> Option<BuiltinMember> {
        use BuiltinMember as M;
        Some(match self {
            ArrayOp::Shape => M::ArrayShape,
            ArrayOp::Len => M::ArrayLen,
            ArrayOp::Clone => M::ArrayClone,
            ArrayOp::Get => M::ArrayGet,
            ArrayOp::Set => M::ArraySet,
            ArrayOp::Fill => M::ArrayFill,
            ArrayOp::Map => M::ArrayMap,
            ArrayOp::Sum => M::ArraySum,
            ArrayOp::Min => M::ArrayMin,
            ArrayOp::Max => M::ArrayMax,
            ArrayOp::Mean => M::ArrayMean,
            ArrayOp::WrappingAdd => M::ArrayWrappingAdd,
            ArrayOp::WrappingSub => M::ArrayWrappingSub,
            ArrayOp::WrappingMul => M::ArrayWrappingMul,
            ArrayOp::SaturatingAdd => M::ArraySaturatingAdd,
            ArrayOp::SaturatingSub => M::ArraySaturatingSub,
            ArrayOp::SaturatingMul => M::ArraySaturatingMul,
            ArrayOp::Index | ArrayOp::SetIndex | ArrayOp::Slice => return None,
        })
    }

    const fn from_builtin_member(member: BuiltinMember) -> Option<Self> {
        use BuiltinMember as M;
        Some(match member {
            M::ArrayShape => ArrayOp::Shape,
            M::ArrayLen => ArrayOp::Len,
            M::ArrayClone => ArrayOp::Clone,
            M::ArrayGet => ArrayOp::Get,
            M::ArraySet => ArrayOp::Set,
            M::ArrayFill => ArrayOp::Fill,
            M::ArrayMap => ArrayOp::Map,
            M::ArraySum => ArrayOp::Sum,
            M::ArrayMin => ArrayOp::Min,
            M::ArrayMax => ArrayOp::Max,
            M::ArrayMean => ArrayOp::Mean,
            M::ArrayWrappingAdd => ArrayOp::WrappingAdd,
            M::ArrayWrappingSub => ArrayOp::WrappingSub,
            M::ArrayWrappingMul => ArrayOp::WrappingMul,
            M::ArraySaturatingAdd => ArrayOp::SaturatingAdd,
            M::ArraySaturatingSub => ArrayOp::SaturatingSub,
            M::ArraySaturatingMul => ArrayOp::SaturatingMul,
            _ => return None,
        })
    }

    const fn private_name(self) -> Option<&'static str> {
        Some(match self {
            ArrayOp::Index => INDEX_MEMBER,
            ArrayOp::SetIndex => SET_INDEX_MEMBER,
            ArrayOp::Slice => SLICE_MEMBER,
            _ => return None,
        })
    }

    fn from_private_name(field: &str) -> Option<Self> {
        Some(match field {
            INDEX_MEMBER => ArrayOp::Index,
            SET_INDEX_MEMBER => ArrayOp::SetIndex,
            SLICE_MEMBER => ArrayOp::Slice,
            _ => return None,
        })
    }

    const fn private_params(self) -> Option<&'static [CallableParam<'static>]> {
        Some(match self {
            ArrayOp::Index => INDEX_PARAMS,
            ArrayOp::SetIndex => SET_INDEX_PARAMS,
            ArrayOp::Slice => SLICE_PARAMS,
            _ => return None,
        })
    }

    /// The helper call. A public member passes a zero source line and
    /// column, and `map` passes its result dtype code after its callback.
    /// A reduction passes its kind word, 0 sum, 1 min, 2 max, 3 mean. An
    /// arithmetic method passes a zero scalar-left word, its operation word,
    /// 0 add, 1 sub, 2 mul, and its mode word, 1 wrapping, 2 saturating.
    const fn runtime_call(self) -> RuntimeCall {
        match self {
            ArrayOp::Shape => helper(RuntimeFn::ArrayShape),
            ArrayOp::Len => helper(RuntimeFn::ArrayLen),
            ArrayOp::Clone => spanned(RuntimeFn::ArrayClone, &[]),
            ArrayOp::Get => spanned(RuntimeFn::ArrayGet, &[]),
            ArrayOp::Set => spanned(RuntimeFn::ArraySetInPlace, &[]),
            ArrayOp::Fill => spanned(RuntimeFn::ArrayFillInPlace, &[]),
            ArrayOp::Map => RuntimeCall {
                function: RuntimeFn::ArrayMap,
                immediates: &[],
                span: true,
                layout_words: 1,
            },
            ArrayOp::Sum => spanned(RuntimeFn::ArrayReduce, &[0]),
            ArrayOp::Min => spanned(RuntimeFn::ArrayReduce, &[1]),
            ArrayOp::Max => spanned(RuntimeFn::ArrayReduce, &[2]),
            ArrayOp::Mean => spanned(RuntimeFn::ArrayReduce, &[3]),
            ArrayOp::WrappingAdd => spanned(RuntimeFn::ArrayBinary, &[0, 0, 1]),
            ArrayOp::WrappingSub => spanned(RuntimeFn::ArrayBinary, &[0, 1, 1]),
            ArrayOp::WrappingMul => spanned(RuntimeFn::ArrayBinary, &[0, 2, 1]),
            ArrayOp::SaturatingAdd => spanned(RuntimeFn::ArrayBinary, &[0, 0, 2]),
            ArrayOp::SaturatingSub => spanned(RuntimeFn::ArrayBinary, &[0, 1, 2]),
            ArrayOp::SaturatingMul => spanned(RuntimeFn::ArrayBinary, &[0, 2, 2]),
            ArrayOp::Index => helper(RuntimeFn::ArrayIndex),
            ArrayOp::SetIndex => helper(RuntimeFn::ArraySetIndexInPlace),
            ArrayOp::Slice => helper(RuntimeFn::ArraySlice),
        }
    }

    /// The result type on an `Array` of type `receiver`. `map` names its
    /// callback's result dtype, so its type comes from its argument.
    fn result_type(self, receiver: Option<&Type>, args: &[Option<Type>]) -> Option<Type> {
        let dtype = first_type_arg(receiver);
        let array = |dtype: Vec<Type>| Type::Named("Array".to_string(), dtype);
        Some(match self {
            ArrayOp::Shape => Type::Named("list".to_string(), vec![Type::named("int64")]),
            ArrayOp::Len => Type::named("int64"),
            ArrayOp::Clone
            | ArrayOp::WrappingAdd
            | ArrayOp::WrappingSub
            | ArrayOp::WrappingMul
            | ArrayOp::SaturatingAdd
            | ArrayOp::SaturatingSub
            | ArrayOp::SaturatingMul
            | ArrayOp::Slice => receiver?.clone(),
            ArrayOp::Get | ArrayOp::Set => {
                optional_type(dtype.unwrap_or_else(|| Type::named("Unknown")))
            }
            ArrayOp::Fill | ArrayOp::SetIndex => Type::Unit,
            ArrayOp::Sum | ArrayOp::Min | ArrayOp::Max | ArrayOp::Index => return dtype,
            ArrayOp::Mean => Type::named("float64"),
            ArrayOp::Map => match args.first() {
                Some(Some(
                    Type::Function { return_type, .. } | Type::Closure { return_type, .. },
                )) => array(vec![(**return_type).clone()]),
                _ => return None,
            },
        })
    }
}

/// A private range operation of a `Span[T]` view, which runs on the span's
/// base list with absolute positions (see `mir_spans`). Each op is named
/// after its MIR spelling, and its opcode is its index in
/// `runtime_value::SPAN_OPERATIONS`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum SpanOp {
    Start,
    Stop,
    Index,
    Contains,
    Find,
    Count,
    Eq,
    Extract,
    Restore,
    Fill,
}

/// The helper's operand words after the receiver and opcode: up to eight
/// `int64` operands and one value word.
const SPAN_OPERAND_WORDS: usize = 9;

const SPAN_BOUNDS_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("outer_start"),
    CallableParam::required("outer_len"),
    CallableParam::required("start"),
    CallableParam::required("has_start"),
    CallableParam::required("end"),
    CallableParam::required("has_end"),
    CallableParam::required("line"),
    CallableParam::required("column"),
];
const SPAN_INDEX_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("start"),
    CallableParam::required("len"),
    CallableParam::required("index"),
    CallableParam::required("line"),
    CallableParam::required("column"),
];
const SPAN_VALUE_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("start"),
    CallableParam::required("len"),
    CallableParam::required("value"),
];
const SPAN_EQ_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("start"),
    CallableParam::required("len"),
    CallableParam::required("other"),
    CallableParam::required("other_start"),
    CallableParam::required("other_len"),
];
const SPAN_EXTRACT_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("start"),
    CallableParam::required("len"),
];
const SPAN_RESTORE_PARAMS: &[CallableParam<'static>] = &[
    CallableParam::required("start"),
    CallableParam::required("values"),
];

impl SpanOp {
    pub const ALL: &'static [SpanOp] = &[
        SpanOp::Start,
        SpanOp::Stop,
        SpanOp::Index,
        SpanOp::Contains,
        SpanOp::Find,
        SpanOp::Count,
        SpanOp::Eq,
        SpanOp::Extract,
        SpanOp::Restore,
        SpanOp::Fill,
    ];

    /// The op's opcode word, its index in `SPAN_OPERATIONS`.
    const fn opcode_words(self) -> &'static [i64] {
        match self {
            SpanOp::Start => &[0],
            SpanOp::Stop => &[1],
            SpanOp::Index => &[2],
            SpanOp::Contains => &[3],
            SpanOp::Find => &[4],
            SpanOp::Count => &[5],
            SpanOp::Eq => &[6],
            SpanOp::Extract => &[7],
            SpanOp::Restore => &[8],
            SpanOp::Fill => &[9],
        }
    }

    /// The MIR spelling, such as `__span_start`.
    pub const fn name(self) -> &'static str {
        crate::runtime_value::SPAN_OPERATIONS[self.opcode_words()[0] as usize]
    }

    fn from_private_name(field: &str) -> Option<Self> {
        for op in SpanOp::ALL {
            if op.name() == field {
                return Some(*op);
            }
        }
        None
    }

    pub(crate) const fn params(self) -> &'static [CallableParam<'static>] {
        match self {
            SpanOp::Start | SpanOp::Stop => SPAN_BOUNDS_PARAMS,
            SpanOp::Index => SPAN_INDEX_PARAMS,
            SpanOp::Contains | SpanOp::Find | SpanOp::Count | SpanOp::Fill => SPAN_VALUE_PARAMS,
            SpanOp::Eq => SPAN_EQ_PARAMS,
            SpanOp::Extract => SPAN_EXTRACT_PARAMS,
            SpanOp::Restore => SPAN_RESTORE_PARAMS,
        }
    }

    /// The position of the argument that passes as the value word: the
    /// element a search or fill compares or writes, the other list of an
    /// equality, or the values a restore puts back. Every other argument is
    /// an `int64` operand.
    pub(crate) const fn value_position(self) -> Option<usize> {
        match self {
            SpanOp::Contains | SpanOp::Find | SpanOp::Count | SpanOp::Eq | SpanOp::Fill => Some(2),
            SpanOp::Restore => Some(1),
            SpanOp::Start | SpanOp::Stop | SpanOp::Index | SpanOp::Extract => None,
        }
    }

    /// Whether the op writes the base list.
    pub const fn mutates_receiver(self) -> bool {
        matches!(self, SpanOp::Extract | SpanOp::Restore | SpanOp::Fill)
    }

    /// The helper takes the receiver and the opcode, then the `int64`
    /// operands padded with zero words to eight, then the value word or a
    /// zero.
    const fn runtime_call(self) -> RuntimeCall {
        helper_with(
            RuntimeFn::VecSpan,
            self.opcode_words(),
            SPAN_OPERAND_WORDS - self.params().len(),
        )
    }

    fn result_type(self, receiver: Option<&Type>) -> Option<Type> {
        Some(match self {
            SpanOp::Start | SpanOp::Stop | SpanOp::Index | SpanOp::Find | SpanOp::Count => {
                Type::named("int64")
            }
            SpanOp::Contains | SpanOp::Eq => Type::named("bool"),
            SpanOp::Extract => receiver?.clone(),
            SpanOp::Restore | SpanOp::Fill => Type::Unit,
        })
    }
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

    /// How the op passes the argument at `index`, by the declaration
    /// `params` reads. `None` when it has no such parameter.
    fn argument_passing(self, index: usize) -> Option<ReceiverKind> {
        use FunctionOp::*;
        match self {
            QueueNew | WaitAny | WaitAll | WaitAnyTimeout | WaitAllTimeout | TaskGroupNew
            | ListEmpty | DictEmpty | SetEmpty => {
                (index < self.params().len()).then_some(ReceiverKind::Borrow)
            }
            RngNew => BuiltinClassConstructor::RandomRng.argument_passing(index),
            _ => {
                if let Some(builtin) = BuiltinFunction::from_name(self.name()) {
                    return builtin.argument_passing(index);
                }
                if let Some(associated) = self
                    .name()
                    .split_once('.')
                    .and_then(|(owner, member)| BuiltinAssociatedFunction::resolve(owner, member))
                {
                    return associated.argument_passing(index);
                }
                Some(self.module_metadata()?.params.get(index)?.passing)
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
            .chain(ListOp::ALL.iter().copied().map(RuntimeOp::List))
            .chain(DictOp::ALL.iter().copied().map(RuntimeOp::Dict))
            .chain(SetOp::ALL.iter().copied().map(RuntimeOp::Set))
            .chain(ArrayOp::ALL.iter().copied().map(RuntimeOp::Array))
            .chain(SpanOp::ALL.iter().copied().map(RuntimeOp::Span))
            .chain(FunctionOp::ALL.iter().copied().map(RuntimeOp::Function))
            .chain(PrintKind::ALL.iter().copied().map(RuntimeOp::Print))
            .chain(HostBuiltin::all().map(RuntimeOp::Host))
    }

    /// The op a call `receiver.field` resolves to, under the member
    /// precedence of `builtin_member_for_receiver`. `class_declares` says
    /// whether the class named by the receiver type declares `field`. A
    /// private member, such as a queue receive or `__index`, resolves by its
    /// MIR spelling on the receiver types that lower to it. The op is
    /// unspecialized; lowering picks a timeout or scalar form from the
    /// receiver and the arguments.
    pub(crate) fn resolve_member(
        receiver: &Type,
        field: &str,
        class_declares: impl FnOnce(&str) -> bool,
    ) -> Option<Self> {
        if let Type::Named(name, _) = receiver {
            if field.starts_with("__") {
                let op = Self::from_private_member(name, field)?;
                return (!class_declares(name)).then_some(op);
            }
        }
        builtin_member_for_receiver(receiver, field, class_declares)
            .and_then(Self::from_builtin_member)
    }

    /// The private op a receiver of type `receiver` spells `field`.
    fn from_private_member(receiver: &str, field: &str) -> Option<Self> {
        match receiver {
            "Queue" => QueueOp::from_private_name(field).map(RuntimeOp::Queue),
            "str" => StrOp::from_private_name(field).map(RuntimeOp::Str),
            "list" => match ListOp::from_private_name(field) {
                Some(op) => Some(RuntimeOp::List(op)),
                None => SpanOp::from_private_name(field).map(RuntimeOp::Span),
            },
            "dict" => DictOp::from_private_name(field).map(RuntimeOp::Dict),
            "set" => SetOp::from_private_name(field).map(RuntimeOp::Set),
            "Array" => ArrayOp::from_private_name(field).map(RuntimeOp::Array),
            _ => None,
        }
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
        if let Some(op) = ListOp::from_builtin_member(member) {
            return Some(RuntimeOp::List(op));
        }
        if let Some(op) = DictOp::from_builtin_member(member) {
            return Some(RuntimeOp::Dict(op));
        }
        if let Some(op) = SetOp::from_builtin_member(member) {
            return Some(RuntimeOp::Set(op));
        }
        if let Some(op) = ArrayOp::from_builtin_member(member) {
            return Some(RuntimeOp::Array(op));
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
    /// collection, so it reports the `list` member, and a timeout or scalar
    /// form reports its base member. A private op has none.
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
            RuntimeOp::Str(op) => return op.builtin_member(),
            RuntimeOp::List(op) => return op.builtin_member(),
            RuntimeOp::Dict(op) => return op.builtin_member(),
            RuntimeOp::Set(op) => return op.builtin_member(),
            RuntimeOp::Array(op) => return op.builtin_member(),
            RuntimeOp::Span(_) => return None,
            RuntimeOp::Queue(op) => return op.builtin_member(),
            RuntimeOp::Task(op) => op.builtin_member(),
            RuntimeOp::TaskGroup(TaskGroupOp::Cancel) => M::TaskGroupCancel,
            RuntimeOp::Function(_) | RuntimeOp::Print(_) | RuntimeOp::Host(_) => return None,
        })
    }

    /// The source name: a member name such as `read_all` for an op with a
    /// receiver, the private member a MIR call spells, such as `__index`, or
    /// the callee name a MIR `Name` call spells, such as `print` or
    /// `fs::exists`.
    pub fn name(self) -> &'static str {
        if let Some(name) = self.private_name() {
            return name;
        }
        match self {
            RuntimeOp::Function(op) => op.name(),
            RuntimeOp::Print(_) => "print",
            RuntimeOp::Host(builtin) => builtin.name(),
            _ => self
                .builtin_member()
                .expect("an op with a receiver has a built-in member")
                .name(),
        }
    }

    /// The MIR spelling of a private member op, or of an op whose member is
    /// named otherwise, such as `contains_key`.
    const fn private_name(self) -> Option<&'static str> {
        match self {
            RuntimeOp::Queue(QueueOp::GetInTaskGroup) => Some(QueueOp::GET_IN_TASK_GROUP),
            RuntimeOp::Queue(QueueOp::GetWithRegisteredProducers) => {
                Some(QueueOp::GET_WITH_REGISTERED_PRODUCERS)
            }
            RuntimeOp::Str(op) => op.private_name(),
            RuntimeOp::List(op) => op.private_name(),
            RuntimeOp::Dict(op) => op.private_name(),
            RuntimeOp::Set(op) => op.private_name(),
            RuntimeOp::Array(op) => op.private_name(),
            RuntimeOp::Span(op) => Some(op.name()),
            _ => None,
        }
    }

    /// The parameters of a private member op, which has no source member
    /// to name them.
    const fn private_params(self) -> Option<&'static [CallableParam<'static>]> {
        match self {
            RuntimeOp::Str(StrOp::Slice) => Some(SLICE_PARAMS),
            RuntimeOp::List(op) => op.private_params(),
            RuntimeOp::Dict(op) => op.private_params(),
            RuntimeOp::Set(op) => op.private_params(),
            RuntimeOp::Array(op) => op.private_params(),
            RuntimeOp::Span(op) => Some(op.params()),
            _ => None,
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
            | RuntimeOp::TaskGroup(_)
            | RuntimeOp::List(_)
            | RuntimeOp::Dict(_)
            | RuntimeOp::Set(_)
            | RuntimeOp::Array(_)
            | RuntimeOp::Span(_) => true,
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
            RuntimeOp::List(op) => RuntimeOp::List(op.unspecialized()),
            RuntimeOp::Dict(op) => RuntimeOp::Dict(op.unspecialized()),
            RuntimeOp::Set(op) => RuntimeOp::Set(op.unspecialized()),
            other => other,
        }
    }

    /// The checked form of an unchecked element read, and any other op
    /// itself.
    pub fn checked(self) -> Self {
        match self {
            RuntimeOp::List(op) => RuntimeOp::List(op.checked()),
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
            _ if self.private_params().is_some() => self
                .private_params()
                .expect("a private op names its parameters")
                .to_vec(),
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

    /// The argument a call that leaves out the optional parameter `name`
    /// passes. A value with a constant spelling is that constant. Any other
    /// is an `Operand::Default` that names it.
    pub fn default_arg(self, name: &str) -> crate::mir::Operand {
        use crate::mir::Operand;
        let named = |default| Operand::Default(default);
        match (self, name) {
            (RuntimeOp::Resource(ResourceOp::ProcessSupervisorStart), _) => match name {
                "cwd" => Operand::Unit,
                "env" => named(ArgDefault::EmptyStrDict),
                "stdin" => named(ArgDefault::StdioNull),
                "stdout" | "stderr" => named(ArgDefault::StdioInherit),
                "restart" => named(ArgDefault::RestartOnFailure),
                "backoff" => named(ArgDefault::Milliseconds(100)),
                "max_restarts" => named(ArgDefault::Int32(-1)),
                "group" => Operand::Bool(true),
                _ => named(ArgDefault::Omitted),
            },
            (RuntimeOp::List(ListOp::Pop), "index") => named(ArgDefault::Int64(-1)),
            _ => named(ArgDefault::Omitted),
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

    /// How the op takes its argument at `index`, in `params()` order:
    /// `Consume` when the helper keeps the reference, as for an `own`
    /// parameter or the value an index write stores, and `Borrow` when it
    /// only reads the value. `None` when the op has no such parameter. A
    /// variadic `select` borrows every source.
    pub fn arg_ownership(self, index: usize) -> Option<OperandRole> {
        // A private op declares no member. `consumed` lists the positions
        // of its parameters the helper keeps.
        let private = |consumed: &[usize]| {
            (index < self.params().len()).then(|| {
                if consumed.contains(&index) {
                    OperandRole::Consume
                } else {
                    OperandRole::Borrow
                }
            })
        };
        // A member op without its timeout form drops the trailing
        // `timeout` the member declares.
        let member = || {
            if index >= self.params().len() {
                return None;
            }
            self.builtin_member()?
                .argument_passing(index)
                .map(OperandRole::of_passing)
        };
        match self {
            RuntimeOp::Resource(_)
            | RuntimeOp::Rng(_)
            | RuntimeOp::Scalar(_)
            | RuntimeOp::Value(_)
            | RuntimeOp::Task(_)
            | RuntimeOp::TaskGroup(_) => member(),
            RuntimeOp::Str(StrOp::Slice) => private(&[]),
            RuntimeOp::Str(_) => member(),
            RuntimeOp::Queue(QueueOp::GetInTaskGroup | QueueOp::GetWithRegisteredProducers) => {
                private(&[])
            }
            RuntimeOp::Queue(_) => member(),
            RuntimeOp::List(ListOp::SetIndex | ListOp::SetIndexScalar(_)) => private(&[1]),
            RuntimeOp::List(
                ListOp::Index
                | ListOp::IndexScalar(_)
                | ListOp::IndexUnchecked
                | ListOp::IndexScalarUnchecked(_)
                | ListOp::Slice
                | ListOp::IndexOption
                | ListOp::TakeIndexOption
                | ListOp::Sort
                | ListOp::SortByKeys
                | ListOp::Permute,
            ) => private(&[]),
            RuntimeOp::List(_) => member(),
            RuntimeOp::Dict(DictOp::SetIndex) => private(&[0, 1]),
            RuntimeOp::Dict(DictOp::Index) => private(&[]),
            RuntimeOp::Dict(_) => member(),
            RuntimeOp::Set(SetOp::IndexOption | SetOp::TakeIndexOption) => private(&[]),
            RuntimeOp::Set(_) => member(),
            RuntimeOp::Array(ArrayOp::SetIndex) => private(&[1]),
            RuntimeOp::Array(ArrayOp::Index | ArrayOp::Slice) => private(&[]),
            RuntimeOp::Array(_) => member(),
            // A restore puts the extracted values back into the list.
            RuntimeOp::Span(SpanOp::Restore) => private(&[1]),
            RuntimeOp::Span(_) => private(&[]),
            RuntimeOp::Function(op) => op.argument_passing(index).map(OperandRole::of_passing),
            RuntimeOp::Print(_) => BuiltinFunction::Print
                .argument_passing(index)
                .map(OperandRole::of_passing),
            RuntimeOp::Host(builtin) => builtin
                .metadata()
                .params
                .get(index)
                .map(|param| OperandRole::of_passing(param.passing)),
        }
    }

    /// How the op takes its receiver: every op with a receiver borrows it,
    /// and a mutating op writes it back in place. `None` for an op with no
    /// receiver.
    pub fn receiver_ownership(self) -> Option<OperandRole> {
        match self {
            RuntimeOp::Resource(_)
            | RuntimeOp::Rng(_)
            | RuntimeOp::Scalar(_)
            | RuntimeOp::Value(_)
            | RuntimeOp::Str(_)
            | RuntimeOp::Queue(_)
            | RuntimeOp::Task(_)
            | RuntimeOp::TaskGroup(_)
            | RuntimeOp::List(_)
            | RuntimeOp::Dict(_)
            | RuntimeOp::Set(_)
            | RuntimeOp::Array(_)
            | RuntimeOp::Span(_) => Some(OperandRole::Borrow),
            RuntimeOp::Function(_) | RuntimeOp::Print(_) | RuntimeOp::Host(_) => None,
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
            RuntimeOp::List(op) => RuntimeTarget::Helper(op.runtime_call()),
            RuntimeOp::Dict(op) => RuntimeTarget::Helper(op.runtime_call()),
            RuntimeOp::Set(op) => RuntimeTarget::Helper(op.runtime_call()),
            RuntimeOp::Array(op) => RuntimeTarget::Helper(op.runtime_call()),
            RuntimeOp::Span(op) => RuntimeTarget::Helper(op.runtime_call()),
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
            RuntimeOp::List(op) => return op.result_type(receiver),
            RuntimeOp::Dict(op) => return op.result_type(receiver),
            RuntimeOp::Set(op) => op.result_type(first_type_arg(receiver)),
            RuntimeOp::Array(op) => return op.result_type(receiver, args),
            RuntimeOp::Span(op) => return op.result_type(receiver),
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
            RuntimeOp::List(op) => write!(formatter, "List.{op:?}"),
            RuntimeOp::Dict(op) => write!(formatter, "Dict.{op:?}"),
            RuntimeOp::Set(op) => write!(formatter, "Set.{op:?}"),
            RuntimeOp::Array(op) => write!(formatter, "Array.{op:?}"),
            RuntimeOp::Span(op) => write!(formatter, "Span.{op:?}"),
            RuntimeOp::Function(op) => write!(formatter, "Function.{op:?}"),
            RuntimeOp::Print(kind) => write!(formatter, "Print.{kind:?}"),
            RuntimeOp::Host(builtin) => write!(formatter, "Host.{}", builtin.name()),
        }
    }
}

#[cfg(test)]
#[path = "runtime_ops_tests.rs"]
mod tests;
