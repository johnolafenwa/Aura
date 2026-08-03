use std::collections::HashMap;

use crate::ast::{Argument, Param, ReceiverKind};
use crate::diag::{Diagnostic, Result, Span};

pub(crate) const MIN_TASK_STACK_BYTES: i64 = 256 * 1024;
pub(crate) const MAX_TASK_STACK_BYTES: i64 = 64 * 1024 * 1024;

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

const PRINT_PARAMS: [BuiltinParam; 1] = [builtin_param!(required, "value", ReceiverKind::Borrow)];
const RANGE_STOP_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "stop", ReceiverKind::Borrow)];
const RANGE_START_STOP_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "start", ReceiverKind::Borrow),
    builtin_param!(required, "stop", ReceiverKind::Borrow),
];
const ABS_PARAMS: [BuiltinParam; 1] = [builtin_param!(required, "value", ReceiverKind::Borrow)];
const MIN_MAX_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "left", ReceiverKind::Borrow),
    builtin_param!(required, "right", ReceiverKind::Borrow),
];
const SQRT_PARAMS: [BuiltinParam; 1] = [builtin_param!(required, "value", ReceiverKind::Borrow)];
const PARSE_TEXT_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "text", ReceiverKind::Borrow)];
const RNG_SEED_PARAMS: [BuiltinParam; 1] = [builtin_param!(required, "seed", ReceiverKind::Borrow)];
const SLEEP_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "duration", ReceiverKind::Borrow)];
const TASK_LIST_TIMEOUT_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "tasks", ReceiverKind::Borrow),
    builtin_param!(optional, "timeout", ReceiverKind::Borrow),
];
const NO_BUILTIN_PARAMS: [BuiltinParam; 0] = [];
const DURATION_VALUE_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "value", ReceiverKind::Borrow)];
const STRING_FROM_BYTES_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "bytes", ReceiverKind::Borrow)];
const ARRAY_SHAPE_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "shape", ReceiverKind::Borrow)];
const ARRAY_FULL_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "shape", ReceiverKind::Borrow),
    builtin_param!(required, "value", ReceiverKind::Borrow),
];
const ARRAY_FROM_VEC_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "values", ReceiverKind::Borrow),
    builtin_param!(required, "shape", ReceiverKind::Borrow),
];
const ARRAY_INDEX_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "index", ReceiverKind::Borrow)];
const ARRAY_SET_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "index", ReceiverKind::Borrow),
    builtin_param!(required, "value", ReceiverKind::Borrow),
];
const ARRAY_VALUE_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "value", ReceiverKind::Borrow)];
const ARRAY_CALLBACK_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "f", ReceiverKind::Borrow)];
const ARITHMETIC_RHS_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "rhs", ReceiverKind::Borrow)];
const SHIFT_COUNT_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "count", ReceiverKind::Borrow)];
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
const VEC_POP_PARAMS: [BuiltinParam; 1] = [builtin_param!(optional, "index", ReceiverKind::Borrow)];
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
const VEC_SORT_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(optional, "key", ReceiverKind::Borrow),
    builtin_param!(optional, "reverse", ReceiverKind::Borrow),
];
const VEC_CALLBACK_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "f", ReceiverKind::Borrow)];
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
const CAPACITY_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "minimum", ReceiverKind::Borrow)];
const RESERVE_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "additional", ReceiverKind::Borrow)];
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
const TASK_GROUP_START_WITH_STACK_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "bytes", ReceiverKind::Borrow),
    builtin_param!(required, "function", ReceiverKind::Borrow),
];
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
    YieldNow,
    Sleep,
    Select,
    WaitAny,
    WaitAll,
    Abs,
    Min,
    Max,
    Sqrt,
    Round,
    Divmod,
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
    BuiltinFunction::YieldNow,
    BuiltinFunction::Sleep,
    BuiltinFunction::Select,
    BuiltinFunction::WaitAny,
    BuiltinFunction::WaitAll,
    BuiltinFunction::Len,
    BuiltinFunction::Str,
    BuiltinFunction::Abs,
    BuiltinFunction::Min,
    BuiltinFunction::Max,
    BuiltinFunction::Sqrt,
    BuiltinFunction::Round,
    BuiltinFunction::Divmod,
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
            "yield_now" => Some(Self::YieldNow),
            "sleep" => Some(Self::Sleep),
            "select" => Some(Self::Select),
            "wait_any" => Some(Self::WaitAny),
            "wait_all" => Some(Self::WaitAll),
            "abs" => Some(Self::Abs),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            "sqrt" => Some(Self::Sqrt),
            "round" => Some(Self::Round),
            "divmod" => Some(Self::Divmod),
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
            Self::YieldNow => "yield_now",
            Self::Sleep => "sleep",
            Self::Select => "select",
            Self::WaitAny => "wait_any",
            Self::WaitAll => "wait_all",
            Self::Abs => "abs",
            Self::Min => "min",
            Self::Max => "max",
            Self::Sqrt => "sqrt",
            Self::Round => "round",
            Self::Divmod => "divmod",
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
            Self::Range => "range(stop: int64) -> Range; range(start: int64, stop: int64) -> Range",
            Self::Cancelled => "cancelled() -> bool",
            Self::YieldNow => "yield_now() -> None",
            Self::Sleep => "sleep(duration: Duration) -> None",
            Self::Select => {
                "select(source, ...) -> SelectOutcome[Q, T] [Queue[Q], Task[T], or Duration sources]"
            }
            Self::WaitAny => {
                "wait_any(tasks: list[Task[T]], timeout: Duration = ...) -> WaitAny[T] [consumes tasks when T is non-repeatable]"
            }
            Self::WaitAll => {
                "wait_all(tasks: list[Task[T]], timeout: Duration = ...) -> WaitAll[T] [consumes tasks when T is non-repeatable]"
            }
            Self::Abs => "abs(value: number) -> number",
            Self::Min => "min(left: number, right: number) -> number",
            Self::Max => "max(left: number, right: number) -> number",
            Self::Sqrt => "sqrt(value: float32|float64) -> float32|float64",
            Self::Round => "round(value: integer|float32|float64) -> integer|int64",
            Self::Divmod => "divmod(left: number, right: number) -> (number, number)",
            Self::ParseInt32 => "parse_int32(text: str) -> Result[int32, str]",
            Self::ParseInt64 => "parse_int64(text: str) -> Result[int64, str]",
            Self::ParseFloat64 => "parse_float64(text: str) -> Result[float64, str]",
            Self::Len => "len(value: str|list[T]|dict[K, V]|set[T]) -> int64",
            Self::Str => "str(value) -> str",
        }
    }

    pub const fn docs(self) -> &'static str {
        match self {
            Self::Print => "Writes a value followed by a newline.",
            Self::Range => {
                "Builds an integer range from 0 up to, but not including, `stop`, or from `start` up to, but not including, `stop`."
            }
            Self::Cancelled => "Returns true when the current task has been cancelled.",
            Self::YieldNow => {
                "Keeps the current task runnable while yielding its scheduler turn to other ready tasks."
            }
            Self::Sleep => "Blocks the current task for the requested duration.",
            Self::Select => {
                "Waits atomically for one Queue receive, Task result, or relative Duration deadline. Queue payloads share one `Q`, task results share one `T`, and non-repeatable Task sources are consumed at call entry."
            }
            Self::WaitAny => {
                "Waits for the first task to complete and reports either the ready index/value pair, the failing task index/error message, a timeout, or cancellation. Observing non-repeatable `T` consumes the whole `list[Task[T]]` observation right; repeatable `T` leaves the list reusable."
            }
            Self::WaitAll => {
                "Waits for every task to complete and reports either the collected results, the failing task index/error message, a timeout, or cancellation. Observing non-repeatable `T` consumes the whole `list[Task[T]]` observation right; repeatable `T` leaves the list reusable."
            }
            Self::Abs => "Returns the absolute value of an integer or float.",
            Self::Min => "Returns the smaller of two numeric values of the same type.",
            Self::Max => "Returns the larger of two numeric values of the same type.",
            Self::Sqrt => "Returns the square root of a `float32` or `float64` value.",
            Self::Round => "Returns an integer unchanged, or rounds a float to the nearest `int64` using ties-to-even.",
            Self::Divmod => "Returns the paired floor quotient and divisor-signed remainder for two values of one exact numeric type.",
            Self::ParseInt32 => "Parses a `str` into an `int32`, returning `Result.Err(str)` on failure.",
            Self::ParseInt64 => "Parses a `str` into an `int64`, returning `Result.Err(str)` on failure.",
            Self::ParseFloat64 => "Parses a `str` into a `float64`, returning `Result.Err(str)` on failure.",
            Self::Len => "Returns the length of a value that has a `len()` member, delegating to that member.",
            Self::Str => "Renders a value the way `print` and f-string interpolation render it.",
        }
    }

    pub fn bind_args(self, args: &[Argument], span: Span) -> Result<Vec<Option<&Argument>>> {
        match self {
            Self::Select => {
                if args.is_empty() {
                    return Err(Diagnostic::coded_at(
                        "AU2004",
                        span,
                        "`select` expects at least one positional source",
                    ));
                }
                if let Some(argument) = args.iter().find(|argument| argument.name.is_some()) {
                    return Err(Diagnostic::coded_at(
                        "AU2004",
                        argument.span,
                        "`select` does not take keyword arguments",
                    ));
                }
                self.call_shape().bind_args("`select`", args, span)
            }
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
                    BuiltinCallShape::fixed(
                        &RANGE_START_STOP_PARAMS,
                        CallConvention::PositionalOrNamed,
                    )
                    .bind_args("`range`", args, span)
                } else {
                    BuiltinCallShape::fixed(&RANGE_STOP_PARAMS, CallConvention::PositionalOrNamed)
                        .bind_args("`range`", args, span)
                }
            }
            _ => self
                .call_shape()
                .bind_args(&format!("`{}`", self.name()), args, span),
        }
    }

    pub const fn argument_passing(self, index: usize) -> Option<ReceiverKind> {
        let shape = self.call_shape();
        if index < shape.params.len() {
            Some(shape.params[index].passing)
        } else if matches!(self, Self::Select) {
            shape.variadic_passing
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
            Self::Print | Self::Len | Self::Str => {
                BuiltinCallShape::fixed(&PRINT_PARAMS, CallConvention::PositionalOrNamed)
            }
            // Argument binding chooses the one- or two-argument shape, while
            // capability metadata is identical for both signatures.
            Self::Range => {
                BuiltinCallShape::fixed(&RANGE_START_STOP_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::Cancelled | Self::YieldNow => {
                BuiltinCallShape::fixed(&NO_BUILTIN_PARAMS, CallConvention::PositionalOnly)
            }
            Self::Sleep => {
                BuiltinCallShape::fixed(&SLEEP_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::Select => BuiltinCallShape::variadic(&NO_BUILTIN_PARAMS, ReceiverKind::Borrow),
            Self::WaitAny | Self::WaitAll => BuiltinCallShape::fixed(
                &TASK_LIST_TIMEOUT_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
            Self::Abs | Self::Round => {
                BuiltinCallShape::fixed(&ABS_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::Min | Self::Max | Self::Divmod => {
                BuiltinCallShape::fixed(&MIN_MAX_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::Sqrt => BuiltinCallShape::fixed(&SQRT_PARAMS, CallConvention::PositionalOrNamed),
            Self::ParseInt32 | Self::ParseInt64 | Self::ParseFloat64 => {
                BuiltinCallShape::fixed(&PARSE_TEXT_PARAMS, CallConvention::PositionalOrNamed)
            }
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuiltinAssociatedFunction {
    DurationMilliseconds,
    DurationSeconds,
    DurationMinutes,
    StringFromBytes,
    ArrayZeros,
    ArrayFull,
    ArrayFromVec,
    ListWithCapacity,
    DictWithCapacity,
    SetWithCapacity,
}

pub const ALL_BUILTIN_ASSOCIATED_FUNCTIONS: &[BuiltinAssociatedFunction] = &[
    BuiltinAssociatedFunction::DurationMilliseconds,
    BuiltinAssociatedFunction::DurationSeconds,
    BuiltinAssociatedFunction::DurationMinutes,
    BuiltinAssociatedFunction::StringFromBytes,
    BuiltinAssociatedFunction::ArrayZeros,
    BuiltinAssociatedFunction::ArrayFull,
    BuiltinAssociatedFunction::ArrayFromVec,
    BuiltinAssociatedFunction::ListWithCapacity,
    BuiltinAssociatedFunction::DictWithCapacity,
    BuiltinAssociatedFunction::SetWithCapacity,
];

impl BuiltinAssociatedFunction {
    pub fn resolve(type_name: &str, name: &str) -> Option<Self> {
        match (type_name, name) {
            ("Duration", "ms") => Some(Self::DurationMilliseconds),
            ("Duration", "seconds") => Some(Self::DurationSeconds),
            ("Duration", "minutes") => Some(Self::DurationMinutes),
            ("str", "from_bytes") => Some(Self::StringFromBytes),
            ("Array", "zeros") => Some(Self::ArrayZeros),
            ("Array", "full") => Some(Self::ArrayFull),
            ("Array", "from_list") => Some(Self::ArrayFromVec),
            ("list", "with_capacity") => Some(Self::ListWithCapacity),
            ("dict", "with_capacity") => Some(Self::DictWithCapacity),
            ("set", "with_capacity") => Some(Self::SetWithCapacity),
            _ => None,
        }
    }

    pub const fn owner_name(self) -> &'static str {
        match self {
            Self::DurationMilliseconds | Self::DurationSeconds | Self::DurationMinutes => {
                "Duration"
            }
            Self::StringFromBytes => "str",
            Self::ArrayZeros | Self::ArrayFull | Self::ArrayFromVec => "Array",
            Self::ListWithCapacity => "list",
            Self::DictWithCapacity => "dict",
            Self::SetWithCapacity => "set",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::DurationMilliseconds => "ms",
            Self::DurationSeconds => "seconds",
            Self::DurationMinutes => "minutes",
            Self::StringFromBytes => "from_bytes",
            Self::ArrayZeros => "zeros",
            Self::ArrayFull => "full",
            Self::ArrayFromVec => "from_list",
            Self::ListWithCapacity | Self::DictWithCapacity | Self::SetWithCapacity => {
                "with_capacity"
            }
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::DurationMilliseconds => "ms(value: int64) -> Duration",
            Self::DurationSeconds => "seconds(value: int64) -> Duration",
            Self::DurationMinutes => "minutes(value: int64) -> Duration",
            Self::StringFromBytes => "from_bytes(bytes: list[uint8]) -> Result[str, bytes.Error]",
            Self::ArrayZeros => "zeros(shape: list[int64]) -> Array[T]",
            Self::ArrayFull => "full(shape: list[int64], value: T) -> Array[T]",
            Self::ArrayFromVec => "from_list(values: list[T], shape: list[int64]) -> Array[T]",
            Self::ListWithCapacity => "with_capacity(minimum: int64) -> list[T]",
            Self::DictWithCapacity => "with_capacity(minimum: int64) -> dict[K, V]",
            Self::SetWithCapacity => "with_capacity(minimum: int64) -> set[T]",
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
            Self::ArrayZeros => {
                "Constructs a numeric Array filled with zeros using the requested shape."
            }
            Self::ArrayFull => {
                "Constructs a numeric Array filled with one value using the requested shape."
            }
            Self::ArrayFromVec => {
                "Copies numeric list values into an Array with the requested shape."
            }
            Self::ListWithCapacity | Self::DictWithCapacity | Self::SetWithCapacity => {
                "Creates an empty collection with at least the requested capacity."
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
            Self::ArrayZeros => {
                BuiltinCallShape::fixed(&ARRAY_SHAPE_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::ArrayFull => {
                BuiltinCallShape::fixed(&ARRAY_FULL_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::ArrayFromVec => {
                BuiltinCallShape::fixed(&ARRAY_FROM_VEC_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::ListWithCapacity | Self::DictWithCapacity | Self::SetWithCapacity => {
                BuiltinCallShape::fixed(&CAPACITY_PARAMS, CallConvention::PositionalOrNamed)
            }
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
        self.call_shape()
            .bind_args(&format!("`{}`", self.qualified_name()), args, span)
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
            Self::RandomRng => {
                BuiltinCallShape::fixed(&RNG_SEED_PARAMS, CallConvention::PositionalOrNamed)
            }
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuiltinMember {
    FloatSqrt,
    IntegerToFloat,
    IntegerWrappingAdd,
    IntegerWrappingSub,
    IntegerWrappingMul,
    IntegerSaturatingAdd,
    IntegerSaturatingSub,
    IntegerSaturatingMul,
    IntegerWrappingShl,
    IntegerWrappingShr,
    IntegerSaturatingShl,
    IntegerSaturatingShr,
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
    ArrayShape,
    ArrayLen,
    ArrayClone,
    ArrayGet,
    ArraySet,
    ArrayFill,
    ArrayMap,
    ArraySum,
    ArrayMin,
    ArrayMax,
    ArrayMean,
    ArrayWrappingAdd,
    ArrayWrappingSub,
    ArrayWrappingMul,
    ArraySaturatingAdd,
    ArraySaturatingSub,
    ArraySaturatingMul,
    VecLen,
    VecIsEmpty,
    VecClone,
    VecPush,
    VecPop,
    VecGet,
    VecSet,
    VecRemove,
    VecIndex,
    VecCount,
    VecSwap,
    VecContains,
    VecExtend,
    VecInsert,
    VecClear,
    VecReverse,
    VecSort,
    VecMap,
    VecFilter,
    VecReserve,
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
    MapClear,
    MapExtend,
    MapReserve,
    SetLen,
    SetIsEmpty,
    SetClone,
    SetContains,
    SetInsert,
    SetRemove,
    SetDiscard,
    SetClear,
    SetReserve,
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
    TaskGroupStartWithStack,
    TaskGroupStartSoonWithStack,
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

fn is_builtin_integer_name(name: &str) -> bool {
    matches!(
        name,
        "int8"
            | "int16"
            | "int32"
            | "int64"
            | "int128"
            | "intsize"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uint128"
            | "uintsize"
    )
}

impl BuiltinMember {
    /// Resolves private operations emitted by checked MIR as well as public
    /// source methods. Private operations never participate in source member
    /// lookup or editor completion.
    pub fn resolve_runtime(receiver_base: &str, name: &str) -> Option<Self> {
        match (receiver_base, name) {
            // These spellings are private MIR operations. Public source lookup
            // is handled by `resolve` below.
            ("dict", "contains_key") => Some(Self::MapContainsKey),
            ("dict", "set") => Some(Self::MapSet),
            ("set", "contains") => Some(Self::SetContains),
            _ => Self::resolve(receiver_base, name),
        }
    }

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
            ("int8", "wrapping_add")
            | ("int16", "wrapping_add")
            | ("int32", "wrapping_add")
            | ("int64", "wrapping_add")
            | ("int128", "wrapping_add")
            | ("intsize", "wrapping_add")
            | ("uint8", "wrapping_add")
            | ("uint16", "wrapping_add")
            | ("uint32", "wrapping_add")
            | ("uint64", "wrapping_add")
            | ("uint128", "wrapping_add")
            | ("uintsize", "wrapping_add") => Some(Self::IntegerWrappingAdd),
            ("int8", "wrapping_sub")
            | ("int16", "wrapping_sub")
            | ("int32", "wrapping_sub")
            | ("int64", "wrapping_sub")
            | ("int128", "wrapping_sub")
            | ("intsize", "wrapping_sub")
            | ("uint8", "wrapping_sub")
            | ("uint16", "wrapping_sub")
            | ("uint32", "wrapping_sub")
            | ("uint64", "wrapping_sub")
            | ("uint128", "wrapping_sub")
            | ("uintsize", "wrapping_sub") => Some(Self::IntegerWrappingSub),
            ("int8", "wrapping_mul")
            | ("int16", "wrapping_mul")
            | ("int32", "wrapping_mul")
            | ("int64", "wrapping_mul")
            | ("int128", "wrapping_mul")
            | ("intsize", "wrapping_mul")
            | ("uint8", "wrapping_mul")
            | ("uint16", "wrapping_mul")
            | ("uint32", "wrapping_mul")
            | ("uint64", "wrapping_mul")
            | ("uint128", "wrapping_mul")
            | ("uintsize", "wrapping_mul") => Some(Self::IntegerWrappingMul),
            ("int8", "saturating_add")
            | ("int16", "saturating_add")
            | ("int32", "saturating_add")
            | ("int64", "saturating_add")
            | ("int128", "saturating_add")
            | ("intsize", "saturating_add")
            | ("uint8", "saturating_add")
            | ("uint16", "saturating_add")
            | ("uint32", "saturating_add")
            | ("uint64", "saturating_add")
            | ("uint128", "saturating_add")
            | ("uintsize", "saturating_add") => Some(Self::IntegerSaturatingAdd),
            ("int8", "saturating_sub")
            | ("int16", "saturating_sub")
            | ("int32", "saturating_sub")
            | ("int64", "saturating_sub")
            | ("int128", "saturating_sub")
            | ("intsize", "saturating_sub")
            | ("uint8", "saturating_sub")
            | ("uint16", "saturating_sub")
            | ("uint32", "saturating_sub")
            | ("uint64", "saturating_sub")
            | ("uint128", "saturating_sub")
            | ("uintsize", "saturating_sub") => Some(Self::IntegerSaturatingSub),
            ("int8", "saturating_mul")
            | ("int16", "saturating_mul")
            | ("int32", "saturating_mul")
            | ("int64", "saturating_mul")
            | ("int128", "saturating_mul")
            | ("intsize", "saturating_mul")
            | ("uint8", "saturating_mul")
            | ("uint16", "saturating_mul")
            | ("uint32", "saturating_mul")
            | ("uint64", "saturating_mul")
            | ("uint128", "saturating_mul")
            | ("uintsize", "saturating_mul") => Some(Self::IntegerSaturatingMul),
            (receiver, "wrapping_shl") if is_builtin_integer_name(receiver) => {
                Some(Self::IntegerWrappingShl)
            }
            (receiver, "wrapping_shr") if is_builtin_integer_name(receiver) => {
                Some(Self::IntegerWrappingShr)
            }
            (receiver, "saturating_shl") if is_builtin_integer_name(receiver) => {
                Some(Self::IntegerSaturatingShl)
            }
            (receiver, "saturating_shr") if is_builtin_integer_name(receiver) => {
                Some(Self::IntegerSaturatingShr)
            }
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
            ("Array", "shape") => Some(Self::ArrayShape),
            ("Array", "len") => Some(Self::ArrayLen),
            ("Array", "clone") => Some(Self::ArrayClone),
            ("Array", "get") => Some(Self::ArrayGet),
            ("Array", "set") => Some(Self::ArraySet),
            ("Array", "fill") => Some(Self::ArrayFill),
            ("Array", "map") => Some(Self::ArrayMap),
            ("Array", "sum") => Some(Self::ArraySum),
            ("Array", "min") => Some(Self::ArrayMin),
            ("Array", "max") => Some(Self::ArrayMax),
            ("Array", "mean") => Some(Self::ArrayMean),
            ("Array", "wrapping_add") => Some(Self::ArrayWrappingAdd),
            ("Array", "wrapping_sub") => Some(Self::ArrayWrappingSub),
            ("Array", "wrapping_mul") => Some(Self::ArrayWrappingMul),
            ("Array", "saturating_add") => Some(Self::ArraySaturatingAdd),
            ("Array", "saturating_sub") => Some(Self::ArraySaturatingSub),
            ("Array", "saturating_mul") => Some(Self::ArraySaturatingMul),
            ("list", "len") => Some(Self::VecLen),
            ("list", "is_empty") => Some(Self::VecIsEmpty),
            ("list", "copy") => Some(Self::VecClone),
            ("list", "append") => Some(Self::VecPush),
            ("list", "pop") => Some(Self::VecPop),
            ("list", "get") => Some(Self::VecGet),
            ("list", "set") => Some(Self::VecSet),
            ("list", "remove") => Some(Self::VecRemove),
            ("list", "index") => Some(Self::VecIndex),
            ("list", "count") => Some(Self::VecCount),
            ("list", "swap") => Some(Self::VecSwap),
            ("list", "contains") => Some(Self::VecContains),
            ("list", "extend") => Some(Self::VecExtend),
            ("list", "insert") => Some(Self::VecInsert),
            ("list", "clear") => Some(Self::VecClear),
            ("list", "reverse") => Some(Self::VecReverse),
            ("list", "sort") => Some(Self::VecSort),
            ("list", "map") => Some(Self::VecMap),
            ("list", "filter") => Some(Self::VecFilter),
            ("list", "reserve") => Some(Self::VecReserve),
            ("dict", "len") => Some(Self::MapLen),
            ("dict", "is_empty") => Some(Self::MapIsEmpty),
            ("dict", "copy") => Some(Self::MapClone),
            ("dict", "get") => Some(Self::MapGet),
            ("dict", "remove") => Some(Self::MapRemove),
            ("dict", "keys") => Some(Self::MapKeys),
            ("dict", "values") => Some(Self::MapValues),
            ("dict", "items") => Some(Self::MapItems),
            ("dict", "clear") => Some(Self::MapClear),
            ("dict", "update") => Some(Self::MapExtend),
            ("dict", "reserve") => Some(Self::MapReserve),
            ("set", "len") => Some(Self::SetLen),
            ("set", "is_empty") => Some(Self::SetIsEmpty),
            ("set", "copy") => Some(Self::SetClone),
            ("set", "add") => Some(Self::SetInsert),
            ("set", "remove") => Some(Self::SetRemove),
            ("set", "discard") => Some(Self::SetDiscard),
            ("set", "clear") => Some(Self::SetClear),
            ("set", "reserve") => Some(Self::SetReserve),
            ("str", "len") => Some(Self::StringLen),
            ("str", "byte_len") => Some(Self::StringByteLen),
            ("str", "contains") => Some(Self::StringContains),
            ("str", "starts_with") => Some(Self::StringStartsWith),
            ("str", "ends_with") => Some(Self::StringEndsWith),
            ("str", "split") => Some(Self::StringSplit),
            ("str", "replace") => Some(Self::StringReplace),
            ("str", "to_lower") => Some(Self::StringToLower),
            ("str", "to_upper") => Some(Self::StringToUpper),
            ("str", "strip_prefix") => Some(Self::StringStripPrefix),
            ("str", "strip_suffix") => Some(Self::StringStripSuffix),
            ("str", "trim") => Some(Self::StringTrim),
            ("str", "join") => Some(Self::StringJoin),
            ("str", "to_bytes") => Some(Self::StringToBytes),
            ("str", "clone") => Some(Self::StringClone),
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
            ("TaskGroup", "start_with_stack") => Some(Self::TaskGroupStartWithStack),
            ("TaskGroup", "start_soon_with_stack") => Some(Self::TaskGroupStartSoonWithStack),
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
            Self::IntegerWrappingAdd | Self::ArrayWrappingAdd => "wrapping_add",
            Self::IntegerWrappingSub | Self::ArrayWrappingSub => "wrapping_sub",
            Self::IntegerWrappingMul | Self::ArrayWrappingMul => "wrapping_mul",
            Self::IntegerSaturatingAdd | Self::ArraySaturatingAdd => "saturating_add",
            Self::IntegerSaturatingSub | Self::ArraySaturatingSub => "saturating_sub",
            Self::IntegerSaturatingMul | Self::ArraySaturatingMul => "saturating_mul",
            Self::IntegerWrappingShl => "wrapping_shl",
            Self::IntegerWrappingShr => "wrapping_shr",
            Self::IntegerSaturatingShl => "saturating_shl",
            Self::IntegerSaturatingShr => "saturating_shr",
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
            Self::ArrayShape => "shape",
            Self::ArrayLen => "len",
            Self::ArrayClone => "clone",
            Self::ArrayGet => "get",
            Self::ArraySet => "set",
            Self::ArrayFill => "fill",
            Self::ArrayMap => "map",
            Self::ArraySum => "sum",
            Self::ArrayMin => "min",
            Self::ArrayMax => "max",
            Self::ArrayMean => "mean",
            Self::VecLen => "len",
            Self::VecIsEmpty => "is_empty",
            Self::VecClone | Self::MapClone => "copy",
            Self::StringClone => "clone",
            Self::VecPush => "append",
            Self::VecPop => "pop",
            Self::VecGet => "get",
            Self::VecSet => "set",
            Self::VecRemove => "remove",
            Self::VecIndex => "index",
            Self::VecCount => "count",
            Self::VecSwap => "swap",
            Self::VecContains => "contains",
            Self::VecExtend => "extend",
            Self::VecInsert => "insert",
            Self::VecClear => "clear",
            Self::VecReverse => "reverse",
            Self::VecSort => "sort",
            Self::VecMap => "map",
            Self::VecFilter => "filter",
            Self::VecReserve => "reserve",
            Self::MapLen => "len",
            Self::MapIsEmpty => "is_empty",
            Self::MapGet => "get",
            Self::MapSet => "set",
            Self::MapRemove => "remove",
            Self::MapContainsKey => "contains",
            Self::MapKeys => "keys",
            Self::MapValues => "values",
            Self::MapItems => "items",
            Self::MapClear => "clear",
            Self::MapExtend => "update",
            Self::MapReserve => "reserve",
            Self::SetLen => "len",
            Self::SetIsEmpty => "is_empty",
            Self::SetClone => "copy",
            Self::SetContains => "contains",
            Self::SetInsert => "add",
            Self::SetRemove => "remove",
            Self::SetDiscard => "discard",
            Self::SetClear => "clear",
            Self::SetReserve => "reserve",
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
            Self::TaskGroupStartWithStack => "start_with_stack",
            Self::TaskGroupStartSoonWithStack => "start_soon_with_stack",
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
            Self::IntegerWrappingAdd => "wrapping_add(rhs: Self) -> Self",
            Self::IntegerWrappingSub => "wrapping_sub(rhs: Self) -> Self",
            Self::IntegerWrappingMul => "wrapping_mul(rhs: Self) -> Self",
            Self::IntegerSaturatingAdd => "saturating_add(rhs: Self) -> Self",
            Self::IntegerSaturatingSub => "saturating_sub(rhs: Self) -> Self",
            Self::IntegerSaturatingMul => "saturating_mul(rhs: Self) -> Self",
            Self::IntegerWrappingShl => "wrapping_shl(count: Self) -> Self",
            Self::IntegerWrappingShr => "wrapping_shr(count: Self) -> Self",
            Self::IntegerSaturatingShl => "saturating_shl(count: Self) -> Self",
            Self::IntegerSaturatingShr => "saturating_shr(count: Self) -> Self",
            Self::DurationToMilliseconds => "to_ms() -> float64",
            Self::DurationToSeconds => "to_seconds() -> float64",
            Self::ScalarToString => "to_string() -> str",
            Self::StringLen => "len() -> int64",
            Self::StringByteLen => "byte_len() -> int64",
            Self::StringContains => "contains(text: str) -> bool",
            Self::StringStartsWith => "starts_with(text: str) -> bool",
            Self::StringEndsWith => "ends_with(text: str) -> bool",
            Self::StringSplit => "split(text: str) -> list[str]",
            Self::StringReplace => "replace(from: str, to: str) -> str",
            Self::StringToLower => "to_lower() -> str",
            Self::StringToUpper => "to_upper() -> str",
            Self::StringStripPrefix => "strip_prefix(text: str) -> Option[str]",
            Self::StringStripSuffix => "strip_suffix(text: str) -> Option[str]",
            Self::StringTrim => "trim() -> str",
            Self::StringJoin => "join(parts: list[str]) -> str",
            Self::StringToBytes => "to_bytes() -> list[uint8]",
            Self::ArrayShape => "shape() -> list[int64]",
            Self::ArrayLen => "len() -> int64",
            Self::ArrayClone => "clone() -> Array[T]",
            Self::ArrayGet => "get(index: list[int64]) -> Option[T]",
            Self::ArraySet => "set(index: list[int64], value: T) -> Option[T]",
            Self::ArrayFill => "fill(value: T) -> None",
            Self::ArrayMap => "map[U](f: def(T) -> U) -> Array[U]",
            Self::ArraySum => "sum() -> T",
            Self::ArrayMin => "min() -> T",
            Self::ArrayMax => "max() -> T",
            Self::ArrayMean => "mean() -> float64",
            Self::ArrayWrappingAdd => "wrapping_add(rhs: Array[T] | T) -> Array[T]",
            Self::ArrayWrappingSub => "wrapping_sub(rhs: Array[T] | T) -> Array[T]",
            Self::ArrayWrappingMul => "wrapping_mul(rhs: Array[T] | T) -> Array[T]",
            Self::ArraySaturatingAdd => "saturating_add(rhs: Array[T] | T) -> Array[T]",
            Self::ArraySaturatingSub => "saturating_sub(rhs: Array[T] | T) -> Array[T]",
            Self::ArraySaturatingMul => "saturating_mul(rhs: Array[T] | T) -> Array[T]",
            Self::VecLen => "len() -> int64",
            Self::VecIsEmpty => "is_empty() -> bool",
            Self::VecClone => "copy() -> list[T]",
            Self::VecPush => "append(value: own T) -> None",
            Self::VecPop => "pop(index: int64 = -1) -> T",
            Self::VecGet => "get(index: int64) -> Option[T]",
            Self::VecSet => "set(index: int64, value: own T) -> T",
            Self::VecRemove => "remove(value: T) -> None",
            Self::VecIndex => "index(value: T) -> int64",
            Self::VecCount => "count(value: T) -> int64",
            Self::VecSwap => "swap(first: int64, second: int64) -> None",
            Self::VecContains => "contains(value: T) -> bool",
            Self::VecExtend => "extend(other: own list[T]) -> None",
            Self::VecInsert => "insert(index: int64, value: own T) -> None",
            Self::VecClear => "clear() -> None",
            Self::VecReverse => "reverse() -> None",
            Self::VecSort => "sort(key: def(T) -> K = ..., reverse: bool = false) -> None",
            Self::VecMap => "map(f: def(T) -> U) -> list[U]",
            Self::VecFilter => "filter(f: def(T) -> bool) -> list[T]",
            Self::VecReserve => "reserve(additional: int64) -> None",
            Self::MapLen => "len() -> int64",
            Self::MapIsEmpty => "is_empty() -> bool",
            Self::MapClone => "copy() -> dict[K, V]",
            Self::MapGet => "get(key: K) -> Option[V]",
            Self::MapSet => "set(key: own K, value: own V) -> Option[V]",
            Self::MapRemove => "remove(key: K) -> Option[V]",
            Self::MapContainsKey => "contains(key: K) -> bool",
            Self::MapKeys => "keys() -> list[K]",
            Self::MapValues => "values() -> list[V]",
            Self::MapItems => "items() -> list[(K, V)]",
            Self::MapClear => "clear() -> None",
            Self::MapExtend => "update(other: own dict[K, V]) -> None",
            Self::MapReserve => "reserve(additional: int64) -> None",
            Self::SetLen => "len() -> int64",
            Self::SetIsEmpty => "is_empty() -> bool",
            Self::SetClone => "copy() -> set[T]",
            Self::SetContains => "contains(value: T) -> bool",
            Self::SetInsert => "add(value: own T) -> None",
            Self::SetRemove => "remove(value: T) -> None",
            Self::SetDiscard => "discard(value: T) -> None",
            Self::SetClear => "clear() -> None",
            Self::SetReserve => "reserve(additional: int64) -> None",
            Self::StringClone => "clone() -> str",
            Self::QueuePut => {
                "put(value: own T, timeout: Duration = ...) -> Result[None, SendError[T]] [T must be Transfer]"
            }
            Self::QueueTryPut => {
                "try_put(value: own T) -> Result[None, SendError[T]] [T must be Transfer]"
            }
            Self::QueueGet => {
                "get(timeout: Duration = ...) -> QueueReceive[T] [T must be Transfer]"
            }
            Self::QueueGetOrNone => {
                "get_or_none(timeout: Duration = ...) -> Option[T] [T must be Transfer]"
            }
            Self::QueueGetOr => {
                "get_or(default: own T, timeout: Duration = ...) -> T [T must be Transfer]"
            }
            Self::QueueClose => "close() -> None",
            Self::TaskResult => {
                "result(timeout: Duration = ...) -> TaskResult[T] [consumes Task[T] when T is non-repeatable]"
            }
            Self::TaskResultOrNone => {
                "result_or_none(timeout: Duration = ...) -> Option[T] [consumes Task[T] when T is non-repeatable]"
            }
            Self::TaskResultOr => {
                "result_or(default: own T, timeout: Duration = ...) -> T [consumes Task[T] when T is non-repeatable]"
            }
            Self::TaskGroupStart => "start(function, own ...) -> Task[T]",
            Self::TaskGroupStartSoon => "start_soon(function, own ...) -> None",
            Self::TaskGroupStartWithStack => {
                "start_with_stack(bytes: int64, function, own ...) -> Task[T]"
            }
            Self::TaskGroupStartSoonWithStack => {
                "start_soon_with_stack(bytes: int64, function, own ...) -> None"
            }
            Self::TaskGroupCancel => "cancel() -> None",
            Self::FileReadAll => "read_all() -> Result[str, io.Error]",
            Self::FileReadBytes => "read_bytes() -> Result[list[uint8], io.Error]",
            Self::FileWriteAll => "write_all(text: str) -> Result[None, io.Error]",
            Self::FileWriteBytes => "write_bytes(bytes: list[uint8]) -> Result[None, io.Error]",
            Self::FileFlush => "flush() -> Result[None, io.Error]",
            Self::FileClose => "close() -> None",
            Self::TcpListenerAccept => "accept(timeout: Duration = ...) -> Result[net.TcpStream, io.Error]",
            Self::TcpListenerLocalAddr => "local_addr() -> Result[str, io.Error]",
            Self::TcpListenerClose => "close() -> None",
            Self::TcpStreamReadAll => "read_all(timeout: Duration = ...) -> Result[str, io.Error]",
            Self::TcpStreamReadLine => "read_line(timeout: Duration = ...) -> Result[Option[str], io.Error]",
            Self::TcpStreamReadBytes => "read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[Option[list[uint8]], io.Error]",
            Self::TcpStreamReadExact => "read_exact(count: int32, timeout: Duration = ...) -> Result[list[uint8], io.Error]",
            Self::TcpStreamWriteAll => "write_all(text: str, timeout: Duration = ...) -> Result[None, io.Error]",
            Self::TcpStreamWriteBytes => "write_bytes(bytes: list[uint8], timeout: Duration = ...) -> Result[None, io.Error]",
            Self::TcpStreamFlush => "flush() -> Result[None, io.Error]",
            Self::TcpStreamLocalAddr => "local_addr() -> Result[str, io.Error]",
            Self::TcpStreamPeerAddr => "peer_addr() -> Result[str, io.Error]",
            Self::TcpStreamShutdownRead => "shutdown_read() -> Result[None, io.Error]",
            Self::TcpStreamShutdownWrite => "shutdown_write() -> Result[None, io.Error]",
            Self::TcpStreamShutdownBoth => "shutdown_both() -> Result[None, io.Error]",
            Self::TcpStreamClose => "close() -> None",
            Self::UdpSocketSendText => "send_text(address: str, text: str, timeout: Duration = ...) -> Result[None, io.Error]",
            Self::UdpSocketSendBytes => "send_bytes(address: str, bytes: list[uint8], timeout: Duration = ...) -> Result[None, io.Error]",
            Self::UdpSocketRecv => "recv(max_bytes: int32, timeout: Duration = ...) -> Result[Option[list[uint8]], io.Error]",
            Self::UdpSocketRecvFrom => "recv_from(max_bytes: int32, timeout: Duration = ...) -> Result[Option[net.UdpDatagram], io.Error]",
            Self::UdpSocketLocalAddr => "local_addr() -> Result[str, io.Error]",
            Self::UdpSocketPeerAddr => "peer_addr() -> Result[str, io.Error]",
            Self::UdpSocketClose => "close() -> None",
            Self::UdpDatagramAddress => "address() -> str",
            Self::UdpDatagramBytes => "bytes() -> list[uint8]",
            Self::UdpDatagramText => "text() -> Result[str, io.Error]",
            Self::HttpListenerAccept => "accept(timeout: Duration = ...) -> Result[net.HttpExchange, io.Error]",
            Self::HttpListenerLocalAddr => "local_addr() -> Result[str, io.Error]",
            Self::HttpListenerClose => "close() -> None",
            Self::HttpExchangeMethod => "method() -> str",
            Self::HttpExchangePath => "path() -> str",
            Self::HttpExchangeHeaders => "headers() -> dict[str, str]",
            Self::HttpExchangeBodyText => "body_text() -> Result[str, io.Error]",
            Self::HttpExchangeBodyBytes => "body_bytes() -> list[uint8]",
            Self::HttpExchangeRespondText => "respond_text(status: int32, text: own str, headers: own dict[str, str]) -> Result[None, io.Error]",
            Self::HttpExchangeRespondBytes => "respond_bytes(status: int32, bytes: own list[uint8], headers: own dict[str, str]) -> Result[None, io.Error]",
            Self::HttpResponseStatus => "status() -> int32",
            Self::HttpResponseReason => "reason() -> str",
            Self::HttpResponseHeaders => "headers() -> dict[str, str]",
            Self::HttpResponseText => "text() -> Result[str, io.Error]",
            Self::HttpResponseBytes => "bytes() -> list[uint8]",
            Self::WebSocketListenerAccept => "accept(timeout: Duration = ...) -> Result[net.WebSocket, io.Error]",
            Self::WebSocketListenerLocalAddr => "local_addr() -> Result[str, io.Error]",
            Self::WebSocketSendText => "send_text(text: str, timeout: Duration = ...) -> Result[None, io.Error]",
            Self::WebSocketSendBytes => "send_bytes(bytes: list[uint8], timeout: Duration = ...) -> Result[None, io.Error]",
            Self::WebSocketRecvText => "recv_text(timeout: Duration = ...) -> Result[Option[str], io.Error]",
            Self::WebSocketRecvBytes => "recv_bytes(timeout: Duration = ...) -> Result[Option[list[uint8]], io.Error]",
            Self::WebSocketClose => "close() -> None",
            Self::UnixListenerAccept => "accept(timeout: Duration = ...) -> Result[net.UnixStream, io.Error]",
            Self::UnixListenerClose => "close() -> None",
            Self::UnixStreamReadLine => "read_line(timeout: Duration = ...) -> Result[Option[str], io.Error]",
            Self::UnixStreamReadExact => "read_exact(count: int32, timeout: Duration = ...) -> Result[list[uint8], io.Error]",
            Self::UnixStreamWriteAll => "write_all(text: str, timeout: Duration = ...) -> Result[None, io.Error]",
            Self::UnixStreamClose => "close() -> None",
            Self::TlsListenerAccept => "accept(timeout: Duration = ...) -> Result[net.TlsStream, io.Error]",
            Self::TlsListenerLocalAddr => "local_addr() -> Result[str, io.Error]",
            Self::TlsListenerClose => "close() -> None",
            Self::TlsStreamReadLine => "read_line(timeout: Duration = ...) -> Result[Option[str], io.Error]",
            Self::TlsStreamReadExact => "read_exact(count: int32, timeout: Duration = ...) -> Result[list[uint8], io.Error]",
            Self::TlsStreamWriteAll => "write_all(text: str, timeout: Duration = ...) -> Result[None, io.Error]",
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
            Self::ProcessPipeReadAll => "read_all() -> Result[str, process.Error]",
            Self::ProcessPipeReadLine => {
                "read_line(timeout: Duration = ...) -> Result[Option[str], process.Error]"
            }
            Self::ProcessPipeReadBytes => {
                "read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[Option[list[uint8]], process.Error]"
            }
            Self::ProcessPipeWriteAll => {
                "write_all(text: str, timeout: Duration = ...) -> Result[None, process.Error]"
            }
            Self::ProcessPipeWriteBytes => {
                "write_bytes(bytes: list[uint8], timeout: Duration = ...) -> Result[None, process.Error]"
            }
            Self::ProcessPipeFlush => "flush() -> Result[None, process.Error]",
            Self::ProcessPipeClose => "close() -> None",
            Self::ProcessCompletedStatus => "status() -> process.ExitStatus",
            Self::ProcessCompletedSuccess => "success() -> bool",
            Self::ProcessCompletedStdout => "stdout() -> str",
            Self::ProcessCompletedStdoutBytes => "stdout_bytes() -> list[uint8]",
            Self::ProcessCompletedStderr => "stderr() -> str",
            Self::ProcessCompletedStderrBytes => "stderr_bytes() -> list[uint8]",
            Self::ProcessCompletedCheck => "check() -> Result[None, process.Error]",
            Self::ProcessSupervisorStart => "start(name: own str, command: own list[str], cwd: own Option[str] = ..., env: own dict[str, str] = ..., stdin: own process.Stdio = ..., stdout: own process.Stdio = ..., stderr: own process.Stdio = ..., restart: own process.RestartPolicy = ..., backoff: own Duration = ..., max_restarts: own int32 = ..., group: own bool = ...) -> Result[None, process.Error]",
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
            Self::RngShuffle => "shuffle(values: mut list[T]) -> None",
        }
    }

    pub const fn docs(self) -> &'static str {
        match self {
            Self::FloatSqrt => "Returns the square root of a `float64` value.",
            Self::IntegerToFloat => {
                "Converts an integer to the nearest `float64` value; large values may round."
            }
            Self::IntegerWrappingAdd
            | Self::IntegerWrappingSub
            | Self::IntegerWrappingMul => {
                "Performs fixed-width integer arithmetic with two's-complement wrapping."
            }
            Self::IntegerSaturatingAdd
            | Self::IntegerSaturatingSub
            | Self::IntegerSaturatingMul => {
                "Performs fixed-width integer arithmetic clamped to the receiver type's range."
            }
            Self::IntegerWrappingShl => {
                "Performs fixed-width integer arithmetic by shifting left and discarding high bits after validating the count."
            }
            Self::IntegerWrappingShr => {
                "Performs fixed-width integer arithmetic by shifting right after validating the count, preserving signed arithmetic or unsigned logical semantics."
            }
            Self::IntegerSaturatingShl => {
                "Performs fixed-width integer arithmetic by shifting left and clamping overflow to the receiver type's range after validating the count."
            }
            Self::IntegerSaturatingShr => {
                "Performs fixed-width integer arithmetic by shifting right after validating the count, preserving signed arithmetic or unsigned logical semantics."
            }
            Self::DurationToMilliseconds => {
                "Converts the Duration to the nearest representable number of milliseconds as `float64`."
            }
            Self::DurationToSeconds => {
                "Converts the Duration to the nearest representable number of seconds as `float64`."
            }
            Self::ScalarToString => "Returns a `str` rendering of a numeric or `bool` value.",
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
                "Splits the string on each occurrence of `text` and returns the pieces as `list[str]`."
            }
            Self::StringReplace => {
                "Returns a new `str` with each occurrence of `from` replaced by `to`."
            }
            Self::StringToLower => {
                "Returns a new `str` with Unicode lowercase conversion applied."
            }
            Self::StringToUpper => {
                "Returns a new `str` with Unicode uppercase conversion applied."
            }
            Self::StringStripPrefix => {
                "Removes `text` from the front of the string and returns the remaining `str`, or `Option.None` when it does not match."
            }
            Self::StringStripSuffix => {
                "Removes `text` from the end of the string and returns the remaining `str`, or `Option.None` when it does not match."
            }
            Self::StringTrim => {
                "Returns a new `str` with surrounding Unicode whitespace removed."
            }
            Self::StringJoin => {
                "Joins the `list[str]` parts using the receiver string as the separator."
            }
            Self::StringToBytes => {
                "Returns a fresh `list[uint8]` containing the string's exact UTF-8 encoding."
            }
            Self::ArrayShape => "Returns a fresh list containing every Array dimension.",
            Self::ArrayLen => "Returns the total number of scalar elements in the Array.",
            Self::ArrayClone => "Creates an independent copy of the Array.",
            Self::ArrayGet => {
                "Returns the scalar at an exact-rank coordinate, or `Option.None` when out of bounds."
            }
            Self::ArraySet => {
                "Replaces the scalar at an exact-rank coordinate and returns the previous value."
            }
            Self::ArrayFill => "Replaces every scalar in the Array with `value`.",
            Self::ArrayMap => {
                "Calls a repeatable shared callback for every scalar and returns a new numeric Array."
            }
            Self::ArraySum => "Returns the sum of all Array elements.",
            Self::ArrayMin => "Returns the minimum Array element.",
            Self::ArrayMax => "Returns the maximum Array element.",
            Self::ArrayMean => "Returns the arithmetic mean as `float64`.",
            Self::ArrayWrappingAdd
            | Self::ArrayWrappingSub
            | Self::ArrayWrappingMul => {
                "Performs elementwise integer Array arithmetic with two's-complement wrapping."
            }
            Self::ArraySaturatingAdd
            | Self::ArraySaturatingSub
            | Self::ArraySaturatingMul => {
                "Performs elementwise integer Array arithmetic clamped to the dtype range."
            }
            Self::VecLen => "Returns the current number of elements in the list.",
            Self::VecIsEmpty => "Returns true when the list contains no elements.",
            Self::VecClone => "Creates a new owned `list[T]` with copied element values.",
            Self::VecPush => "Appends a value to the end of the list.",
            Self::VecPop => "Removes and returns the element at `index`.",
            Self::VecGet => {
                "Returns the element at `index`, or `Option.None` when the index is out of bounds."
            }
            Self::VecSet => {
                "Replaces the element at `index` and returns the previous element. Out-of-bounds indices raise a runtime error."
            }
            Self::VecRemove => {
                "Removes the first element equal to `value`. Missing values raise AU4008."
            }
            Self::VecIndex => "Returns the first index containing `value`.",
            Self::VecCount => "Returns the number of elements equal to `value`.",
            Self::VecSwap => {
                "Swaps the elements at `first` and `second`. Out-of-bounds indices raise a runtime error."
            }
            Self::VecContains => "Returns true when the list contains `value`.",
            Self::VecExtend => "Appends the elements of `other` to the end of the list.",
            Self::VecInsert => {
                "Inserts `value` at a Python-clamped `index`."
            }
            Self::VecClear => "Removes all elements from the list.",
            Self::VecReverse => "Reverses the list elements in place.",
            Self::VecSort => {
                "Stably sorts the list in place, optionally using a key function and reverse order."
            }
            Self::VecMap => {
                "Calls `f` with shared access to each element and eagerly returns the owned results as a new list."
            }
            Self::VecFilter => {
                "Calls `f` with shared access to each element and eagerly copies retained elements into a new list."
            }
            Self::VecReserve => "Reserves room for additional list elements.",
            Self::MapLen => "Returns the current number of entries in the map.",
            Self::MapIsEmpty => "Returns true when the map contains no entries.",
            Self::MapClone => "Creates a new owned `dict[K, V]` with copied keys and values.",
            Self::MapGet => {
                "Returns the value for `key`, or `Option.None` when the key is absent."
            }
            Self::MapSet => {
                "Inserts or replaces `key`, returning the previous value as `Option[V]`."
            }
            Self::MapRemove => {
                "Removes `key` and returns its previous value, or `Option.None` when absent."
            }
            Self::MapContainsKey => "Returns true when the dict contains `key`.",
            Self::MapKeys => "Returns the current keys as a `list[K]`.",
            Self::MapValues => "Returns the current values as a `list[V]`.",
            Self::MapItems => "Returns the current entries as `list[(K, V)]` in insertion order.",
            Self::MapClear => "Removes all entries from the map.",
            Self::MapExtend => "Inserts the entries from `other`, replacing matching keys.",
            Self::MapReserve => "Reserves room for additional dictionary entries.",
            Self::SetLen => "Returns the current number of elements in the set.",
            Self::SetIsEmpty => "Returns true when the set contains no elements.",
            Self::SetClone => "Creates a new owned `set[T]` with copied element values.",
            Self::SetContains => "Returns true when the set contains `value`.",
            Self::SetInsert => "Adds `value` when it is not already present.",
            Self::SetRemove => "Removes `value`. Missing values raise AU4008.",
            Self::SetDiscard => "Removes `value` when present.",
            Self::SetClear => "Removes all set elements.",
            Self::SetReserve => "Reserves room for additional set elements.",
            Self::StringClone => "Creates a new owned `str` with the same contents.",
            Self::QueuePut => {
                "Puts a value into the queue, waiting for capacity when needed, or returns `SendError.Closed(value)`, `SendError.Cancelled(value)`, or `SendError.TimedOut(value)` if the send cannot complete. Queue payload type `T` must be Transfer so values are safe to transport between tasks."
            }
            Self::QueueTryPut => {
                "Attempts to put a value into the queue without waiting and returns `SendError.Full(value)` when the queue is already at capacity. Queue payload type `T` must be Transfer so values are safe to transport between tasks."
            }
            Self::QueueGet => {
                "Receives the next queue outcome as `QueueReceive.Item(value)`, `QueueReceive.Closed`, `QueueReceive.TimedOut`, or `QueueReceive.Cancelled`. Queue payload type `T` must be Transfer so values are safe to transport between tasks."
            }
            Self::QueueGetOrNone => {
                "Receives the next queue value and returns `Option.Some(value)`, or `Option.None` when the queue is closed, the timeout expires, or cancellation interrupts the wait. Queue payload type `T` must be Transfer so values are safe to transport between tasks."
            }
            Self::QueueGetOr => {
                "Receives the next queue value or returns `default` when the queue is closed, the timeout expires, or cancellation interrupts the wait. Queue payload type `T` must be Transfer so values are safe to transport between tasks."
            }
            Self::QueueClose => "Closes the queue and wakes blocked receivers.",
            Self::TaskResult => {
                "Waits for the task to finish and reports `TaskResult.Ready(value)`, `TaskResult.Error(message)`, `TaskResult.TimedOut`, or `TaskResult.Cancelled`. Observing non-repeatable `T` consumes the unique `Task[T]` observation right. Copy data, `Queue` handles, and recursively repeatable `Task` handles remain repeatable; `Task[T]` is copyable only when `T` is repeatable."
            }
            Self::TaskResultOrNone => {
                "Waits for the task result and returns `Option.Some(value)`, or `Option.None` when the task fails, the timeout expires, or cancellation interrupts the wait. Observing non-repeatable `T` consumes the unique `Task[T]` observation right. Copy data, `Queue` handles, and recursively repeatable `Task` handles remain repeatable; `Task[T]` is copyable only when `T` is repeatable."
            }
            Self::TaskResultOr => {
                "Waits for the task result or returns `default` when the task fails, the timeout expires, or cancellation interrupts the wait. Observing non-repeatable `T` consumes the unique `Task[T]` observation right. Copy data, `Queue` handles, and recursively repeatable `Task` handles remain repeatable; `Task[T]` is copyable only when `T` is repeatable."
            }
            Self::TaskGroupStart => {
                "Starts a child task on the guarded 512 KiB default stack and returns its handle."
            }
            Self::TaskGroupStartSoon => {
                "Starts a child task on the guarded 512 KiB default stack without returning a task handle."
            }
            Self::TaskGroupStartWithStack => {
                "Starts a child task with a guarded 256 KiB..64 MiB stack request and returns its handle. The 256 KiB minimum is opt-in for a measured shallow task; ordinary starts use the safe 512 KiB default."
            }
            Self::TaskGroupStartSoonWithStack => {
                "Starts a child task with a guarded 256 KiB..64 MiB stack request without returning a task handle. The 256 KiB minimum is opt-in for a measured shallow task; ordinary starts use the safe 512 KiB default."
            }
            Self::TaskGroupCancel => {
                "Signals cancellation to child tasks in the current task group."
            }
            Self::FileReadAll => "Reads the remaining file contents into a `str`.",
            Self::FileReadBytes => "Reads the remaining file contents into `list[uint8]`.",
            Self::FileWriteAll => "Writes all of `text` to the file, returning an `io.Error` on failure.",
            Self::FileWriteBytes => "Writes all of `bytes` to the file, returning an `io.Error` on failure.",
            Self::FileFlush => "Flushes pending file writes to the operating system.",
            Self::FileClose => "Closes the file handle so the resource can no longer be used.",
            Self::TcpListenerAccept => "Accepts the next incoming TCP connection, optionally timing out.",
            Self::TcpListenerLocalAddr => "Returns the bound local address for the listener.",
            Self::TcpListenerClose => "Closes the TCP listener handle.",
            Self::TcpStreamReadAll => "Reads the remaining TCP stream contents into a `str` until the peer closes.",
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
            Self::ProcessPipeReadAll => "Reads the remaining piped output into a str until EOF.",
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
            | Self::ArrayShape
            | Self::ArrayLen
            | Self::ArrayClone
            | Self::ArraySum
            | Self::ArrayMin
            | Self::ArrayMax
            | Self::ArrayMean
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
            | Self::MapClear
            | Self::SetLen
            | Self::SetIsEmpty
            | Self::SetClone
            | Self::SetClear
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
            Self::IntegerWrappingAdd
            | Self::IntegerWrappingSub
            | Self::IntegerWrappingMul
            | Self::IntegerSaturatingAdd
            | Self::IntegerSaturatingSub
            | Self::IntegerSaturatingMul
            | Self::ArrayWrappingAdd
            | Self::ArrayWrappingSub
            | Self::ArrayWrappingMul
            | Self::ArraySaturatingAdd
            | Self::ArraySaturatingSub
            | Self::ArraySaturatingMul => {
                BuiltinCallShape::fixed(&ARITHMETIC_RHS_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::IntegerWrappingShl
            | Self::IntegerWrappingShr
            | Self::IntegerSaturatingShl
            | Self::IntegerSaturatingShr => {
                BuiltinCallShape::fixed(&SHIFT_COUNT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::ArrayGet => {
                BuiltinCallShape::fixed(&ARRAY_INDEX_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::ArraySet => {
                BuiltinCallShape::fixed(&ARRAY_SET_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::ArrayFill => {
                BuiltinCallShape::fixed(&ARRAY_VALUE_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::ArrayMap => {
                BuiltinCallShape::fixed(&ARRAY_CALLBACK_PARAMS, CallConvention::PositionalOrNamed)
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
            Self::VecGet => {
                BuiltinCallShape::fixed(&VEC_INDEX_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::VecPop => {
                BuiltinCallShape::fixed(&VEC_POP_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::VecRemove | Self::VecIndex | Self::VecCount => {
                BuiltinCallShape::fixed(&VALUE_PARAMS, CallConvention::PositionalOrNamed)
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
            Self::VecSort => {
                BuiltinCallShape::fixed(&VEC_SORT_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::VecMap | Self::VecFilter => {
                BuiltinCallShape::fixed(&VEC_CALLBACK_PARAMS, CallConvention::PositionalOrNamed)
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
            Self::VecReserve | Self::MapReserve | Self::SetReserve => {
                BuiltinCallShape::fixed(&RESERVE_PARAMS, CallConvention::PositionalOrNamed)
            }
            Self::SetContains | Self::SetRemove | Self::SetDiscard => {
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
            Self::TaskGroupStartWithStack | Self::TaskGroupStartSoonWithStack => {
                BuiltinCallShape::variadic(&TASK_GROUP_START_WITH_STACK_PARAMS, ReceiverKind::Value)
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

    pub const fn argument_name(self, index: usize) -> Option<&'static str> {
        let shape = self.call_shape();
        if index < shape.params.len() {
            Some(shape.params[index].binding.name)
        } else if shape.variadic_passing.is_some() {
            Some("variadic argument")
        } else {
            None
        }
    }

    pub const fn variadic_argument_passing(self) -> Option<ReceiverKind> {
        self.call_shape().variadic_passing
    }

    pub const fn receiver_passing(self) -> ReceiverKind {
        if matches!(
            self,
            Self::VecPush
                | Self::ArraySet
                | Self::ArrayFill
                | Self::VecPop
                | Self::VecSet
                | Self::VecRemove
                | Self::VecSwap
                | Self::VecExtend
                | Self::VecInsert
                | Self::VecClear
                | Self::VecReverse
                | Self::VecSort
                | Self::VecReserve
                | Self::MapSet
                | Self::MapRemove
                | Self::MapClear
                | Self::MapExtend
                | Self::MapReserve
                | Self::SetInsert
                | Self::SetRemove
                | Self::SetDiscard
                | Self::SetClear
                | Self::SetReserve
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
