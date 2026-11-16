//! `control.recover`, which runs a function and hands a trap back as a
//! value (ADR-0104).
//!
//! The call is one runtime operation. To give it a type, the checker and
//! the lowering both read `control.recover(function, arguments...)` as two
//! calls they already know how to type:
//!
//! ```text
//! control.__aura_caught(control.<raw>(function, arguments...))
//! ```
//!
//! The raw call is checked as `TaskGroup.start` is checked and has the type
//! `Result[T, RawTrap]`, and `__aura_caught`, an ordinary generic function
//! in the Aura-source part of `control`, maps that to
//! `Result[T, control.Trap]`. Neither call is ever lowered: the lowering
//! emits the operation itself, and the runtime builds the `control.Trap`.
//! The raw member's name is not an identifier, so no program can write it.

use crate::ast::{Argument, Expr, ExprKind};
use crate::diag::Span;

use super::Type;

/// The builtin module that holds `recover`.
pub(crate) const MODULE: &str = "control";
/// The member a program calls.
pub(crate) const RECOVER: &str = "recover";
/// The operation `recover` is rewritten to.
pub(crate) const RECOVER_RAW: &str = "recover:raw";
/// The function that turns the operation's result into `Result[T, Trap]`.
pub(crate) const CAUGHT: &str = "__aura_caught";

/// The parts of a trap as the runtime hands them over: code, message,
/// path, line, column, and the call chain, innermost first.
pub(crate) fn raw_trap_type() -> Type {
    Type::Tuple(vec![
        Type::named("str"),
        Type::named("str"),
        Type::named("str"),
        Type::named("int64"),
        Type::named("int64"),
        Type::Named("list".to_string(), vec![Type::named("str")]),
    ])
}

/// The type of `control.__aura_recover(...)` for a function that returns
/// `child_return`.
pub(crate) fn raw_result_type(child_return: Type) -> Type {
    Type::Named("Result".to_string(), vec![child_return, raw_trap_type()])
}

/// Rewrites `module.recover(args...)`, where `module` names the `control`
/// module, as `module.__aura_caught(module.__aura_recover(args...))`.
/// Every new node carries the span of the call.
pub(crate) fn desugar_recover_call(module: &Expr, args: &[Argument], span: Span) -> Expr {
    let member = |field: &str| Expr {
        kind: ExprKind::Member {
            object: Box::new(module.clone()),
            field: field.to_string(),
        },
        span,
    };
    let raw = Expr {
        kind: ExprKind::Call {
            callee: Box::new(member(RECOVER_RAW)),
            args: args.to_vec(),
        },
        span,
    };
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(member(CAUGHT)),
            args: vec![Argument {
                name: None,
                value: raw,
                span,
            }],
        },
        span,
    }
}
