//! Evaluate module initializers once the complete import graph has been checked.
use crate::ast::{Expr, ExprKind, UnaryOp};
use crate::diag::{Diagnostic, Result, Span};
use crate::integer::{IntegerKind, IntegerRepresentation, IntegerValue};
use crate::mir;
use crate::sema::{ConstantInfo, Program, Type};
pub use aura_runtime::frozen_value::FrozenValue;

pub(crate) const STEP_LIMIT: usize = 100_000;
pub(crate) const MEMORY_LIMIT: usize = 8 * 1024 * 1024;

pub(crate) fn evaluate(program: &mut Program) -> Result<()> {
    if program.constant_init_plan.is_empty() {
        return Ok(());
    }
    let mut needs_interpreter = false;
    for constant in &program.constant_init_plan {
        if let Some(value) = literal(constant) {
            program.frozen_constants.insert(key(constant), value);
        } else {
            needs_interpreter = true;
        }
    }
    if !needs_interpreter {
        return Ok(());
    }
    let mut module = mir::lower_implicit(program);
    mir::methods::resolve_methods(&mut module);
    let plan = &program.constant_init_plan;
    let evaluated = crate::mir_runtime::evaluate_constants(module)?;
    for constant in plan {
        let key = key(constant);
        if let Some(value) = evaluated.get(&key) {
            program.frozen_constants.insert(key, value.clone());
        } else if constant.decl.required && !program.frozen_constants.contains_key(&key) {
            return Err(Diagnostic::coded_at(
                "AU3025", constant.decl.span,
                format!("module constant `{}` cannot be evaluated at build time", constant.decl.name),
            ));
        }
    }
    Ok(())
}

fn key(constant: &ConstantInfo) -> String {
    format!("{}::{}", constant.module_name, constant.decl.name)
}

/// A literal-only program needs neither a second MIR lowering nor an interpreter.
fn literal(constant: &ConstantInfo) -> Option<FrozenValue> {
    let kind = &constant.decl.value.kind;
    let ty = &constant.ty;
    match kind {
        ExprKind::String(s) if s.len() <= MEMORY_LIMIT => Some(FrozenValue::Str(s.clone())),
        ExprKind::Bool(b) => Some(FrozenValue::Bool(*b)),
        ExprKind::DurationNanos(n) => Some(FrozenValue::Duration(*n)),
        ExprKind::Int(i) => {
            let Type::Named(name, _) = ty else { return None; };
            if name == "float64" { return Some(FrozenValue::Float((*i as f64).to_bits())); }
            if name == "float32" { return Some(FrozenValue::Float(f64::from(*i as f32).to_bits())); }
            let value = IntegerValue::from_literal(*i).with_runtime_kind(IntegerKind::from_runtime_type_name(name)?)?;
            Some(FrozenValue::Int(value))
        }
        ExprKind::Float(v) => {
            let v = if ty == &Type::named("float32") { f64::from(*v as f32) } else { *v };
            Some(FrozenValue::Float(if v.is_nan() { f64::NAN.to_bits() } else { v.to_bits() }))
        }
        _ => None,
    }
}

/// Use ordinary typed literal lowering for computed scalars, including signed minima.
pub(crate) fn scalar_expr(value: &FrozenValue, span: Span) -> Option<Expr> {
    let kind = match value {
        FrozenValue::Int(n) => match n.representation() {
            IntegerRepresentation::Unsigned(n) => ExprKind::Int(n),
            IntegerRepresentation::Signed(n) if n >= 0 => ExprKind::Int(n as u128),
            IntegerRepresentation::Signed(n) => ExprKind::Unary {
                op: UnaryOp::Neg, expr: Box::new(Expr { kind: ExprKind::Int(n.unsigned_abs()), span })
            },
        },
        FrozenValue::Bool(v) => ExprKind::Bool(*v),
        FrozenValue::Duration(v) => ExprKind::DurationNanos(*v),
        FrozenValue::Float(bits) => ExprKind::Float(f64::from_bits(*bits)),
        _ => return None,
    };
    Some(Expr { kind, span })
}

#[cfg(test)]
#[path = "const_eval_tests.rs"]
mod tests;
