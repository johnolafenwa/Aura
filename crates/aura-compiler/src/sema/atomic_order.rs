use super::*;
use aura_core::atomic_order::AtomicOperation;

pub(super) fn check(function: &FunctionInfo, args: &[Argument], span: crate::diag::Span) -> Result<()> {
    if function.module_name != "mem" {
        return Ok(());
    }
    let Some(op) = AtomicOperation::of(&function.decl.name) else {
        return Ok(());
    };
    let ordered = bind_call_arguments(
        &function.decl.name, &callable_params_from_decl(&function.decl.params), args, span,
        CallConvention::PositionalOrNamed,
    )?;
    let order = |slot: usize| -> Result<Option<&str>> {
        match ordered.get(slot).copied().flatten() {
            None => Ok(None),
            Some(arg) => match &arg.value.kind {
                ExprKind::String(name) => Ok(Some(name.as_str())),
                _ => Err(Diagnostic::coded_at("AU2087", arg.span, "an atomic ordering must be a string literal")),
            },
        }
    };
    op.orders(order(op.arity())?, order(op.arity()+1)?)
        .map(|_| ()).map_err(|message| Diagnostic::coded_at("AU2087", span, message))
}
