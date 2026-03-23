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
                "{} expects {} arguments, found {}",
                callee_name,
                params.len(),
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

        while next_positional < ordered_args.len() && ordered_args[next_positional].is_some() {
            next_positional += 1;
        }

        if next_positional >= ordered_args.len() {
            return Err(Diagnostic::at(
                argument.span,
                format!(
                    "{} expects {} arguments, found {}",
                    callee_name,
                    params.len(),
                    args.len()
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
const AFTER_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("duration")];
const SLEEP_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("duration")];
const CHANNEL_SEND_PARAMS: [CallableParam<'static>; 1] = [CallableParam::required("value")];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuiltinFunction {
    Print,
    Range,
    Channel,
    TaskGroup,
    Cancelled,
    After,
    Sleep,
}

pub const ALL_BUILTIN_FUNCTIONS: &[BuiltinFunction] = &[
    BuiltinFunction::Print,
    BuiltinFunction::Range,
    BuiltinFunction::Channel,
    BuiltinFunction::TaskGroup,
    BuiltinFunction::Cancelled,
    BuiltinFunction::After,
    BuiltinFunction::Sleep,
];

impl BuiltinFunction {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "print" => Some(Self::Print),
            "range" => Some(Self::Range),
            "channel" => Some(Self::Channel),
            "task_group" => Some(Self::TaskGroup),
            "cancelled" => Some(Self::Cancelled),
            "after" => Some(Self::After),
            "sleep" => Some(Self::Sleep),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Print => "print",
            Self::Range => "range",
            Self::Channel => "channel",
            Self::TaskGroup => "task_group",
            Self::Cancelled => "cancelled",
            Self::After => "after",
            Self::Sleep => "sleep",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::Print => "print(value) -> None",
            Self::Range => "range(stop: int32) -> Range; range(start: int32, stop: int32) -> Range",
            Self::Channel => "channel() -> Channel[T]",
            Self::TaskGroup => "task_group() -> TaskGroup",
            Self::Cancelled => "cancelled() -> bool",
            Self::After => "after(duration: Duration) -> Duration",
            Self::Sleep => "sleep(duration: Duration) -> None",
        }
    }

    pub const fn docs(self) -> &'static str {
        match self {
            Self::Print => "Writes a value followed by a newline.",
            Self::Range => {
                "Builds an integer range from 0 up to, but not including, `stop`, or from `start` up to, but not including, `stop`."
            }
            Self::Channel => {
                "Creates a typed channel when the surrounding annotation or expectation provides `T`."
            }
            Self::TaskGroup => {
                "Creates a managed structured-concurrency task group for use with `with`."
            }
            Self::Cancelled => "Returns true when the current task has been cancelled.",
            Self::After => {
                "Builds a timeout/select timer expression from a duration literal or duration value."
            }
            Self::Sleep => "Blocks the current task for the requested duration.",
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
            Self::Channel => {
                bind_call_arguments("`channel`", &[], args, span, CallConvention::PositionalOnly)
            }
            Self::TaskGroup => bind_call_arguments(
                "`task_group`",
                &[],
                args,
                span,
                CallConvention::PositionalOnly,
            ),
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
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuiltinMember {
    FloatSqrt,
    StringClone,
    ChannelClone,
    ChannelSend,
    ChannelRecv,
    ChannelClose,
    TaskClone,
    TaskJoin,
    TaskGroupCancel,
}

impl BuiltinMember {
    pub fn resolve(receiver_base: &str, name: &str) -> Option<Self> {
        match (receiver_base, name) {
            ("float64", "sqrt") => Some(Self::FloatSqrt),
            ("String", "clone") => Some(Self::StringClone),
            ("Channel", "clone") => Some(Self::ChannelClone),
            ("Channel", "send") => Some(Self::ChannelSend),
            ("Channel", "recv") => Some(Self::ChannelRecv),
            ("Channel", "close") => Some(Self::ChannelClose),
            ("Task", "clone") => Some(Self::TaskClone),
            ("Task", "join") => Some(Self::TaskJoin),
            ("TaskGroup", "cancel") => Some(Self::TaskGroupCancel),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::FloatSqrt => "sqrt",
            Self::StringClone | Self::ChannelClone | Self::TaskClone => "clone",
            Self::ChannelSend => "send",
            Self::ChannelRecv => "recv",
            Self::ChannelClose => "close",
            Self::TaskJoin => "join",
            Self::TaskGroupCancel => "cancel",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::FloatSqrt => "sqrt() -> float64",
            Self::StringClone => "clone() -> String",
            Self::ChannelClone => "clone() -> Channel[T]",
            Self::ChannelSend => "send(value) -> Result[None, SendError[T]]",
            Self::ChannelRecv => "recv() -> Option[T]",
            Self::ChannelClose => "close() -> None",
            Self::TaskClone => "clone() -> Task[T]",
            Self::TaskJoin => "join() -> T",
            Self::TaskGroupCancel => "cancel() -> None",
        }
    }

    pub const fn docs(self) -> &'static str {
        match self {
            Self::FloatSqrt => "Returns the square root of a `float64` value.",
            Self::StringClone => "Creates a new owned `String` with the same contents.",
            Self::ChannelClone => "Creates another handle to the same underlying channel.",
            Self::ChannelSend => {
                "Sends a value to the channel or returns `SendError.Closed(value)` if the channel is closed."
            }
            Self::ChannelRecv => {
                "Receives the next value from the channel, or `Option.None` when closed."
            }
            Self::ChannelClose => "Closes the channel and wakes blocked receivers.",
            Self::TaskClone => "Creates another handle to the same spawned task.",
            Self::TaskJoin => "Waits for the spawned task to finish and returns its value.",
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
            | Self::StringClone
            | Self::ChannelClone
            | Self::ChannelRecv
            | Self::ChannelClose
            | Self::TaskClone
            | Self::TaskJoin
            | Self::TaskGroupCancel => bind_call_arguments(
                &format!("`{}`", self.name()),
                &[],
                args,
                span,
                CallConvention::PositionalOnly,
            ),
            Self::ChannelSend => bind_call_arguments(
                "`send`",
                &CHANNEL_SEND_PARAMS,
                args,
                span,
                CallConvention::PositionalOrNamed,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{Argument, Expr, ExprKind};
    use crate::diag::Span;

    use super::{bind_call_arguments, CallConvention, CallableParam};

    fn dummy_arg(name: Option<&str>) -> Argument {
        Argument {
            name: name.map(|value| value.to_string()),
            value: Expr {
                kind: ExprKind::Int(1),
                span: Span::new(1, 1),
            },
            span: Span::new(1, 1),
        }
    }

    #[test]
    fn positional_or_named_binding_orders_arguments() {
        let params = [
            CallableParam::required("left"),
            CallableParam::required("right"),
        ];
        let args = [dummy_arg(None), dummy_arg(Some("right"))];
        let ordered = bind_call_arguments(
            "function `add`",
            &params,
            &args,
            Span::new(1, 1),
            CallConvention::PositionalOrNamed,
        )
        .expect("binding should succeed");

        assert!(ordered[0].is_some());
        assert!(ordered[1].is_some());
        assert_eq!(ordered[1].unwrap().name.as_deref(), Some("right"));
    }

    #[test]
    fn keyword_only_binding_rejects_positional_arguments() {
        let params = [CallableParam::required("value")];
        let error = bind_call_arguments(
            "class constructor `Box`",
            &params,
            &[dummy_arg(None)],
            Span::new(1, 1),
            CallConvention::KeywordOnly,
        )
        .expect_err("binding should fail");

        assert!(error.message.contains("must be named"));
    }
}
