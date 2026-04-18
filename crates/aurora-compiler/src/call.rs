use std::collections::HashMap;

use crate::ast::{Argument, Param};
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
        .map(|param| {
            if param.default.is_some() {
                CallableParam::optional(&param.name)
            } else {
                CallableParam::required(&param.name)
            }
        })
        .collect()
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
                return Err(Diagnostic::at(
                    argument.span,
                    format!("{} has no parameter named `{}`", callee_name, name),
                ));
            };
            if ordered_args[param_index].is_some() {
                return Err(Diagnostic::at(
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

        debug_assert!(
            ordered_args[next_positional].is_none(),
            "internal error: positional argument binding attempted to reuse parameter slot {} in {}",
            next_positional,
            callee_name
        );
        if ordered_args[next_positional].is_some() {
            return Err(Diagnostic::at(
                argument.span,
                format!(
                    "internal error: argument binding reused parameter slot `{}` in {}",
                    params[next_positional].name, callee_name
                ),
            ));
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
const AFTER_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("duration")];
const SLEEP_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("duration")];
const QUEUE_PUT_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("value")];
const QUEUE_GET_PARAMS: [CallableParam<'static>; 1] = [CallableParam::optional("timeout")];
const VEC_INDEX_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("index")];
const VEC_PUSH_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("value")];
const VEC_SET_PARAMS: [CallableParam<'static>; 2] = [
    CallableParam::required("index"),
    CallableParam::required("value"),
];
const VEC_SWAP_PARAMS: [CallableParam<'static>; 2] = [
    CallableParam::required("first"),
    CallableParam::required("second"),
];
const VEC_INSERT_PARAMS: [CallableParam<'static>; 2] = [
    CallableParam::required("index"),
    CallableParam::required("value"),
];
const VEC_EXTEND_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("other")];
const STRING_TEXT_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("text")];
const STRING_REPLACE_PARAMS: [CallableParam<'static>; 2] = [
    CallableParam::required("from"),
    CallableParam::required("to"),
];
const STRING_JOIN_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("parts")];
const MAP_KEY_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("key")];
const MAP_SET_PARAMS: [CallableParam<'static>; 2] = [
    CallableParam::required("key"),
    CallableParam::required("value"),
];
const MAP_EXTEND_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("other")];
const SET_VALUE_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("value")];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuiltinFunction {
    Print,
    Range,
    Queue,
    Tasks,
    Cancelled,
    After,
    Sleep,
    Abs,
    Min,
    Max,
    Sqrt,
    ParseInt32,
    ParseInt64,
    ParseFloat64,
}

pub const ALL_BUILTIN_FUNCTIONS: &[BuiltinFunction] = &[
    BuiltinFunction::Print,
    BuiltinFunction::Range,
    BuiltinFunction::Queue,
    BuiltinFunction::Tasks,
    BuiltinFunction::Cancelled,
    BuiltinFunction::After,
    BuiltinFunction::Sleep,
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
            "queue" => Some(Self::Queue),
            "tasks" => Some(Self::Tasks),
            "cancelled" => Some(Self::Cancelled),
            "after" => Some(Self::After),
            "sleep" => Some(Self::Sleep),
            "abs" => Some(Self::Abs),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            "sqrt" => Some(Self::Sqrt),
            "parse_int32" => Some(Self::ParseInt32),
            "parse_int64" => Some(Self::ParseInt64),
            "parse_float64" => Some(Self::ParseFloat64),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Print => "print",
            Self::Range => "range",
            Self::Queue => "queue",
            Self::Tasks => "tasks",
            Self::Cancelled => "cancelled",
            Self::After => "after",
            Self::Sleep => "sleep",
            Self::Abs => "abs",
            Self::Min => "min",
            Self::Max => "max",
            Self::Sqrt => "sqrt",
            Self::ParseInt32 => "parse_int32",
            Self::ParseInt64 => "parse_int64",
            Self::ParseFloat64 => "parse_float64",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::Print => "print(value) -> None",
            Self::Range => "range(stop: int32) -> Range; range(start: int32, stop: int32) -> Range",
            Self::Queue => "queue() -> Queue[T]",
            Self::Tasks => "tasks() -> TaskGroup",
            Self::Cancelled => "cancelled() -> bool",
            Self::After => "after(duration: Duration) -> Duration",
            Self::Sleep => "sleep(duration: Duration) -> None",
            Self::Abs => "abs(value: number) -> number",
            Self::Min => "min(left: number, right: number) -> number",
            Self::Max => "max(left: number, right: number) -> number",
            Self::Sqrt => "sqrt(value: float32|float64) -> float32|float64",
            Self::ParseInt32 => "parse_int32(text: String) -> Result[int32, String]",
            Self::ParseInt64 => "parse_int64(text: String) -> Result[int64, String]",
            Self::ParseFloat64 => "parse_float64(text: String) -> Result[float64, String]",
        }
    }

    pub const fn docs(self) -> &'static str {
        match self {
            Self::Print => "Writes a value followed by a newline.",
            Self::Range => {
                "Builds an integer range from 0 up to, but not including, `stop`, or from `start` up to, but not including, `stop`."
            }
            Self::Queue => {
                "Creates a typed queue when the surrounding annotation or expectation provides `T`."
            }
            Self::Tasks => {
                "Creates a managed structured-concurrency task group for use with `with`."
            }
            Self::Cancelled => "Returns true when the current task has been cancelled.",
            Self::After => {
                "Builds a timeout/select timer expression from a duration literal or duration value."
            }
            Self::Sleep => "Blocks the current task for the requested duration.",
            Self::Abs => "Returns the absolute value of an integer or float.",
            Self::Min => "Returns the smaller of two numeric values of the same type.",
            Self::Max => "Returns the larger of two numeric values of the same type.",
            Self::Sqrt => "Returns the square root of a `float32` or `float64` value.",
            Self::ParseInt32 => "Parses a `String` into an `int32`, returning `Result.Err(String)` on failure.",
            Self::ParseInt64 => "Parses a `String` into an `int64`, returning `Result.Err(String)` on failure.",
            Self::ParseFloat64 => "Parses a `String` into a `float64`, returning `Result.Err(String)` on failure.",
        }
    }

    pub fn bind_args<'arg>(
        self,
        args: &'arg [Argument],
        span: Span,
    ) -> Result<Vec<Option<&'arg Argument>>> {
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
            Self::Queue => bind_call_arguments(
                &format!("`{}`", self.name()),
                &[],
                args,
                span,
                CallConvention::PositionalOnly,
            ),
            Self::Tasks => {
                bind_call_arguments("`tasks`", &[], args, span, CallConvention::PositionalOnly)
            }
            Self::Cancelled => bind_call_arguments(
                "`cancelled`",
                &[],
                args,
                span,
                CallConvention::PositionalOnly,
            ),
            Self::After => bind_call_arguments(
                "`after`",
                &AFTER_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::Sleep => bind_call_arguments(
                "`sleep`",
                &SLEEP_PARAMS,
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
pub enum BuiltinMember {
    FloatSqrt,
    StringLen,
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
    QueueGet,
    QueueClose,
    TaskResult,
    TaskGroupStart,
    TaskGroupCancel,
}

impl BuiltinMember {
    pub fn resolve(receiver_base: &str, name: &str) -> Option<Self> {
        match (receiver_base, name) {
            ("float64", "sqrt") => Some(Self::FloatSqrt),
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
            ("String", "clone") => Some(Self::StringClone),
            ("Queue", "put") => Some(Self::QueuePut),
            ("Queue", "get") => Some(Self::QueueGet),
            ("Queue", "close") => Some(Self::QueueClose),
            ("Task", "result") => Some(Self::TaskResult),
            ("TaskGroup", "start") => Some(Self::TaskGroupStart),
            ("TaskGroup", "cancel") => Some(Self::TaskGroupCancel),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::FloatSqrt => "sqrt",
            Self::ScalarToString => "to_string",
            Self::StringLen => "len",
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
            Self::QueueGet => "get",
            Self::QueueClose => "close",
            Self::TaskResult => "result",
            Self::TaskGroupStart => "start",
            Self::TaskGroupCancel => "cancel",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::FloatSqrt => "sqrt() -> float64",
            Self::ScalarToString => "to_string() -> String",
            Self::StringLen => "len() -> int32",
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
            Self::VecLen => "len() -> int32",
            Self::VecIsEmpty => "is_empty() -> bool",
            Self::VecClone => "clone() -> Vec[T]",
            Self::VecPush => "push(value) -> None",
            Self::VecPop => "pop() -> Option[T]",
            Self::VecGet => "get(index: int32) -> Option[T]",
            Self::VecSet => "set(index: int32, value: T) -> Option[T]",
            Self::VecRemove => "remove(index: int32) -> Option[T]",
            Self::VecSwap => "swap(first: int32, second: int32) -> bool",
            Self::VecContains => "contains(value: T) -> bool",
            Self::VecExtend => "extend(other: Vec[T]) -> None",
            Self::VecInsert => "insert(index: int32, value: T) -> bool",
            Self::VecClear => "clear() -> None",
            Self::VecReverse => "reverse() -> None",
            Self::MapLen => "len() -> int32",
            Self::MapIsEmpty => "is_empty() -> bool",
            Self::MapClone => "clone() -> Map[K, V]",
            Self::MapGet => "get(key: K) -> Option[V]",
            Self::MapSet => "set(key: K, value: V) -> Option[V]",
            Self::MapRemove => "remove(key: K) -> Option[V]",
            Self::MapContainsKey => "contains_key(key: K) -> bool",
            Self::MapKeys => "keys() -> Vec[K]",
            Self::MapValues => "values() -> Vec[V]",
            Self::MapItems => "items() -> Vec[MapEntry[K, V]]",
            Self::MapEntries => "entries() -> Vec[MapEntry[K, V]]",
            Self::MapClear => "clear() -> None",
            Self::MapExtend => "extend(other: Map[K, V]) -> None",
            Self::SetLen => "len() -> int32",
            Self::SetIsEmpty => "is_empty() -> bool",
            Self::SetClone => "clone() -> Set[T]",
            Self::SetContains => "contains(value: T) -> bool",
            Self::SetInsert => "insert(value: T) -> bool",
            Self::SetRemove => "remove(value: T) -> bool",
            Self::StringClone => "clone() -> String",
            Self::QueuePut => "put(value) -> Result[None, SendError[T]]",
            Self::QueueGet => "get(timeout: Duration = ...) -> Option[T]",
            Self::QueueClose => "close() -> None",
            Self::TaskResult => "result() -> T",
            Self::TaskGroupStart => "start(function, ...) -> Task[T]",
            Self::TaskGroupCancel => "cancel() -> None",
        }
    }

    pub const fn docs(self) -> &'static str {
        match self {
            Self::FloatSqrt => "Returns the square root of a `float64` value.",
            Self::ScalarToString => "Returns a `String` rendering of a numeric or `bool` value.",
            Self::StringLen => "Returns the number of bytes in the string.",
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
            Self::VecLen => "Returns the current number of elements in the vector.",
            Self::VecIsEmpty => "Returns true when the vector contains no elements.",
            Self::VecClone => "Creates a new owned `Vec[T]` with cloned element values.",
            Self::VecPush => "Appends a value to the end of the vector.",
            Self::VecPop => "Removes and returns the final element, or `Option.None` when empty.",
            Self::VecGet => {
                "Returns the element at `index`, or `Option.None` when the index is out of bounds."
            }
            Self::VecSet => {
                "Replaces the element at `index` and returns the previous element, or `Option.None` when the index is out of bounds."
            }
            Self::VecRemove => {
                "Removes the element at `index` and returns it, or `Option.None` when the index is out of bounds."
            }
            Self::VecSwap => {
                "Swaps the elements at `first` and `second`, returning `false` when either index is out of bounds."
            }
            Self::VecContains => "Returns true when the vector contains `value`.",
            Self::VecExtend => "Appends the elements of `other` to the end of the vector.",
            Self::VecInsert => {
                "Inserts `value` at `index`, returning `false` when the index is out of bounds."
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
                "Puts a value into the queue or returns `SendError.Closed(value)` if the queue is closed."
            }
            Self::QueueGet => {
                "Receives the next value from the queue, or `Option.None` when the queue is closed or the optional timeout expires."
            }
            Self::QueueClose => "Closes the queue and wakes blocked receivers.",
            Self::TaskResult => "Waits for the spawned task to finish and returns its value.",
            Self::TaskGroupStart => "Starts a child task in the current task group.",
            Self::TaskGroupCancel => {
                "Signals cancellation to child tasks in the current task group."
            }
        }
    }

    pub fn bind_args<'arg>(
        self,
        args: &'arg [Argument],
        span: Span,
    ) -> Result<Vec<Option<&'arg Argument>>> {
        match self {
            Self::FloatSqrt
            | Self::ScalarToString
            | Self::StringLen
            | Self::StringToLower
            | Self::StringToUpper
            | Self::StringTrim
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
            | Self::TaskResult
            | Self::QueueClose
            | Self::TaskGroupCancel => bind_call_arguments(
                &format!("`{}`", self.name()),
                &[],
                args,
                span,
                CallConvention::PositionalOnly,
            ),
            Self::QueueGet => bind_call_arguments(
                "`get`",
                &QUEUE_GET_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::VecGet | Self::VecRemove => bind_call_arguments(
                &format!("`{}`", self.name()),
                &VEC_INDEX_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::VecPush => bind_call_arguments(
                "`push`",
                &VEC_PUSH_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::VecSet => bind_call_arguments(
                "`set`",
                &VEC_SET_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::VecSwap => bind_call_arguments(
                "`swap`",
                &VEC_SWAP_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::VecContains => bind_call_arguments(
                "`contains`",
                &VEC_PUSH_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::VecExtend => bind_call_arguments(
                "`extend`",
                &VEC_EXTEND_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::VecInsert => bind_call_arguments(
                "`insert`",
                &VEC_INSERT_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::StringContains | Self::StringStartsWith | Self::StringEndsWith => {
                bind_call_arguments(
                    &format!("`{}`", self.name()),
                    &STRING_TEXT_PARAMS,
                    args,
                    span,
                    CallConvention::PositionalOrNamed,
                )
            }
            Self::StringSplit | Self::StringStripPrefix | Self::StringStripSuffix => {
                bind_call_arguments(
                    &format!("`{}`", self.name()),
                    &STRING_TEXT_PARAMS,
                    args,
                    span,
                    CallConvention::PositionalOrNamed,
                )
            }
            Self::StringReplace => bind_call_arguments(
                "`replace`",
                &STRING_REPLACE_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::StringJoin => bind_call_arguments(
                "`join`",
                &STRING_JOIN_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::MapGet | Self::MapRemove | Self::MapContainsKey => bind_call_arguments(
                &format!("`{}`", self.name()),
                &MAP_KEY_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::MapSet => bind_call_arguments(
                "`set`",
                &MAP_SET_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::MapExtend => bind_call_arguments(
                "`extend`",
                &MAP_EXTEND_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::SetContains | Self::SetInsert | Self::SetRemove => bind_call_arguments(
                &format!("`{}`", self.name()),
                &SET_VALUE_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::QueuePut => bind_call_arguments(
                &format!("`{}`", self.name()),
                &QUEUE_PUT_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
            Self::TaskGroupStart => bind_call_arguments(
                "`start`",
                &[CallableParam::required("function")],
                args,
                span,
                CallConvention::PositionalOnly,
            ),
        }
    }
}

#[cfg(test)]
#[path = "call_tests.rs"]
mod tests;
