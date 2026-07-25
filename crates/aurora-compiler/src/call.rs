use std::collections::HashMap;

use crate::ast::{Argument, Param, ReceiverKind};
use crate::diag::{Diagnostic, Result, Span};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CallConvention {
    PositionalOnly,
    PositionalOrNamed,
    KeywordOnly,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CallableParam<'a> {
    pub name: &'a str,
    pub required: bool,
}

impl<'a> CallableParam<'a> {
    pub const fn required(name: &'a str) -> Self {
        Self {
            name,
            required: true,
        }
    }

    pub const fn optional(name: &'a str) -> Self {
        Self {
            name,
            required: false,
        }
    }
}

pub fn callable_params_from_decl<'a>(params: &'a [Param]) -> Vec<CallableParam<'a>> {
    params
        .iter()
        .map(|param| match param.default.is_some() {
            true => CallableParam::optional(&param.name),
            false => CallableParam::required(&param.name),
        })
        .collect()
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BuiltinParam {
    binding: CallableParam<'static>,
    passing: ReceiverKind,
}

macro_rules! builtin_param {
    (required, $name:literal, $passing:expr) => {
        BuiltinParam {
            binding: CallableParam {
                name: $name,
                required: true,
            },
            passing: $passing,
        }
    };
    (optional, $name:literal, $passing:expr) => {
        BuiltinParam {
            binding: CallableParam {
                name: $name,
                required: false,
            },
            passing: $passing,
        }
    };
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BuiltinCallShape {
    params: &'static [BuiltinParam],
    convention: CallConvention,
    variadic_passing: Option<ReceiverKind>,
}

impl BuiltinCallShape {
    const fn fixed(params: &'static [BuiltinParam], convention: CallConvention) -> Self {
        Self {
            params,
            convention,
            variadic_passing: None,
        }
    }

    const fn variadic(params: &'static [BuiltinParam], variadic_passing: ReceiverKind) -> Self {
        Self {
            params,
            convention: CallConvention::PositionalOnly,
            variadic_passing: Some(variadic_passing),
        }
    }

    fn bind_args<'arg>(
        self,
        callee_name: &str,
        args: &'arg [Argument],
        span: Span,
    ) -> Result<Vec<Option<&'arg Argument>>> {
        let bindings = self
            .params
            .iter()
            .map(|param| param.binding)
            .collect::<Vec<_>>();
        let Some(_) = self.variadic_passing else {
            return bind_call_arguments(callee_name, &bindings, args, span, self.convention);
        };

        if let Some(argument) = args.iter().find(|argument| argument.name.is_some()) {
            return Err(Diagnostic::at(
                argument.span,
                format!("{} does not take keyword arguments", callee_name),
            ));
        }
        let fixed_len = self.params.len().min(args.len());
        let mut ordered = bind_call_arguments(
            callee_name,
            &bindings,
            &args[..fixed_len],
            span,
            self.convention,
        )?;
        ordered.extend(args[fixed_len..].iter().map(Some));
        Ok(ordered)
    }
}

fn format_argument_count(count: usize) -> String {
    format!("{} argument{}", count, if count == 1 { "" } else { "s" })
}

pub fn bind_call_arguments<'arg, 'param>(
    callee_name: &str,
    params: &[CallableParam<'param>],
    args: &'arg [Argument],
    span: Span,
    convention: CallConvention,
) -> Result<Vec<Option<&'arg Argument>>> {
    if args.len() > params.len() {
        return Err(Diagnostic::at(
            span,
            format!(
                "{} expects {}, found {}",
                callee_name,
                format_argument_count(params.len()),
                args.len()
            ),
        ));
    }

    let mut ordered_args = vec![None; params.len()];
    let mut param_indexes = HashMap::new();
    for (index, param) in params.iter().enumerate() {
        param_indexes.insert(param.name, index);
    }

    let mut next_positional = 0usize;
    let mut saw_named = false;

    for argument in args {
        if let Some(name) = argument.name.as_deref() {
            if convention == CallConvention::PositionalOnly {
                return Err(Diagnostic::at(
                    argument.span,
                    format!("{} does not take keyword arguments", callee_name),
                ));
            }

            saw_named = true;
            let Some(&param_index) = param_indexes.get(name) else {
                return Err(Diagnostic::coded_at(
                    "AU2004",
                    argument.span,
                    format!("{} has no parameter named `{}`", callee_name, name),
                ));
            };
            if ordered_args[param_index].is_some() {
                return Err(Diagnostic::coded_at(
                    "AU2004",
                    argument.span,
                    format!("parameter `{}` was provided more than once", name),
                ));
            }
            ordered_args[param_index] = Some(argument);
            continue;
        }

        match convention {
            CallConvention::KeywordOnly => {
                return Err(Diagnostic::at(
                    argument.span,
                    format!("all arguments in {} must be named", callee_name),
                ));
            }
            CallConvention::PositionalOrNamed if saw_named => {
                return Err(Diagnostic::at(
                    argument.span,
                    format!(
                        "positional arguments must come before named arguments in {}",
                        callee_name
                    ),
                ));
            }
            CallConvention::PositionalOnly | CallConvention::PositionalOrNamed => {}
        }

        ordered_args[next_positional] = Some(argument);
        next_positional += 1;
    }

    if let Some(missing_param) = params
        .iter()
        .enumerate()
        .find(|(index, param)| ordered_args[*index].is_none() && param.required)
        .map(|(_, param)| param)
    {
        return Err(Diagnostic::at(
            span,
            format!(
                "{} is missing required argument `{}`",
                callee_name, missing_param.name
            ),
        ));
    }

    Ok(ordered_args)
}

const PRINT_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("value")];
const RANGE_STOP_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("stop")];
const RANGE_START_STOP_PARAMS: [CallableParam<'static>; 2] = [
    CallableParam::required("start"),
    CallableParam::required("stop"),
];
const ABS_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("value")];
const MIN_MAX_PARAMS: [CallableParam<'static>; 2] = [
    CallableParam::required("left"),
    CallableParam::required("right"),
];
const SQRT_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("value")];
const PARSE_TEXT_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("text")];
const RNG_SEED_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("seed")];
const SLEEP_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("duration")];
const TASK_LIST_TIMEOUT_PARAMS: [CallableParam<'static>; 2] = [
    CallableParam::required("tasks"),
    CallableParam::optional("timeout"),
];
const NO_BUILTIN_PARAMS: [BuiltinParam; 0] = [];
const DURATION_VALUE_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "value", ReceiverKind::Value)];
const STRING_FROM_BYTES_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "bytes", ReceiverKind::Borrow)];
const FILE_WRITE_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "text", ReceiverKind::Borrow)];
const FILE_WRITE_BYTES_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "bytes", ReceiverKind::Borrow)];
const VALUE_TIMEOUT_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "value", ReceiverKind::Value),
    builtin_param!(optional, "timeout", ReceiverKind::Borrow),
];
const DEFAULT_TIMEOUT_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "default", ReceiverKind::Value),
    builtin_param!(optional, "timeout", ReceiverKind::Borrow),
];
const QUEUE_GET_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(optional, "timeout", ReceiverKind::Borrow)];
const TIMEOUT_ONLY_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(optional, "timeout", ReceiverKind::Borrow)];
const VEC_INDEX_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "index", ReceiverKind::Borrow)];
const VEC_PUSH_PARAMS: [BuiltinParam; 1] = [builtin_param!(required, "value", ReceiverKind::Value)];
const VALUE_PARAMS: [BuiltinParam; 1] = [builtin_param!(required, "value", ReceiverKind::Borrow)];
const VEC_SET_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "index", ReceiverKind::Borrow),
    builtin_param!(required, "value", ReceiverKind::Value),
];
const VEC_SWAP_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "first", ReceiverKind::Borrow),
    builtin_param!(required, "second", ReceiverKind::Borrow),
];
const VEC_INSERT_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "index", ReceiverKind::Borrow),
    builtin_param!(required, "value", ReceiverKind::Value),
];
const VEC_EXTEND_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "other", ReceiverKind::Value)];
const STRING_TEXT_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "text", ReceiverKind::Borrow)];
const STRING_REPLACE_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "from", ReceiverKind::Borrow),
    builtin_param!(required, "to", ReceiverKind::Borrow),
];
const STRING_JOIN_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "parts", ReceiverKind::Borrow)];
const MAP_KEY_PARAMS: [BuiltinParam; 1] = [builtin_param!(required, "key", ReceiverKind::Borrow)];
const MAP_SET_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "key", ReceiverKind::Value),
    builtin_param!(required, "value", ReceiverKind::Value),
];
const MAP_EXTEND_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "other", ReceiverKind::Value)];
const SET_VALUE_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "value", ReceiverKind::Borrow)];
const SET_INSERT_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "value", ReceiverKind::Value)];
const COUNT_TIMEOUT_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "count", ReceiverKind::Borrow),
    builtin_param!(optional, "timeout", ReceiverKind::Borrow),
];
const MAX_BYTES_TIMEOUT_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "max_bytes", ReceiverKind::Borrow),
    builtin_param!(optional, "timeout", ReceiverKind::Borrow),
];
const TEXT_TIMEOUT_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "text", ReceiverKind::Borrow),
    builtin_param!(optional, "timeout", ReceiverKind::Borrow),
];
const BYTES_TIMEOUT_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "bytes", ReceiverKind::Borrow),
    builtin_param!(optional, "timeout", ReceiverKind::Borrow),
];
const PROCESS_SUPERVISOR_START_PARAMS: [BuiltinParam; 11] = [
    builtin_param!(required, "name", ReceiverKind::Value),
    builtin_param!(required, "command", ReceiverKind::Value),
    builtin_param!(optional, "cwd", ReceiverKind::Value),
    builtin_param!(optional, "env", ReceiverKind::Value),
    builtin_param!(optional, "stdin", ReceiverKind::Value),
    builtin_param!(optional, "stdout", ReceiverKind::Value),
    builtin_param!(optional, "stderr", ReceiverKind::Value),
    builtin_param!(optional, "restart", ReceiverKind::Value),
    builtin_param!(optional, "backoff", ReceiverKind::Value),
    builtin_param!(optional, "max_restarts", ReceiverKind::Value),
    builtin_param!(optional, "group", ReceiverKind::Value),
];
const ADDRESS_TEXT_TIMEOUT_PARAMS: [BuiltinParam; 3] = [
    builtin_param!(required, "address", ReceiverKind::Borrow),
    builtin_param!(required, "text", ReceiverKind::Borrow),
    builtin_param!(optional, "timeout", ReceiverKind::Borrow),
];
const ADDRESS_BYTES_TIMEOUT_PARAMS: [BuiltinParam; 3] = [
    builtin_param!(required, "address", ReceiverKind::Borrow),
    builtin_param!(required, "bytes", ReceiverKind::Borrow),
    builtin_param!(optional, "timeout", ReceiverKind::Borrow),
];
const STATUS_TEXT_HEADERS_PARAMS: [BuiltinParam; 3] = [
    builtin_param!(required, "status", ReceiverKind::Borrow),
    builtin_param!(required, "text", ReceiverKind::Value),
    builtin_param!(required, "headers", ReceiverKind::Value),
];
const STATUS_BYTES_HEADERS_PARAMS: [BuiltinParam; 3] = [
    builtin_param!(required, "status", ReceiverKind::Borrow),
    builtin_param!(required, "bytes", ReceiverKind::Value),
    builtin_param!(required, "headers", ReceiverKind::Value),
];
const TASK_GROUP_START_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "function", ReceiverKind::Borrow)];
const RNG_NEXT_INT_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "lo", ReceiverKind::Borrow),
    builtin_param!(required, "hi", ReceiverKind::Borrow),
];
const RNG_SHUFFLE_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "values", ReceiverKind::BorrowMut)];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuiltinFunction {
    Print,
    Range,
    Cancelled,
    Sleep,
    WaitAny,
    WaitAll,
    Abs,
    Min,
    Max,
    Sqrt,
    ParseInt32,
    ParseInt64,
    ParseFloat64,
    Len,
    Str,
}

pub const ALL_BUILTIN_FUNCTIONS: &[BuiltinFunction] = &[
    BuiltinFunction::Print,
    BuiltinFunction::Range,
    BuiltinFunction::Cancelled,
    BuiltinFunction::Sleep,
    BuiltinFunction::WaitAny,
    BuiltinFunction::WaitAll,
    BuiltinFunction::Len,
    BuiltinFunction::Str,
    BuiltinFunction::Abs,
    BuiltinFunction::Min,
    BuiltinFunction::Max,
    BuiltinFunction::Sqrt,
    BuiltinFunction::ParseInt32,
    BuiltinFunction::ParseInt64,
    BuiltinFunction::ParseFloat64,
];

impl BuiltinFunction {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "print" => Some(Self::Print),
            "range" => Some(Self::Range),
            "cancelled" => Some(Self::Cancelled),
            "sleep" => Some(Self::Sleep),
            "wait_any" => Some(Self::WaitAny),
            "wait_all" => Some(Self::WaitAll),
            "abs" => Some(Self::Abs),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            "sqrt" => Some(Self::Sqrt),
            "parse_int32" => Some(Self::ParseInt32),
            "parse_int64" => Some(Self::ParseInt64),
            "parse_float64" => Some(Self::ParseFloat64),
            "len" => Some(Self::Len),
            "str" => Some(Self::Str),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Print => "print",
            Self::Range => "range",
            Self::Cancelled => "cancelled",
            Self::Sleep => "sleep",
            Self::WaitAny => "wait_any",
            Self::WaitAll => "wait_all",
            Self::Abs => "abs",
            Self::Min => "min",
            Self::Max => "max",
            Self::Sqrt => "sqrt",
            Self::ParseInt32 => "parse_int32",
            Self::ParseInt64 => "parse_int64",
            Self::ParseFloat64 => "parse_float64",
            Self::Len => "len",
            Self::Str => "str",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::Print => "print(value) -> None",
            Self::Range => "range(stop: int32) -> Range; range(start: int32, stop: int32) -> Range",
            Self::Cancelled => "cancelled() -> bool",
            Self::Sleep => "sleep(duration: Duration) -> None",
            Self::WaitAny => "wait_any(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAny[T]",
            Self::WaitAll => "wait_all(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAll[T]",
            Self::Abs => "abs(value: number) -> number",
            Self::Min => "min(left: number, right: number) -> number",
            Self::Max => "max(left: number, right: number) -> number",
            Self::Sqrt => "sqrt(value: float32|float64) -> float32|float64",
            Self::ParseInt32 => "parse_int32(text: String) -> Result[int32, String]",
            Self::ParseInt64 => "parse_int64(text: String) -> Result[int64, String]",
            Self::ParseFloat64 => "parse_float64(text: String) -> Result[float64, String]",
            Self::Len => "len(value: String|Vec[T]|Map[K, V]|Set[T]) -> int64",
            Self::Str => "str(value) -> String",
        }
    }

    pub const fn docs(self) -> &'static str {
        match self {
            Self::Print => "Writes a value followed by a newline.",
            Self::Range => {
                "Builds an integer range from 0 up to, but not including, `stop`, or from `start` up to, but not including, `stop`."
            }
            Self::Cancelled => "Returns true when the current task has been cancelled.",
            Self::Sleep => "Blocks the current task for the requested duration.",
            Self::WaitAny => {
                "Waits for the first task to complete and reports either the ready index/value pair, the failing task index/error message, a timeout, or cancellation."
            }
            Self::WaitAll => {
                "Waits for every task to complete and reports either the collected results, the failing task index/error message, a timeout, or cancellation."
            }
            Self::Abs => "Returns the absolute value of an integer or float.",
            Self::Min => "Returns the smaller of two numeric values of the same type.",
            Self::Max => "Returns the larger of two numeric values of the same type.",
            Self::Sqrt => "Returns the square root of a `float32` or `float64` value.",
            Self::ParseInt32 => "Parses a `String` into an `int32`, returning `Result.Err(String)` on failure.",
            Self::ParseInt64 => "Parses a `String` into an `int64`, returning `Result.Err(String)` on failure.",
            Self::ParseFloat64 => "Parses a `String` into a `float64`, returning `Result.Err(String)` on failure.",
            Self::Len => "Returns the length of a value that has a `len()` member, delegating to that member.",
            Self::Str => "Renders a value the way `print` and f-string interpolation render it.",
        }
    }

    pub fn bind_args(self, args: &[Argument], span: Span) -> Result<Vec<Option<&Argument>>> {
        match self {
            Self::Print => bind_call_arguments(
                "`print`",
                &PRINT_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::Range => {
                if args.len() > 2 {
                    return Err(Diagnostic::at(
                        span,
                        format!("`range` expects 1 or 2 arguments, found {}", args.len()),
                    ));
                }
                let use_two_arg_signature =
                    args.len() == 2 || args.iter().any(|arg| arg.name.as_deref() == Some("start"));
                if use_two_arg_signature {
                    bind_call_arguments(
                        "`range`",
                        &RANGE_START_STOP_PARAMS,
                        args,
                        span,
                        CallConvention::PositionalOrNamed,
                    )
                } else {
                    bind_call_arguments(
                        "`range`",
                        &RANGE_STOP_PARAMS,
                        args,
                        span,
                        CallConvention::PositionalOrNamed,
                    )
                }
            }
            Self::Cancelled => bind_call_arguments(
                "`cancelled`",
                &[],
                args,
                span,
                CallConvention::PositionalOnly,
            ),
            Self::Sleep => bind_call_arguments(
                "`sleep`",
                &SLEEP_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::WaitAny | Self::WaitAll => bind_call_arguments(
                &format!("`{}`", self.name()),
                &TASK_LIST_TIMEOUT_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::Len | Self::Str => bind_call_arguments(
                &format!("`{}`", self.name()),
                &PRINT_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::Abs => bind_call_arguments(
                "`abs`",
                &ABS_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::Min => bind_call_arguments(
                "`min`",
                &MIN_MAX_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::Max => bind_call_arguments(
                "`max`",
                &MIN_MAX_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::Sqrt => bind_call_arguments(
                "`sqrt`",
                &SQRT_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::ParseInt32 | Self::ParseInt64 | Self::ParseFloat64 => bind_call_arguments(
                &format!("`{}`", self.name()),
                &PARSE_TEXT_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuiltinAssociatedFunction {
    DurationMilliseconds,
    DurationSeconds,
    DurationMinutes,
    StringFromBytes,
}

pub const ALL_BUILTIN_ASSOCIATED_FUNCTIONS: &[BuiltinAssociatedFunction] = &[
    BuiltinAssociatedFunction::DurationMilliseconds,
    BuiltinAssociatedFunction::DurationSeconds,
    BuiltinAssociatedFunction::DurationMinutes,
    BuiltinAssociatedFunction::StringFromBytes,
];

impl BuiltinAssociatedFunction {
    pub fn resolve(type_name: &str, name: &str) -> Option<Self> {
        match (type_name, name) {
            ("Duration", "ms") => Some(Self::DurationMilliseconds),
            ("Duration", "seconds") => Some(Self::DurationSeconds),
            ("Duration", "minutes") => Some(Self::DurationMinutes),
            ("String", "from_bytes") => Some(Self::StringFromBytes),
            _ => None,
        }
    }

    pub const fn owner_name(self) -> &'static str {
        match self {
            Self::DurationMilliseconds | Self::DurationSeconds | Self::DurationMinutes => {
                "Duration"
            }
            Self::StringFromBytes => "String",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::DurationMilliseconds => "ms",
            Self::DurationSeconds => "seconds",
            Self::DurationMinutes => "minutes",
            Self::StringFromBytes => "from_bytes",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::DurationMilliseconds => "ms(value: int64) -> Duration",
            Self::DurationSeconds => "seconds(value: int64) -> Duration",
            Self::DurationMinutes => "minutes(value: int64) -> Duration",
            Self::StringFromBytes => "from_bytes(bytes: Vec[uint8]) -> Result[String, bytes.Error]",
        }
    }

    pub const fn docs(self) -> &'static str {
        match self {
            Self::DurationMilliseconds => {
                "Constructs a Duration from an exact signed number of milliseconds."
            }
            Self::DurationSeconds => {
                "Constructs a Duration from an exact signed number of seconds."
            }
            Self::DurationMinutes => {
                "Constructs a Duration from an exact signed number of minutes."
            }
            Self::StringFromBytes => {
                "Strictly decodes UTF-8 bytes, returning `bytes.Error.InvalidUtf8` at the first invalid byte."
            }
        }
    }

    pub fn bind_args(self, args: &[Argument], span: Span) -> Result<Vec<Option<&Argument>>> {
        self.call_shape().bind_args(
            &format!("`{}.{}`", self.owner_name(), self.name()),
            args,
            span,
        )
    }

    pub const fn argument_passing(self, index: usize) -> Option<ReceiverKind> {
        let shape = self.call_shape();
        if index < shape.params.len() {
            Some(shape.params[index].passing)
        } else {
            None
        }
    }

    pub const fn argument_name(self, index: usize) -> Option<&'static str> {
        let shape = self.call_shape();
        if index < shape.params.len() {
            Some(shape.params[index].binding.name)
        } else {
            None
        }
    }

    const fn call_shape(self) -> BuiltinCallShape {
        match self {
            Self::DurationMilliseconds | Self::DurationSeconds | Self::DurationMinutes => {
                BuiltinCallShape::fixed(&DURATION_VALUE_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::StringFromBytes => BuiltinCallShape::fixed(
                &STRING_FROM_BYTES_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuiltinClassConstructor {
    RandomRng,
}

impl BuiltinClassConstructor {
    pub fn resolve(module_name: &str, class_name: &str) -> Option<Self> {
        match (module_name, class_name) {
            ("random", "Rng") => Some(Self::RandomRng),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::RandomRng => "Rng",
        }
    }

    pub const fn qualified_name(self) -> &'static str {
        match self {
            Self::RandomRng => "random.Rng",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::RandomRng => "Rng(seed: int64) -> random.Rng",
        }
    }

    pub const fn docs(self) -> &'static str {
        match self {
            Self::RandomRng => {
                "Constructs a deterministic random-number generator from a signed 64-bit seed."
            }
        }
    }

    pub fn bind_args(self, args: &[Argument], span: Span) -> Result<Vec<Option<&Argument>>> {
        bind_call_arguments(
            &format!("`{}`", self.qualified_name()),
            &RNG_SEED_PARAMS,
            args,
            span,
            CallConvention::PositionalOrNamed,
        )
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuiltinMember {
    FloatSqrt,
    IntegerToFloat,
    DurationToMilliseconds,
    DurationToSeconds,
    StringLen,
    StringByteLen,
    StringContains,
    StringStartsWith,
    StringEndsWith,
    StringSplit,
    StringReplace,
    StringToLower,
    StringToUpper,
    StringStripPrefix,
    StringStripSuffix,
    StringTrim,
    StringJoin,
    StringToBytes,
    ScalarToString,
    VecLen,
    VecIsEmpty,
    VecClone,
    VecPush,
    VecPop,
    VecGet,
    VecSet,
    VecRemove,
    VecSwap,
    VecContains,
    VecExtend,
    VecInsert,
    VecClear,
    VecReverse,
    MapLen,
    MapIsEmpty,
    MapClone,
    MapGet,
    MapSet,
    MapRemove,
    MapContainsKey,
    MapKeys,
    MapValues,
    MapItems,
    MapEntries,
    MapClear,
    MapExtend,
    SetLen,
    SetIsEmpty,
    SetClone,
    SetContains,
    SetInsert,
    SetRemove,
    StringClone,
    QueuePut,
    QueueTryPut,
    QueueGet,
    QueueGetOrNone,
    QueueGetOr,
    QueueClose,
    TaskResult,
    TaskResultOrNone,
    TaskResultOr,
    TaskGroupStart,
    TaskGroupStartSoon,
    TaskGroupCancel,
    FileReadAll,
    FileReadBytes,
    FileWriteAll,
    FileWriteBytes,
    FileFlush,
    FileClose,
    TcpListenerAccept,
    TcpListenerLocalAddr,
    TcpListenerClose,
    TcpStreamReadAll,
    TcpStreamReadLine,
    TcpStreamReadBytes,
    TcpStreamReadExact,
    TcpStreamWriteAll,
    TcpStreamWriteBytes,
    TcpStreamFlush,
    TcpStreamLocalAddr,
    TcpStreamPeerAddr,
    TcpStreamShutdownRead,
    TcpStreamShutdownWrite,
    TcpStreamShutdownBoth,
    TcpStreamClose,
    UdpSocketSendText,
    UdpSocketSendBytes,
    UdpSocketRecv,
    UdpSocketRecvFrom,
    UdpSocketLocalAddr,
    UdpSocketPeerAddr,
    UdpSocketClose,
    UdpDatagramAddress,
    UdpDatagramBytes,
    UdpDatagramText,
    HttpListenerAccept,
    HttpListenerLocalAddr,
    HttpListenerClose,
    HttpExchangeMethod,
    HttpExchangePath,
    HttpExchangeHeaders,
    HttpExchangeBodyText,
    HttpExchangeBodyBytes,
    HttpExchangeRespondText,
    HttpExchangeRespondBytes,
    HttpResponseStatus,
    HttpResponseReason,
    HttpResponseHeaders,
    HttpResponseText,
    HttpResponseBytes,
    WebSocketListenerAccept,
    WebSocketListenerLocalAddr,
    WebSocketSendText,
    WebSocketSendBytes,
    WebSocketRecvText,
    WebSocketRecvBytes,
    WebSocketClose,
    UnixListenerAccept,
    UnixListenerClose,
    UnixStreamReadLine,
    UnixStreamReadExact,
    UnixStreamWriteAll,
    UnixStreamClose,
    TlsListenerAccept,
    TlsListenerLocalAddr,
    TlsListenerClose,
    TlsStreamReadLine,
    TlsStreamReadExact,
    TlsStreamWriteAll,
    TlsStreamClose,
    ProcessChildStdin,
    ProcessChildStdout,
    ProcessChildStderr,
    ProcessChildWait,
    ProcessChildWaitOrNone,
    ProcessChildWaitOk,
    ProcessChildKill,
    ProcessChildTerminate,
    ProcessChildClose,
    ProcessPipeReadAll,
    ProcessPipeReadLine,
    ProcessPipeReadBytes,
    ProcessPipeWriteAll,
    ProcessPipeWriteBytes,
    ProcessPipeFlush,
    ProcessPipeClose,
    ProcessCompletedStatus,
    ProcessCompletedSuccess,
    ProcessCompletedStdout,
    ProcessCompletedStdoutBytes,
    ProcessCompletedStderr,
    ProcessCompletedStderrBytes,
    ProcessCompletedCheck,
    ProcessSupervisorStart,
    ProcessSupervisorWait,
    ProcessSupervisorWaitOrNone,
    ProcessSupervisorStop,
    ProcessSupervisorIsEmpty,
    ProcessSupervisorClose,
    RngNextInt,
    RngNextFloat,
    RngShuffle,
}

impl BuiltinMember {
    pub fn resolve(receiver_base: &str, name: &str) -> Option<Self> {
        match (receiver_base, name) {
            ("float64", "sqrt") => Some(Self::FloatSqrt),
            ("int8", "to_float")
            | ("int16", "to_float")
            | ("int32", "to_float")
            | ("int64", "to_float")
            | ("int128", "to_float")
            | ("intsize", "to_float")
            | ("uint8", "to_float")
            | ("uint16", "to_float")
            | ("uint32", "to_float")
            | ("uint64", "to_float")
            | ("uint128", "to_float")
            | ("uintsize", "to_float") => Some(Self::IntegerToFloat),
            ("Duration", "to_ms") => Some(Self::DurationToMilliseconds),
            ("Duration", "to_seconds") => Some(Self::DurationToSeconds),
            ("bool", "to_string") => Some(Self::ScalarToString),
            ("int8", "to_string")
            | ("int16", "to_string")
            | ("int32", "to_string")
            | ("int64", "to_string")
            | ("int128", "to_string")
            | ("intsize", "to_string")
            | ("uint8", "to_string")
            | ("uint16", "to_string")
            | ("uint32", "to_string")
            | ("uint64", "to_string")
            | ("uint128", "to_string")
            | ("uintsize", "to_string")
            | ("float32", "to_string")
            | ("float64", "to_string") => Some(Self::ScalarToString),
            ("Vec", "len") => Some(Self::VecLen),
            ("Vec", "is_empty") => Some(Self::VecIsEmpty),
            ("Vec", "clone") => Some(Self::VecClone),
            ("Vec", "push") => Some(Self::VecPush),
            ("Vec", "pop") => Some(Self::VecPop),
            ("Vec", "get") => Some(Self::VecGet),
            ("Vec", "set") => Some(Self::VecSet),
            ("Vec", "remove") => Some(Self::VecRemove),
            ("Vec", "swap") => Some(Self::VecSwap),
            ("Vec", "contains") => Some(Self::VecContains),
            ("Vec", "extend") => Some(Self::VecExtend),
            ("Vec", "insert") => Some(Self::VecInsert),
            ("Vec", "clear") => Some(Self::VecClear),
            ("Vec", "reverse") => Some(Self::VecReverse),
            ("Map", "len") => Some(Self::MapLen),
            ("Map", "is_empty") => Some(Self::MapIsEmpty),
            ("Map", "clone") => Some(Self::MapClone),
            ("Map", "get") => Some(Self::MapGet),
            ("Map", "set") => Some(Self::MapSet),
            ("Map", "remove") => Some(Self::MapRemove),
            ("Map", "contains_key") => Some(Self::MapContainsKey),
            ("Map", "keys") => Some(Self::MapKeys),
            ("Map", "values") => Some(Self::MapValues),
            ("Map", "items") => Some(Self::MapItems),
            ("Map", "entries") => Some(Self::MapEntries),
            ("Map", "clear") => Some(Self::MapClear),
            ("Map", "extend") => Some(Self::MapExtend),
            ("Set", "len") => Some(Self::SetLen),
            ("Set", "is_empty") => Some(Self::SetIsEmpty),
            ("Set", "clone") => Some(Self::SetClone),
            ("Set", "contains") => Some(Self::SetContains),
            ("Set", "insert") => Some(Self::SetInsert),
            ("Set", "remove") => Some(Self::SetRemove),
            ("String", "len") => Some(Self::StringLen),
            ("String", "byte_len") => Some(Self::StringByteLen),
            ("String", "contains") => Some(Self::StringContains),
            ("String", "starts_with") => Some(Self::StringStartsWith),
            ("String", "ends_with") => Some(Self::StringEndsWith),
            ("String", "split") => Some(Self::StringSplit),
            ("String", "replace") => Some(Self::StringReplace),
            ("String", "to_lower") => Some(Self::StringToLower),
            ("String", "to_upper") => Some(Self::StringToUpper),
            ("String", "strip_prefix") => Some(Self::StringStripPrefix),
            ("String", "strip_suffix") => Some(Self::StringStripSuffix),
            ("String", "trim") => Some(Self::StringTrim),
            ("String", "join") => Some(Self::StringJoin),
            ("String", "to_bytes") => Some(Self::StringToBytes),
            ("String", "clone") => Some(Self::StringClone),
            ("Queue", "put") => Some(Self::QueuePut),
            ("Queue", "try_put") => Some(Self::QueueTryPut),
            ("Queue", "get") => Some(Self::QueueGet),
            ("Queue", "get_or_none") => Some(Self::QueueGetOrNone),
            ("Queue", "get_or") => Some(Self::QueueGetOr),
            ("Queue", "close") => Some(Self::QueueClose),
            ("Task", "result") => Some(Self::TaskResult),
            ("Task", "result_or_none") => Some(Self::TaskResultOrNone),
            ("Task", "result_or") => Some(Self::TaskResultOr),
            ("TaskGroup", "start") => Some(Self::TaskGroupStart),
            ("TaskGroup", "start_soon") => Some(Self::TaskGroupStartSoon),
            ("TaskGroup", "cancel") => Some(Self::TaskGroupCancel),
            ("fs.File", "read_all") => Some(Self::FileReadAll),
            ("fs.File", "read_bytes") => Some(Self::FileReadBytes),
            ("fs.File", "write_all") => Some(Self::FileWriteAll),
            ("fs.File", "write_bytes") => Some(Self::FileWriteBytes),
            ("fs.File", "flush") => Some(Self::FileFlush),
            ("fs.File", "close") => Some(Self::FileClose),
            ("net.TcpListener", "accept") => Some(Self::TcpListenerAccept),
            ("net.TcpListener", "local_addr") => Some(Self::TcpListenerLocalAddr),
            ("net.TcpListener", "close") => Some(Self::TcpListenerClose),
            ("net.TcpStream", "read_all") => Some(Self::TcpStreamReadAll),
            ("net.TcpStream", "read_line") => Some(Self::TcpStreamReadLine),
            ("net.TcpStream", "read_bytes") => Some(Self::TcpStreamReadBytes),
            ("net.TcpStream", "read_exact") => Some(Self::TcpStreamReadExact),
            ("net.TcpStream", "write_all") => Some(Self::TcpStreamWriteAll),
            ("net.TcpStream", "write_bytes") => Some(Self::TcpStreamWriteBytes),
            ("net.TcpStream", "flush") => Some(Self::TcpStreamFlush),
            ("net.TcpStream", "local_addr") => Some(Self::TcpStreamLocalAddr),
            ("net.TcpStream", "peer_addr") => Some(Self::TcpStreamPeerAddr),
            ("net.TcpStream", "shutdown_read") => Some(Self::TcpStreamShutdownRead),
            ("net.TcpStream", "shutdown_write") => Some(Self::TcpStreamShutdownWrite),
            ("net.TcpStream", "shutdown_both") => Some(Self::TcpStreamShutdownBoth),
            ("net.TcpStream", "close") => Some(Self::TcpStreamClose),
            ("net.UdpSocket", "send_text") => Some(Self::UdpSocketSendText),
            ("net.UdpSocket", "send_bytes") => Some(Self::UdpSocketSendBytes),
            ("net.UdpSocket", "recv") => Some(Self::UdpSocketRecv),
            ("net.UdpSocket", "recv_from") => Some(Self::UdpSocketRecvFrom),
            ("net.UdpSocket", "local_addr") => Some(Self::UdpSocketLocalAddr),
            ("net.UdpSocket", "peer_addr") => Some(Self::UdpSocketPeerAddr),
            ("net.UdpSocket", "close") => Some(Self::UdpSocketClose),
            ("net.UdpDatagram", "address") => Some(Self::UdpDatagramAddress),
            ("net.UdpDatagram", "bytes") => Some(Self::UdpDatagramBytes),
            ("net.UdpDatagram", "text") => Some(Self::UdpDatagramText),
            ("net.HttpListener", "accept") => Some(Self::HttpListenerAccept),
            ("net.HttpListener", "local_addr") => Some(Self::HttpListenerLocalAddr),
            ("net.HttpListener", "close") => Some(Self::HttpListenerClose),
            ("net.HttpExchange", "method") => Some(Self::HttpExchangeMethod),
            ("net.HttpExchange", "path") => Some(Self::HttpExchangePath),
            ("net.HttpExchange", "headers") => Some(Self::HttpExchangeHeaders),
            ("net.HttpExchange", "body_text") => Some(Self::HttpExchangeBodyText),
            ("net.HttpExchange", "body_bytes") => Some(Self::HttpExchangeBodyBytes),
            ("net.HttpExchange", "respond_text") => Some(Self::HttpExchangeRespondText),
            ("net.HttpExchange", "respond_bytes") => Some(Self::HttpExchangeRespondBytes),
            ("net.HttpResponse", "status") => Some(Self::HttpResponseStatus),
            ("net.HttpResponse", "reason") => Some(Self::HttpResponseReason),
            ("net.HttpResponse", "headers") => Some(Self::HttpResponseHeaders),
            ("net.HttpResponse", "text") => Some(Self::HttpResponseText),
            ("net.HttpResponse", "bytes") => Some(Self::HttpResponseBytes),
            ("net.WebSocketListener", "accept") => Some(Self::WebSocketListenerAccept),
            ("net.WebSocketListener", "local_addr") => Some(Self::WebSocketListenerLocalAddr),
            ("net.WebSocket", "send_text") => Some(Self::WebSocketSendText),
            ("net.WebSocket", "send_bytes") => Some(Self::WebSocketSendBytes),
            ("net.WebSocket", "recv_text") => Some(Self::WebSocketRecvText),
            ("net.WebSocket", "recv_bytes") => Some(Self::WebSocketRecvBytes),
            ("net.WebSocket", "close") => Some(Self::WebSocketClose),
            ("net.UnixListener", "accept") => Some(Self::UnixListenerAccept),
            ("net.UnixListener", "close") => Some(Self::UnixListenerClose),
            ("net.UnixStream", "read_line") => Some(Self::UnixStreamReadLine),
            ("net.UnixStream", "read_exact") => Some(Self::UnixStreamReadExact),
            ("net.UnixStream", "write_all") => Some(Self::UnixStreamWriteAll),
            ("net.UnixStream", "close") => Some(Self::UnixStreamClose),
            ("net.TlsListener", "accept") => Some(Self::TlsListenerAccept),
            ("net.TlsListener", "local_addr") => Some(Self::TlsListenerLocalAddr),
            ("net.TlsListener", "close") => Some(Self::TlsListenerClose),
            ("net.TlsStream", "read_line") => Some(Self::TlsStreamReadLine),
            ("net.TlsStream", "read_exact") => Some(Self::TlsStreamReadExact),
            ("net.TlsStream", "write_all") => Some(Self::TlsStreamWriteAll),
            ("net.TlsStream", "close") => Some(Self::TlsStreamClose),
            ("process.Child", "stdin") => Some(Self::ProcessChildStdin),
            ("process.Child", "stdout") => Some(Self::ProcessChildStdout),
            ("process.Child", "stderr") => Some(Self::ProcessChildStderr),
            ("process.Child", "wait") => Some(Self::ProcessChildWait),
            ("process.Child", "wait_or_none") => Some(Self::ProcessChildWaitOrNone),
            ("process.Child", "wait_ok") => Some(Self::ProcessChildWaitOk),
            ("process.Child", "kill") => Some(Self::ProcessChildKill),
            ("process.Child", "terminate") => Some(Self::ProcessChildTerminate),
            ("process.Child", "close") => Some(Self::ProcessChildClose),
            ("process.Pipe", "read_all") => Some(Self::ProcessPipeReadAll),
            ("process.Pipe", "read_line") => Some(Self::ProcessPipeReadLine),
            ("process.Pipe", "read_bytes") => Some(Self::ProcessPipeReadBytes),
            ("process.Pipe", "write_all") => Some(Self::ProcessPipeWriteAll),
            ("process.Pipe", "write_bytes") => Some(Self::ProcessPipeWriteBytes),
            ("process.Pipe", "flush") => Some(Self::ProcessPipeFlush),
            ("process.Pipe", "close") => Some(Self::ProcessPipeClose),
            ("process.Completed", "status") => Some(Self::ProcessCompletedStatus),
            ("process.Completed", "success") => Some(Self::ProcessCompletedSuccess),
            ("process.Completed", "stdout") => Some(Self::ProcessCompletedStdout),
            ("process.Completed", "stdout_bytes") => Some(Self::ProcessCompletedStdoutBytes),
            ("process.Completed", "stderr") => Some(Self::ProcessCompletedStderr),
            ("process.Completed", "stderr_bytes") => Some(Self::ProcessCompletedStderrBytes),
            ("process.Completed", "check") => Some(Self::ProcessCompletedCheck),
            ("process.Supervisor", "start") => Some(Self::ProcessSupervisorStart),
            ("process.Supervisor", "wait") => Some(Self::ProcessSupervisorWait),
            ("process.Supervisor", "wait_or_none") => Some(Self::ProcessSupervisorWaitOrNone),
            ("process.Supervisor", "stop") => Some(Self::ProcessSupervisorStop),
            ("process.Supervisor", "is_empty") => Some(Self::ProcessSupervisorIsEmpty),
            ("process.Supervisor", "close") => Some(Self::ProcessSupervisorClose),
            ("random.Rng", "next_int") => Some(Self::RngNextInt),
            ("random.Rng", "next_float") => Some(Self::RngNextFloat),
            ("random.Rng", "shuffle") => Some(Self::RngShuffle),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::FloatSqrt => "sqrt",
            Self::IntegerToFloat => "to_float",
            Self::DurationToMilliseconds => "to_ms",
            Self::DurationToSeconds => "to_seconds",
            Self::ScalarToString => "to_string",
            Self::StringLen => "len",
            Self::StringByteLen => "byte_len",
            Self::StringContains => "contains",
            Self::StringStartsWith => "starts_with",
            Self::StringEndsWith => "ends_with",
            Self::StringSplit => "split",
            Self::StringReplace => "replace",
            Self::StringToLower => "to_lower",
            Self::StringToUpper => "to_upper",
            Self::StringStripPrefix => "strip_prefix",
            Self::StringStripSuffix => "strip_suffix",
            Self::StringTrim => "trim",
            Self::StringJoin => "join",
            Self::StringToBytes => "to_bytes",
            Self::VecLen => "len",
            Self::VecIsEmpty => "is_empty",
            Self::VecClone | Self::MapClone | Self::StringClone => "clone",
            Self::VecPush => "push",
            Self::VecPop => "pop",
            Self::VecGet => "get",
            Self::VecSet => "set",
            Self::VecRemove => "remove",
            Self::VecSwap => "swap",
            Self::VecContains => "contains",
            Self::VecExtend => "extend",
            Self::VecInsert => "insert",
            Self::VecClear => "clear",
            Self::VecReverse => "reverse",
            Self::MapLen => "len",
            Self::MapIsEmpty => "is_empty",
            Self::MapGet => "get",
            Self::MapSet => "set",
            Self::MapRemove => "remove",
            Self::MapContainsKey => "contains_key",
            Self::MapKeys => "keys",
            Self::MapValues => "values",
            Self::MapItems => "items",
            Self::MapEntries => "entries",
            Self::MapClear => "clear",
            Self::MapExtend => "extend",
            Self::SetLen => "len",
            Self::SetIsEmpty => "is_empty",
            Self::SetClone => "clone",
            Self::SetContains => "contains",
            Self::SetInsert => "insert",
            Self::SetRemove => "remove",
            Self::QueuePut => "put",
            Self::QueueTryPut => "try_put",
            Self::QueueGet => "get",
            Self::QueueGetOrNone => "get_or_none",
            Self::QueueGetOr => "get_or",
            Self::QueueClose => "close",
            Self::TaskResult => "result",
            Self::TaskResultOrNone => "result_or_none",
            Self::TaskResultOr => "result_or",
            Self::TaskGroupStart => "start",
            Self::TaskGroupStartSoon => "start_soon",
            Self::TaskGroupCancel => "cancel",
            Self::FileReadAll => "read_all",
            Self::FileReadBytes => "read_bytes",
            Self::FileWriteAll => "write_all",
            Self::FileWriteBytes => "write_bytes",
            Self::FileFlush => "flush",
            Self::FileClose => "close",
            Self::TcpListenerAccept => "accept",
            Self::TcpListenerLocalAddr => "local_addr",
            Self::TcpListenerClose => "close",
            Self::TcpStreamReadAll => "read_all",
            Self::TcpStreamReadLine => "read_line",
            Self::TcpStreamReadBytes => "read_bytes",
            Self::TcpStreamReadExact => "read_exact",
            Self::TcpStreamWriteAll => "write_all",
            Self::TcpStreamWriteBytes => "write_bytes",
            Self::TcpStreamFlush => "flush",
            Self::TcpStreamLocalAddr => "local_addr",
            Self::TcpStreamPeerAddr => "peer_addr",
            Self::TcpStreamShutdownRead => "shutdown_read",
            Self::TcpStreamShutdownWrite => "shutdown_write",
            Self::TcpStreamShutdownBoth => "shutdown_both",
            Self::TcpStreamClose => "close",
            Self::UdpSocketSendText => "send_text",
            Self::UdpSocketSendBytes => "send_bytes",
            Self::UdpSocketRecv => "recv",
            Self::UdpSocketRecvFrom => "recv_from",
            Self::UdpSocketLocalAddr => "local_addr",
            Self::UdpSocketPeerAddr => "peer_addr",
            Self::UdpSocketClose => "close",
            Self::UdpDatagramAddress => "address",
            Self::UdpDatagramBytes => "bytes",
            Self::UdpDatagramText => "text",
            Self::HttpListenerAccept => "accept",
            Self::HttpListenerLocalAddr => "local_addr",
            Self::HttpListenerClose => "close",
            Self::HttpExchangeMethod => "method",
            Self::HttpExchangePath => "path",
            Self::HttpExchangeHeaders => "headers",
            Self::HttpExchangeBodyText => "body_text",
            Self::HttpExchangeBodyBytes => "body_bytes",
            Self::HttpExchangeRespondText => "respond_text",
            Self::HttpExchangeRespondBytes => "respond_bytes",
            Self::HttpResponseStatus => "status",
            Self::HttpResponseReason => "reason",
            Self::HttpResponseHeaders => "headers",
            Self::HttpResponseText => "text",
            Self::HttpResponseBytes => "bytes",
            Self::WebSocketListenerAccept => "accept",
            Self::WebSocketListenerLocalAddr => "local_addr",
            Self::WebSocketSendText => "send_text",
            Self::WebSocketSendBytes => "send_bytes",
            Self::WebSocketRecvText => "recv_text",
            Self::WebSocketRecvBytes => "recv_bytes",
            Self::WebSocketClose => "close",
            Self::UnixListenerAccept => "accept",
            Self::UnixListenerClose => "close",
            Self::UnixStreamReadLine => "read_line",
            Self::UnixStreamReadExact => "read_exact",
            Self::UnixStreamWriteAll => "write_all",
            Self::UnixStreamClose => "close",
            Self::TlsListenerAccept => "accept",
            Self::TlsListenerLocalAddr => "local_addr",
            Self::TlsListenerClose => "close",
            Self::TlsStreamReadLine => "read_line",
            Self::TlsStreamReadExact => "read_exact",
            Self::TlsStreamWriteAll => "write_all",
            Self::TlsStreamClose => "close",
            Self::ProcessChildStdin => "stdin",
            Self::ProcessChildStdout => "stdout",
            Self::ProcessChildStderr => "stderr",
            Self::ProcessChildWait => "wait",
            Self::ProcessChildWaitOrNone => "wait_or_none",
            Self::ProcessChildWaitOk => "wait_ok",
            Self::ProcessChildKill => "kill",
            Self::ProcessChildTerminate => "terminate",
            Self::ProcessChildClose => "close",
            Self::ProcessPipeReadAll => "read_all",
            Self::ProcessPipeReadLine => "read_line",
            Self::ProcessPipeReadBytes => "read_bytes",
            Self::ProcessPipeWriteAll => "write_all",
            Self::ProcessPipeWriteBytes => "write_bytes",
            Self::ProcessPipeFlush => "flush",
            Self::ProcessPipeClose => "close",
            Self::ProcessCompletedStatus => "status",
            Self::ProcessCompletedSuccess => "success",
            Self::ProcessCompletedStdout => "stdout",
            Self::ProcessCompletedStdoutBytes => "stdout_bytes",
            Self::ProcessCompletedStderr => "stderr",
            Self::ProcessCompletedStderrBytes => "stderr_bytes",
            Self::ProcessCompletedCheck => "check",
            Self::ProcessSupervisorStart => "start",
            Self::ProcessSupervisorWait => "wait",
            Self::ProcessSupervisorWaitOrNone => "wait_or_none",
            Self::ProcessSupervisorStop => "stop",
            Self::ProcessSupervisorIsEmpty => "is_empty",
            Self::ProcessSupervisorClose => "close",
            Self::RngNextInt => "next_int",
            Self::RngNextFloat => "next_float",
            Self::RngShuffle => "shuffle",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::FloatSqrt => "sqrt() -> float64",
            Self::IntegerToFloat => "to_float() -> float64",
            Self::DurationToMilliseconds => "to_ms() -> float64",
            Self::DurationToSeconds => "to_seconds() -> float64",
            Self::ScalarToString => "to_string() -> String",
            Self::StringLen => "len() -> int32",
            Self::StringByteLen => "byte_len() -> int32",
            Self::StringContains => "contains(text: String) -> bool",
            Self::StringStartsWith => "starts_with(text: String) -> bool",
            Self::StringEndsWith => "ends_with(text: String) -> bool",
            Self::StringSplit => "split(text: String) -> Vec[String]",
            Self::StringReplace => "replace(from: String, to: String) -> String",
            Self::StringToLower => "to_lower() -> String",
            Self::StringToUpper => "to_upper() -> String",
            Self::StringStripPrefix => "strip_prefix(text: String) -> Option[String]",
            Self::StringStripSuffix => "strip_suffix(text: String) -> Option[String]",
            Self::StringTrim => "trim() -> String",
            Self::StringJoin => "join(parts: Vec[String]) -> String",
            Self::StringToBytes => "to_bytes() -> Vec[uint8]",
            Self::VecLen => "len() -> int32",
            Self::VecIsEmpty => "is_empty() -> bool",
            Self::VecClone => "clone() -> Vec[T]",
            Self::VecPush => "push(value: own T) -> None",
            Self::VecPop => "pop() -> Option[T]",
            Self::VecGet => "get(index: int32) -> Option[T]",
            Self::VecSet => "set(index: int32, value: own T) -> Option[T]",
            Self::VecRemove => "remove(index: int32) -> Option[T]",
            Self::VecSwap => "swap(first: int32, second: int32) -> bool",
            Self::VecContains => "contains(value: T) -> bool",
            Self::VecExtend => "extend(other: own Vec[T]) -> None",
            Self::VecInsert => "insert(index: int32, value: own T) -> bool",
            Self::VecClear => "clear() -> None",
            Self::VecReverse => "reverse() -> None",
            Self::MapLen => "len() -> int32",
            Self::MapIsEmpty => "is_empty() -> bool",
            Self::MapClone => "clone() -> Map[K, V]",
            Self::MapGet => "get(key: K) -> Option[V]",
            Self::MapSet => "set(key: own K, value: own V) -> Option[V]",
            Self::MapRemove => "remove(key: K) -> Option[V]",
            Self::MapContainsKey => "contains_key(key: K) -> bool",
            Self::MapKeys => "keys() -> Vec[K]",
            Self::MapValues => "values() -> Vec[V]",
            Self::MapItems => "items() -> Vec[MapEntry[K, V]]",
            Self::MapEntries => "entries() -> Vec[MapEntry[K, V]]",
            Self::MapClear => "clear() -> None",
            Self::MapExtend => "extend(other: own Map[K, V]) -> None",
            Self::SetLen => "len() -> int32",
            Self::SetIsEmpty => "is_empty() -> bool",
            Self::SetClone => "clone() -> Set[T]",
            Self::SetContains => "contains(value: T) -> bool",
            Self::SetInsert => "insert(value: own T) -> bool",
            Self::SetRemove => "remove(value: T) -> bool",
            Self::StringClone => "clone() -> String",
            Self::QueuePut => "put(value: own T, timeout: Duration = ...) -> Result[None, SendError[T]]",
            Self::QueueTryPut => "try_put(value: own T) -> Result[None, SendError[T]]",
            Self::QueueGet => "get(timeout: Duration = ...) -> QueueReceive[T]",
            Self::QueueGetOrNone => "get_or_none(timeout: Duration = ...) -> Option[T]",
            Self::QueueGetOr => "get_or(default: own T, timeout: Duration = ...) -> T",
            Self::QueueClose => "close() -> None",
            Self::TaskResult => "result(timeout: Duration = ...) -> TaskResult[T]",
            Self::TaskResultOrNone => "result_or_none(timeout: Duration = ...) -> Option[T]",
            Self::TaskResultOr => "result_or(default: own T, timeout: Duration = ...) -> T",
            Self::TaskGroupStart => "start(function, own ...) -> Task[T]",
            Self::TaskGroupStartSoon => "start_soon(function, own ...) -> None",
            Self::TaskGroupCancel => "cancel() -> None",
            Self::FileReadAll => "read_all() -> Result[String, io.Error]",
            Self::FileReadBytes => "read_bytes() -> Result[Vec[uint8], io.Error]",
            Self::FileWriteAll => "write_all(text: String) -> Result[None, io.Error]",
            Self::FileWriteBytes => "write_bytes(bytes: Vec[uint8]) -> Result[None, io.Error]",
            Self::FileFlush => "flush() -> Result[None, io.Error]",
            Self::FileClose => "close() -> None",
            Self::TcpListenerAccept => "accept(timeout: Duration = ...) -> Result[net.TcpStream, io.Error]",
            Self::TcpListenerLocalAddr => "local_addr() -> Result[String, io.Error]",
            Self::TcpListenerClose => "close() -> None",
            Self::TcpStreamReadAll => "read_all(timeout: Duration = ...) -> Result[String, io.Error]",
            Self::TcpStreamReadLine => "read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]",
            Self::TcpStreamReadBytes => "read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]",
            Self::TcpStreamReadExact => "read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]",
            Self::TcpStreamWriteAll => "write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]",
            Self::TcpStreamWriteBytes => "write_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]",
            Self::TcpStreamFlush => "flush() -> Result[None, io.Error]",
            Self::TcpStreamLocalAddr => "local_addr() -> Result[String, io.Error]",
            Self::TcpStreamPeerAddr => "peer_addr() -> Result[String, io.Error]",
            Self::TcpStreamShutdownRead => "shutdown_read() -> Result[None, io.Error]",
            Self::TcpStreamShutdownWrite => "shutdown_write() -> Result[None, io.Error]",
            Self::TcpStreamShutdownBoth => "shutdown_both() -> Result[None, io.Error]",
            Self::TcpStreamClose => "close() -> None",
            Self::UdpSocketSendText => "send_text(address: String, text: String, timeout: Duration = ...) -> Result[None, io.Error]",
            Self::UdpSocketSendBytes => "send_bytes(address: String, bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]",
            Self::UdpSocketRecv => "recv(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]",
            Self::UdpSocketRecvFrom => "recv_from(max_bytes: int32, timeout: Duration = ...) -> Result[Option[net.UdpDatagram], io.Error]",
            Self::UdpSocketLocalAddr => "local_addr() -> Result[String, io.Error]",
            Self::UdpSocketPeerAddr => "peer_addr() -> Result[String, io.Error]",
            Self::UdpSocketClose => "close() -> None",
            Self::UdpDatagramAddress => "address() -> String",
            Self::UdpDatagramBytes => "bytes() -> Vec[uint8]",
            Self::UdpDatagramText => "text() -> Result[String, io.Error]",
            Self::HttpListenerAccept => "accept(timeout: Duration = ...) -> Result[net.HttpExchange, io.Error]",
            Self::HttpListenerLocalAddr => "local_addr() -> Result[String, io.Error]",
            Self::HttpListenerClose => "close() -> None",
            Self::HttpExchangeMethod => "method() -> String",
            Self::HttpExchangePath => "path() -> String",
            Self::HttpExchangeHeaders => "headers() -> Map[String, String]",
            Self::HttpExchangeBodyText => "body_text() -> Result[String, io.Error]",
            Self::HttpExchangeBodyBytes => "body_bytes() -> Vec[uint8]",
            Self::HttpExchangeRespondText => "respond_text(status: int32, text: own String, headers: own Map[String, String]) -> Result[None, io.Error]",
            Self::HttpExchangeRespondBytes => "respond_bytes(status: int32, bytes: own Vec[uint8], headers: own Map[String, String]) -> Result[None, io.Error]",
            Self::HttpResponseStatus => "status() -> int32",
            Self::HttpResponseReason => "reason() -> String",
            Self::HttpResponseHeaders => "headers() -> Map[String, String]",
            Self::HttpResponseText => "text() -> Result[String, io.Error]",
            Self::HttpResponseBytes => "bytes() -> Vec[uint8]",
            Self::WebSocketListenerAccept => "accept(timeout: Duration = ...) -> Result[net.WebSocket, io.Error]",
            Self::WebSocketListenerLocalAddr => "local_addr() -> Result[String, io.Error]",
            Self::WebSocketSendText => "send_text(text: String, timeout: Duration = ...) -> Result[None, io.Error]",
            Self::WebSocketSendBytes => "send_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]",
            Self::WebSocketRecvText => "recv_text(timeout: Duration = ...) -> Result[Option[String], io.Error]",
            Self::WebSocketRecvBytes => "recv_bytes(timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]",
            Self::WebSocketClose => "close() -> None",
            Self::UnixListenerAccept => "accept(timeout: Duration = ...) -> Result[net.UnixStream, io.Error]",
            Self::UnixListenerClose => "close() -> None",
            Self::UnixStreamReadLine => "read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]",
            Self::UnixStreamReadExact => "read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]",
            Self::UnixStreamWriteAll => "write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]",
            Self::UnixStreamClose => "close() -> None",
            Self::TlsListenerAccept => "accept(timeout: Duration = ...) -> Result[net.TlsStream, io.Error]",
            Self::TlsListenerLocalAddr => "local_addr() -> Result[String, io.Error]",
            Self::TlsListenerClose => "close() -> None",
            Self::TlsStreamReadLine => "read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]",
            Self::TlsStreamReadExact => "read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]",
            Self::TlsStreamWriteAll => "write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]",
            Self::TlsStreamClose => "close() -> None",
            Self::ProcessChildStdin => "stdin() -> Option[process.Pipe]",
            Self::ProcessChildStdout => "stdout() -> Option[process.Pipe]",
            Self::ProcessChildStderr => "stderr() -> Option[process.Pipe]",
            Self::ProcessChildWait => "wait(timeout: Duration = ...) -> process.Wait",
            Self::ProcessChildWaitOrNone => {
                "wait_or_none(timeout: Duration = ...) -> Result[Option[process.ExitStatus], process.Error]"
            }
            Self::ProcessChildWaitOk => {
                "wait_ok(timeout: Duration = ...) -> Result[process.ExitStatus, process.Error]"
            }
            Self::ProcessChildKill => "kill() -> Result[None, process.Error]",
            Self::ProcessChildTerminate => "terminate() -> Result[None, process.Error]",
            Self::ProcessChildClose => "close() -> None",
            Self::ProcessPipeReadAll => "read_all() -> Result[String, process.Error]",
            Self::ProcessPipeReadLine => {
                "read_line(timeout: Duration = ...) -> Result[Option[String], process.Error]"
            }
            Self::ProcessPipeReadBytes => {
                "read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], process.Error]"
            }
            Self::ProcessPipeWriteAll => {
                "write_all(text: String, timeout: Duration = ...) -> Result[None, process.Error]"
            }
            Self::ProcessPipeWriteBytes => {
                "write_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, process.Error]"
            }
            Self::ProcessPipeFlush => "flush() -> Result[None, process.Error]",
            Self::ProcessPipeClose => "close() -> None",
            Self::ProcessCompletedStatus => "status() -> process.ExitStatus",
            Self::ProcessCompletedSuccess => "success() -> bool",
            Self::ProcessCompletedStdout => "stdout() -> String",
            Self::ProcessCompletedStdoutBytes => "stdout_bytes() -> Vec[uint8]",
            Self::ProcessCompletedStderr => "stderr() -> String",
            Self::ProcessCompletedStderrBytes => "stderr_bytes() -> Vec[uint8]",
            Self::ProcessCompletedCheck => "check() -> Result[None, process.Error]",
            Self::ProcessSupervisorStart => "start(name: own String, command: own Vec[String], cwd: own Option[String] = ..., env: own Map[String, String] = ..., stdin: own process.Stdio = ..., stdout: own process.Stdio = ..., stderr: own process.Stdio = ..., restart: own process.RestartPolicy = ..., backoff: own Duration = ..., max_restarts: own int32 = ..., group: own bool = ...) -> Result[None, process.Error]",
            Self::ProcessSupervisorWait => {
                "wait(timeout: Duration = ...) -> process.SupervisorWait"
            }
            Self::ProcessSupervisorWaitOrNone => {
                "wait_or_none(timeout: Duration = ...) -> Result[Option[process.SupervisorEvent], process.Error]"
            }
            Self::ProcessSupervisorStop => "stop() -> Result[None, process.Error]",
            Self::ProcessSupervisorIsEmpty => "is_empty() -> bool",
            Self::ProcessSupervisorClose => "close() -> None",
            Self::RngNextInt => "next_int(lo: int64, hi: int64) -> int64",
            Self::RngNextFloat => "next_float() -> float64",
            Self::RngShuffle => "shuffle(values: borrow mut Vec[T]) -> None",
        }
    }

    pub const fn docs(self) -> &'static str {
        match self {
            Self::FloatSqrt => "Returns the square root of a `float64` value.",
            Self::IntegerToFloat => {
                "Converts an integer to the nearest `float64` value; large values may round."
            }
            Self::DurationToMilliseconds => {
                "Converts the Duration to the nearest representable number of milliseconds as `float64`."
            }
            Self::DurationToSeconds => {
                "Converts the Duration to the nearest representable number of seconds as `float64`."
            }
            Self::ScalarToString => "Returns a `String` rendering of a numeric or `bool` value.",
            Self::StringLen => {
                "Returns the number of Unicode scalar values in the string in O(n) time."
            }
            Self::StringByteLen => {
                "Returns the number of UTF-8 bytes in the string in O(1) time."
            }
            Self::StringContains => "Returns true when the string contains `text`.",
            Self::StringStartsWith => "Returns true when the string starts with `text`.",
            Self::StringEndsWith => "Returns true when the string ends with `text`.",
            Self::StringSplit => {
                "Splits the string on each occurrence of `text` and returns the pieces as `Vec[String]`."
            }
            Self::StringReplace => {
                "Returns a new `String` with each occurrence of `from` replaced by `to`."
            }
            Self::StringToLower => {
                "Returns a new `String` with Unicode lowercase conversion applied."
            }
            Self::StringToUpper => {
                "Returns a new `String` with Unicode uppercase conversion applied."
            }
            Self::StringStripPrefix => {
                "Removes `text` from the front of the string and returns the remaining `String`, or `Option.None` when it does not match."
            }
            Self::StringStripSuffix => {
                "Removes `text` from the end of the string and returns the remaining `String`, or `Option.None` when it does not match."
            }
            Self::StringTrim => {
                "Returns a new `String` with surrounding Unicode whitespace removed."
            }
            Self::StringJoin => {
                "Joins the `Vec[String]` parts using the receiver string as the separator."
            }
            Self::StringToBytes => {
                "Returns a fresh `Vec[uint8]` containing the string's exact UTF-8 encoding."
            }
            Self::VecLen => "Returns the current number of elements in the vector.",
            Self::VecIsEmpty => "Returns true when the vector contains no elements.",
            Self::VecClone => "Creates a new owned `Vec[T]` with cloned element values.",
            Self::VecPush => "Appends a value to the end of the vector.",
            Self::VecPop => "Removes and returns the final element, or `Option.None` when empty.",
            Self::VecGet => {
                "Returns the element at `index`, or `Option.None` when the index is out of bounds."
            }
            Self::VecSet => {
                "Replaces the element at `index` and returns the previous element. Out-of-bounds indices raise a runtime error."
            }
            Self::VecRemove => {
                "Removes the element at `index` and returns it. Out-of-bounds indices raise a runtime error."
            }
            Self::VecSwap => {
                "Swaps the elements at `first` and `second` and returns `true`. Out-of-bounds indices raise a runtime error."
            }
            Self::VecContains => "Returns true when the vector contains `value`.",
            Self::VecExtend => "Appends the elements of `other` to the end of the vector.",
            Self::VecInsert => {
                "Inserts `value` at `index` and returns `true`. Out-of-bounds indices raise a runtime error."
            }
            Self::VecClear => "Removes all elements from the vector.",
            Self::VecReverse => "Reverses the vector elements in place.",
            Self::MapLen => "Returns the current number of entries in the map.",
            Self::MapIsEmpty => "Returns true when the map contains no entries.",
            Self::MapClone => "Creates a new owned `Map[K, V]` with cloned keys and values.",
            Self::MapGet => {
                "Returns the value for `key`, or `Option.None` when the key is absent."
            }
            Self::MapSet => {
                "Inserts or replaces `key`, returning the previous value as `Option[V]`."
            }
            Self::MapRemove => {
                "Removes `key` and returns its previous value, or `Option.None` when absent."
            }
            Self::MapContainsKey => "Returns true when the map contains `key`.",
            Self::MapKeys => "Returns the current keys as a `Vec[K]`.",
            Self::MapValues => "Returns the current values as a `Vec[V]`.",
            Self::MapItems => "Returns the current entries as `Vec[MapEntry[K, V]]` in insertion order.",
            Self::MapEntries => "Returns the current entries as `Vec[MapEntry[K, V]]` in insertion order.",
            Self::MapClear => "Removes all entries from the map.",
            Self::MapExtend => "Inserts the entries from `other`, replacing matching keys.",
            Self::SetLen => "Returns the current number of elements in the set.",
            Self::SetIsEmpty => "Returns true when the set contains no elements.",
            Self::SetClone => "Creates a new owned `Set[T]` with cloned element values.",
            Self::SetContains => "Returns true when the set contains `value`.",
            Self::SetInsert => "Inserts `value`, returning false when it is already present.",
            Self::SetRemove => "Removes `value`, returning false when it is absent.",
            Self::StringClone => "Creates a new owned `String` with the same contents.",
            Self::QueuePut => {
                "Puts a value into the queue, waiting for capacity when needed, or returns `SendError.Closed(value)`, `SendError.Cancelled(value)`, or `SendError.TimedOut(value)` if the send cannot complete."
            }
            Self::QueueTryPut => {
                "Attempts to put a value into the queue without waiting and returns `SendError.Full(value)` when the queue is already at capacity."
            }
            Self::QueueGet => {
                "Receives the next queue outcome as `QueueReceive.Item(value)`, `QueueReceive.Closed`, `QueueReceive.TimedOut`, or `QueueReceive.Cancelled`."
            }
            Self::QueueGetOrNone => {
                "Receives the next queue value and returns `Option.Some(value)`, or `Option.None` when the queue is closed, the timeout expires, or cancellation interrupts the wait."
            }
            Self::QueueGetOr => {
                "Receives the next queue value or returns `default` when the queue is closed, the timeout expires, or cancellation interrupts the wait."
            }
            Self::QueueClose => "Closes the queue and wakes blocked receivers.",
            Self::TaskResult => {
                "Waits for the task to finish and reports `TaskResult.Ready(value)`, `TaskResult.Error(message)`, `TaskResult.TimedOut`, or `TaskResult.Cancelled`."
            }
            Self::TaskResultOrNone => {
                "Waits for the task result and returns `Option.Some(value)`, or `Option.None` when the task fails, the timeout expires, or cancellation interrupts the wait."
            }
            Self::TaskResultOr => {
                "Waits for the task result or returns `default` when the task fails, the timeout expires, or cancellation interrupts the wait."
            }
            Self::TaskGroupStart => "Starts a child task in the current task group.",
            Self::TaskGroupStartSoon => {
                "Starts a child task in the current task group without returning a task handle."
            }
            Self::TaskGroupCancel => {
                "Signals cancellation to child tasks in the current task group."
            }
            Self::FileReadAll => "Reads the remaining file contents into a `String`.",
            Self::FileReadBytes => "Reads the remaining file contents into `Vec[uint8]`.",
            Self::FileWriteAll => "Writes all of `text` to the file, returning an `io.Error` on failure.",
            Self::FileWriteBytes => "Writes all of `bytes` to the file, returning an `io.Error` on failure.",
            Self::FileFlush => "Flushes pending file writes to the operating system.",
            Self::FileClose => "Closes the file handle so the resource can no longer be used.",
            Self::TcpListenerAccept => "Accepts the next incoming TCP connection, optionally timing out.",
            Self::TcpListenerLocalAddr => "Returns the bound local address for the listener.",
            Self::TcpListenerClose => "Closes the TCP listener handle.",
            Self::TcpStreamReadAll => "Reads the remaining TCP stream contents into a `String` until the peer closes.",
            Self::TcpStreamReadLine => "Reads a UTF-8 line from the TCP stream, returning `Option.None` on EOF.",
            Self::TcpStreamReadBytes => "Reads up to `max_bytes` raw bytes from the TCP stream.",
            Self::TcpStreamReadExact => "Reads exactly `count` raw bytes from the TCP stream or returns an `io.Error`.",
            Self::TcpStreamWriteAll => "Writes all of `text` to the TCP stream.",
            Self::TcpStreamWriteBytes => "Writes all of `bytes` to the TCP stream.",
            Self::TcpStreamFlush => "Flushes pending TCP stream writes.",
            Self::TcpStreamLocalAddr => "Returns the local address for the TCP stream.",
            Self::TcpStreamPeerAddr => "Returns the connected peer address for the TCP stream.",
            Self::TcpStreamShutdownRead => "Shuts down the read half of the TCP stream.",
            Self::TcpStreamShutdownWrite => "Shuts down the write half of the TCP stream.",
            Self::TcpStreamShutdownBoth => "Shuts down both halves of the TCP stream.",
            Self::TcpStreamClose => "Closes the TCP stream handle.",
            Self::UdpSocketSendText => "Sends UTF-8 text to a UDP address.",
            Self::UdpSocketSendBytes => "Sends raw bytes to a UDP address.",
            Self::UdpSocketRecv => "Receives raw bytes from a connected UDP socket.",
            Self::UdpSocketRecvFrom => "Receives a datagram and source address from a UDP socket.",
            Self::UdpSocketLocalAddr => "Returns the local address for the UDP socket.",
            Self::UdpSocketPeerAddr => "Returns the connected peer address for the UDP socket.",
            Self::UdpSocketClose => "Closes the UDP socket handle.",
            Self::UdpDatagramAddress => "Returns the source address for the UDP datagram.",
            Self::UdpDatagramBytes => "Returns the datagram payload as raw bytes.",
            Self::UdpDatagramText => "Decodes the datagram payload as UTF-8 text.",
            Self::HttpListenerAccept => "Accepts the next incoming HTTP request.",
            Self::HttpListenerLocalAddr => "Returns the bound local address for the HTTP listener.",
            Self::HttpListenerClose => "Closes the HTTP listener handle.",
            Self::HttpExchangeMethod => "Returns the HTTP request method.",
            Self::HttpExchangePath => "Returns the HTTP request path.",
            Self::HttpExchangeHeaders => "Returns the HTTP request headers as a map.",
            Self::HttpExchangeBodyText => "Returns the HTTP request body decoded as UTF-8.",
            Self::HttpExchangeBodyBytes => "Returns the HTTP request body as raw bytes.",
            Self::HttpExchangeRespondText => "Sends a text HTTP response for the current request.",
            Self::HttpExchangeRespondBytes => "Sends a binary HTTP response for the current request.",
            Self::HttpResponseStatus => "Returns the HTTP response status code.",
            Self::HttpResponseReason => "Returns the HTTP response reason phrase.",
            Self::HttpResponseHeaders => "Returns the HTTP response headers as a map.",
            Self::HttpResponseText => "Returns the HTTP response body decoded as UTF-8.",
            Self::HttpResponseBytes => "Returns the HTTP response body as raw bytes.",
            Self::WebSocketListenerAccept => "Accepts the next incoming WebSocket connection.",
            Self::WebSocketListenerLocalAddr => "Returns the bound local address for the WebSocket listener.",
            Self::WebSocketSendText => "Sends a text WebSocket frame.",
            Self::WebSocketSendBytes => "Sends a binary WebSocket frame.",
            Self::WebSocketRecvText => "Receives the next text WebSocket frame.",
            Self::WebSocketRecvBytes => "Receives the next binary WebSocket frame.",
            Self::WebSocketClose => "Closes the WebSocket connection.",
            Self::UnixListenerAccept => "Accepts the next incoming Unix domain stream connection.",
            Self::UnixListenerClose => "Closes the Unix listener handle.",
            Self::UnixStreamReadLine => "Reads a UTF-8 line from the Unix stream.",
            Self::UnixStreamReadExact => "Reads exactly `count` bytes from the Unix stream.",
            Self::UnixStreamWriteAll => "Writes all of `text` to the Unix stream.",
            Self::UnixStreamClose => "Closes the Unix stream handle.",
            Self::TlsListenerAccept => "Accepts the next incoming TLS connection.",
            Self::TlsListenerLocalAddr => "Returns the bound local address for the TLS listener.",
            Self::TlsListenerClose => "Closes the TLS listener handle.",
            Self::TlsStreamReadLine => "Reads a UTF-8 line from the TLS stream.",
            Self::TlsStreamReadExact => "Reads exactly `count` bytes from the TLS stream.",
            Self::TlsStreamWriteAll => "Writes all of `text` to the TLS stream.",
            Self::TlsStreamClose => "Closes the TLS stream handle.",
            Self::ProcessChildStdin => "Returns the child's piped stdin handle when stdin was configured with `process.pipe()`.",
            Self::ProcessChildStdout => "Returns the child's piped stdout handle when stdout was configured with `process.pipe()`.",
            Self::ProcessChildStderr => "Returns the child's piped stderr handle when stderr was configured with `process.pipe()`.",
            Self::ProcessChildWait => "Waits for the child process to exit and reports exit, timeout, cancellation, or wait failure.",
            Self::ProcessChildWaitOrNone => {
                "Waits for the child process to exit and returns `Result.Ok(Option.Some(status))`, `Result.Ok(Option.None)` on timeout, or `Result.Err(...)` for cancellation or wait failures."
            }
            Self::ProcessChildWaitOk => {
                "Waits for the child process to exit and returns the exit status for a successful exit, or `process.Error` for timeouts, cancellation, wait failures, or non-zero exits."
            }
            Self::ProcessChildKill => "Immediately kills the child process.",
            Self::ProcessChildTerminate => "Requests graceful child-process termination.",
            Self::ProcessChildClose => "Closes the child resource, terminating it if it is still running.",
            Self::ProcessPipeReadAll => "Reads the remaining piped output into a String until EOF.",
            Self::ProcessPipeReadLine => "Reads a UTF-8 line from the process pipe, returning `Option.None` on EOF.",
            Self::ProcessPipeReadBytes => "Reads up to `max_bytes` raw bytes from the process pipe.",
            Self::ProcessPipeWriteAll => "Writes all of `text` to the process pipe.",
            Self::ProcessPipeWriteBytes => "Writes all of `bytes` to the process pipe.",
            Self::ProcessPipeFlush => "Flushes pending process-pipe writes.",
            Self::ProcessPipeClose => "Closes the process pipe handle.",
            Self::ProcessCompletedStatus => "Returns the process exit status captured by `process.run(...)`.",
            Self::ProcessCompletedSuccess => "Returns true when the completed process exited with code 0.",
            Self::ProcessCompletedStdout => "Returns the UTF-8 stdout captured by `process.run(...)`.",
            Self::ProcessCompletedStdoutBytes => "Returns the raw stdout bytes captured by `process.run(...)`.",
            Self::ProcessCompletedStderr => "Returns the UTF-8 stderr captured by `process.run(...)`.",
            Self::ProcessCompletedStderrBytes => "Returns the raw stderr bytes captured by `process.run(...)`.",
            Self::ProcessCompletedCheck => {
                "Returns `Result.Ok(None)` when the completed process exited successfully, or `Result.Err(process.Error)` for abnormal exits."
            }
            Self::ProcessSupervisorStart => "Starts a named supervised child process using the configured restart policy and process-group behavior.",
            Self::ProcessSupervisorWait => "Waits for the next supervisor event, timeout, or cancellation outcome.",
            Self::ProcessSupervisorWaitOrNone => {
                "Waits for the next supervisor event and returns `Result.Ok(Option.Some(event))`, `Result.Ok(Option.None)` on timeout, or `Result.Err(...)` when the wait was cancelled."
            }
            Self::ProcessSupervisorStop => "Stops every supervised child and clears the supervisor.",
            Self::ProcessSupervisorIsEmpty => "Returns true when the supervisor has no running or pending services.",
            Self::ProcessSupervisorClose => "Closes the supervisor, stopping all managed children.",
            Self::RngNextInt => {
                "Returns an unbiased deterministic integer from the half-open interval `[lo, hi)`."
            }
            Self::RngNextFloat => {
                "Returns a deterministic `float64` value from the half-open interval `[0, 1)`."
            }
            Self::RngShuffle => {
                "Shuffles `values` in place using this generator's deterministic state."
            }
        }
    }

    const fn call_shape(self) -> BuiltinCallShape {
        match self {
            Self::FloatSqrt
            | Self::IntegerToFloat
            | Self::DurationToMilliseconds
            | Self::DurationToSeconds
            | Self::ScalarToString
            | Self::StringLen
            | Self::StringByteLen
            | Self::StringToLower
            | Self::StringToUpper
            | Self::StringTrim
            | Self::StringToBytes
            | Self::VecLen
            | Self::VecIsEmpty
            | Self::VecClone
            | Self::VecClear
            | Self::VecReverse
            | Self::MapLen
            | Self::MapIsEmpty
            | Self::MapClone
            | Self::MapKeys
            | Self::MapValues
            | Self::MapItems
            | Self::MapEntries
            | Self::MapClear
            | Self::SetLen
            | Self::SetIsEmpty
            | Self::SetClone
            | Self::VecPop
            | Self::StringClone
            | Self::QueueClose
            | Self::TaskGroupCancel
            | Self::FileReadAll
            | Self::FileReadBytes
            | Self::FileFlush
            | Self::FileClose
            | Self::TcpListenerLocalAddr
            | Self::TcpListenerClose
            | Self::TcpStreamFlush
            | Self::TcpStreamLocalAddr
            | Self::TcpStreamPeerAddr
            | Self::TcpStreamShutdownRead
            | Self::TcpStreamShutdownWrite
            | Self::TcpStreamShutdownBoth
            | Self::TcpStreamClose
            | Self::UdpSocketLocalAddr
            | Self::UdpSocketPeerAddr
            | Self::UdpSocketClose
            | Self::UdpDatagramAddress
            | Self::UdpDatagramBytes
            | Self::UdpDatagramText
            | Self::HttpListenerLocalAddr
            | Self::HttpListenerClose
            | Self::HttpExchangeMethod
            | Self::HttpExchangePath
            | Self::HttpExchangeHeaders
            | Self::HttpExchangeBodyText
            | Self::HttpExchangeBodyBytes
            | Self::HttpResponseStatus
            | Self::HttpResponseReason
            | Self::HttpResponseHeaders
            | Self::HttpResponseText
            | Self::HttpResponseBytes
            | Self::WebSocketListenerLocalAddr
            | Self::WebSocketClose
            | Self::UnixListenerClose
            | Self::UnixStreamClose
            | Self::TlsListenerLocalAddr
            | Self::TlsListenerClose
            | Self::TlsStreamClose
            | Self::ProcessChildStdin
            | Self::ProcessChildStdout
            | Self::ProcessChildStderr
            | Self::ProcessChildKill
            | Self::ProcessChildTerminate
            | Self::ProcessChildClose
            | Self::ProcessPipeReadAll
            | Self::ProcessPipeFlush
            | Self::ProcessPipeClose
            | Self::ProcessCompletedStatus
            | Self::ProcessCompletedSuccess
            | Self::ProcessCompletedStdout
            | Self::ProcessCompletedStdoutBytes
            | Self::ProcessCompletedStderr
            | Self::ProcessCompletedStderrBytes
            | Self::ProcessCompletedCheck
            | Self::ProcessSupervisorStop
            | Self::ProcessSupervisorIsEmpty
            | Self::ProcessSupervisorClose
            | Self::RngNextFloat => {
                BuiltinCallShape::fixed(&NO_BUILTIN_PARAMS, CallConvention::PositionalOnly)
            }
            Self::RngNextInt => {
                BuiltinCallShape::fixed(&RNG_NEXT_INT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::RngShuffle => {
                BuiltinCallShape::fixed(&RNG_SHUFFLE_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::TcpListenerAccept
            | Self::TcpStreamReadAll
            | Self::TcpStreamReadLine
            | Self::WebSocketRecvText
            | Self::WebSocketRecvBytes
            | Self::WebSocketListenerAccept
            | Self::HttpListenerAccept
            | Self::UnixListenerAccept
            | Self::UnixStreamReadLine
            | Self::TlsListenerAccept
            | Self::TlsStreamReadLine
            | Self::ProcessChildWait
            | Self::ProcessChildWaitOrNone
            | Self::ProcessChildWaitOk
            | Self::ProcessPipeReadLine
            | Self::ProcessSupervisorWait
            | Self::ProcessSupervisorWaitOrNone
            | Self::QueueGetOrNone
            | Self::TaskResult
            | Self::TaskResultOrNone => {
                BuiltinCallShape::fixed(&TIMEOUT_ONLY_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::QueueGet => {
                BuiltinCallShape::fixed(&QUEUE_GET_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::QueueGetOr | Self::TaskResultOr => {
                BuiltinCallShape::fixed(&DEFAULT_TIMEOUT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::VecGet | Self::VecRemove => {
                BuiltinCallShape::fixed(&VEC_INDEX_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::VecPush | Self::QueueTryPut => {
                BuiltinCallShape::fixed(&VEC_PUSH_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::VecSet => {
                BuiltinCallShape::fixed(&VEC_SET_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::VecSwap => {
                BuiltinCallShape::fixed(&VEC_SWAP_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::VecContains => {
                BuiltinCallShape::fixed(&VALUE_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::VecExtend => {
                BuiltinCallShape::fixed(&VEC_EXTEND_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::VecInsert => {
                BuiltinCallShape::fixed(&VEC_INSERT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::StringContains
            | Self::StringStartsWith
            | Self::StringEndsWith
            | Self::StringSplit
            | Self::StringStripPrefix
            | Self::StringStripSuffix => {
                BuiltinCallShape::fixed(&STRING_TEXT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::StringReplace => {
                BuiltinCallShape::fixed(&STRING_REPLACE_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::StringJoin => {
                BuiltinCallShape::fixed(&STRING_JOIN_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::MapGet | Self::MapRemove | Self::MapContainsKey => {
                BuiltinCallShape::fixed(&MAP_KEY_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::MapSet => {
                BuiltinCallShape::fixed(&MAP_SET_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::MapExtend => {
                BuiltinCallShape::fixed(&MAP_EXTEND_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::SetContains | Self::SetRemove => {
                BuiltinCallShape::fixed(&SET_VALUE_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::SetInsert => {
                BuiltinCallShape::fixed(&SET_INSERT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::QueuePut => {
                BuiltinCallShape::fixed(&VALUE_TIMEOUT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::FileWriteAll => {
                BuiltinCallShape::fixed(&FILE_WRITE_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::FileWriteBytes => {
                BuiltinCallShape::fixed(&FILE_WRITE_BYTES_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::TcpStreamWriteAll
            | Self::UnixStreamWriteAll
            | Self::TlsStreamWriteAll
            | Self::ProcessPipeWriteAll
            | Self::WebSocketSendText => {
                BuiltinCallShape::fixed(&TEXT_TIMEOUT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::TcpStreamWriteBytes | Self::WebSocketSendBytes | Self::ProcessPipeWriteBytes => {
                BuiltinCallShape::fixed(&BYTES_TIMEOUT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::TcpStreamReadBytes
            | Self::UdpSocketRecv
            | Self::UdpSocketRecvFrom
            | Self::ProcessPipeReadBytes => BuiltinCallShape::fixed(
                &MAX_BYTES_TIMEOUT_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
            Self::TcpStreamReadExact | Self::UnixStreamReadExact | Self::TlsStreamReadExact => {
                BuiltinCallShape::fixed(&COUNT_TIMEOUT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::ProcessSupervisorStart => BuiltinCallShape::fixed(
                &PROCESS_SUPERVISOR_START_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
            Self::UdpSocketSendText => BuiltinCallShape::fixed(
                &ADDRESS_TEXT_TIMEOUT_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
            Self::UdpSocketSendBytes => BuiltinCallShape::fixed(
                &ADDRESS_BYTES_TIMEOUT_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
            Self::HttpExchangeRespondText => BuiltinCallShape::fixed(
                &STATUS_TEXT_HEADERS_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
            Self::HttpExchangeRespondBytes => BuiltinCallShape::fixed(
                &STATUS_BYTES_HEADERS_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
            Self::TaskGroupStart | Self::TaskGroupStartSoon => {
                BuiltinCallShape::variadic(&TASK_GROUP_START_PARAMS, ReceiverKind::Value)
            }
        }
    }

    pub fn bind_args(self, args: &[Argument], span: Span) -> Result<Vec<Option<&Argument>>> {
        self.call_shape()
            .bind_args(&format!("`{}`", self.name()), args, span)
    }

    pub const fn argument_passing(self, index: usize) -> Option<ReceiverKind> {
        let shape = self.call_shape();
        if index < shape.params.len() {
            Some(shape.params[index].passing)
        } else {
            shape.variadic_passing
        }
    }

    pub const fn variadic_argument_passing(self) -> Option<ReceiverKind> {
        self.call_shape().variadic_passing
    }

    pub const fn receiver_passing(self) -> ReceiverKind {
        if matches!(
            self,
            Self::VecPush
                | Self::VecPop
                | Self::VecSet
                | Self::VecRemove
                | Self::VecSwap
                | Self::VecExtend
                | Self::VecInsert
                | Self::VecClear
                | Self::VecReverse
                | Self::MapSet
                | Self::MapRemove
                | Self::MapClear
                | Self::MapExtend
                | Self::SetInsert
                | Self::SetRemove
                | Self::QueuePut
                | Self::QueueTryPut
                | Self::QueueClose
                | Self::FileWriteAll
                | Self::FileWriteBytes
                | Self::FileFlush
                | Self::FileClose
                | Self::TcpListenerClose
                | Self::TcpStreamWriteAll
                | Self::TcpStreamWriteBytes
                | Self::TcpStreamFlush
                | Self::TcpStreamShutdownRead
                | Self::TcpStreamShutdownWrite
                | Self::TcpStreamShutdownBoth
                | Self::TcpStreamClose
                | Self::UdpSocketSendText
                | Self::UdpSocketSendBytes
                | Self::UdpSocketClose
                | Self::HttpListenerClose
                | Self::HttpExchangeRespondText
                | Self::HttpExchangeRespondBytes
                | Self::WebSocketSendText
                | Self::WebSocketSendBytes
                | Self::WebSocketClose
                | Self::UnixListenerClose
                | Self::UnixStreamWriteAll
                | Self::UnixStreamClose
                | Self::TlsListenerClose
                | Self::TlsStreamWriteAll
                | Self::TlsStreamClose
                | Self::ProcessChildKill
                | Self::ProcessChildTerminate
                | Self::ProcessChildClose
                | Self::ProcessPipeWriteAll
                | Self::ProcessPipeWriteBytes
                | Self::ProcessPipeFlush
                | Self::ProcessPipeClose
                | Self::ProcessSupervisorStart
                | Self::ProcessSupervisorStop
                | Self::ProcessSupervisorClose
                | Self::RngNextInt
                | Self::RngNextFloat
                | Self::RngShuffle
        ) {
            ReceiverKind::BorrowMut
        } else {
            ReceiverKind::Borrow
        }
    }
}

#[cfg(test)]
#[path = "call_tests.rs"]
mod tests;
