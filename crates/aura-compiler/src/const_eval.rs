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
    program
        .frozen_constants
        .extend(crate::mir_runtime::evaluate_constants(module)?);
    for constant in &program.constant_init_plan {
        if constant.decl.required && !program.frozen_constants.contains_key(&key(constant)) {
            return Err(Diagnostic::coded_at(
                "AU3025",
                constant.decl.span,
                format!(
                    "module constant `{}` cannot be evaluated at build time",
                    constant.decl.name
                ),
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
        ExprKind::String(s) if ty == &Type::named("str") && s.len() <= MEMORY_LIMIT => {
            Some(FrozenValue::Str(s.clone()))
        }
        ExprKind::Bool(b) if ty == &Type::named("bool") => Some(FrozenValue::Bool(*b)),
        ExprKind::DurationNanos(n) if ty == &Type::named("Duration") => {
            Some(FrozenValue::Duration(*n))
        }
        ExprKind::Int(i) => {
            let Type::Named(name, _) = ty else {
                return None;
            };
            if name == "float64" {
                return Some(FrozenValue::Float((*i as f64).to_bits()));
            }
            if name == "float32" {
                return Some(FrozenValue::Float(f64::from(*i as f32).to_bits()));
            }
            let value = IntegerValue::from_literal(*i)
                .with_runtime_kind(IntegerKind::from_runtime_type_name(name)?)?;
            Some(FrozenValue::Int(value))
        }
        ExprKind::Map(entries)
            if constant.module_name == "build"
                && constant.decl.name == "cfg"
                && matches!(ty, Type::Named(name,args) if name=="dict" && args==&[Type::named("str"),Type::named("str")]) =>
        {
            // The builtin's table has unique keys in configuration order.
            let mut bytes = 0usize;
            let mut pairs = Vec::new();
            for entry in entries {
                let (ExprKind::String(k), ExprKind::String(v)) =
                    (&entry.key.kind, &entry.value.kind)
                else {
                    return None;
                };
                bytes = bytes
                    .saturating_add(k.len())
                    .saturating_add(v.len())
                    .saturating_add(16);
                if bytes > MEMORY_LIMIT {
                    return None;
                }
                pairs.push((FrozenValue::Str(k.clone()), FrozenValue::Str(v.clone())));
            }
            Some(FrozenValue::Dict(
                Type::named("str"),
                Type::named("str"),
                pairs,
            ))
        }
        ExprKind::Float(v) if matches!(ty, Type::Named(name, _) if name == "float32" || name == "float64") =>
        {
            let v = if ty == &Type::named("float32") {
                f64::from(*v as f32)
            } else {
                *v
            };
            Some(FrozenValue::Float(if v.is_nan() {
                f64::NAN.to_bits()
            } else {
                v.to_bits()
            }))
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
                op: UnaryOp::Neg,
                expr: Box::new(Expr {
                    kind: ExprKind::Int(n.unsigned_abs()),
                    span,
                }),
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

/// JSON numbers cannot encode infinity or NaN. Preserve finite numeric MIR
/// as written by existing compilers and transport non-finite values by bits.
pub(crate) mod float_operand {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize)]
    struct Bits {
        bits: u64,
    }

    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        if value.is_finite() {
            serializer.serialize_f64(*value)
        } else {
            Bits {
                bits: value.to_bits(),
            }
            .serialize(serializer)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        value
            .as_f64()
            .or_else(|| value.get("bits")?.as_u64().map(f64::from_bits))
            .ok_or_else(|| serde::de::Error::custom("expected a float or its IEEE bits"))
    }
}
