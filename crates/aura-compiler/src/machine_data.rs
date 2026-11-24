//! Section and alignment on module data. Literal constants have no dynamic
//! initializer in a freestanding object.
use serde::{Deserialize, Serialize};

use crate::ast::{Decorator, Expr, ExprKind, UnaryOp};
use crate::diag::{Diagnostic, Result};
use crate::machine_attributes::{attribute, valid_symbol};
use crate::sema::{ConstantInfo, Program, Type};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataPlacement {
    pub section: Option<String>,
    pub align: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacedData {
    pub symbol: String,
    /// Immutable literal bytes. Mutable declarations use the existing
    /// checked image carried by the MIR module constant.
    pub bytes: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_key: Option<String>,
    pub placement: DataPlacement,
}

pub(crate) fn placement(decorators: &[Decorator]) -> Result<DataPlacement> {
    let mut placement = DataPlacement::default();
    for d in decorators {
        match attribute(d) {
            Some((name, [arg])) if arg.name.is_none() => match (name, &arg.value.kind) {
                ("section", ExprKind::String(s))
                    if placement.section.is_none() && valid_symbol(s) =>
                {
                    placement.section = Some(s.clone());
                    continue;
                }
                ("align", ExprKind::Int(n))
                    if placement.align.is_none() && n.is_power_of_two() && *n <= 1 << 29 =>
                {
                    placement.align = Some(*n as u32);
                    continue;
                }
                _ => {}
            },
            _ => {}
        }
        return Err(Diagnostic::coded_at(
            "AU2095",
            d.span,
            "module data accepts one section name and one power-of-two alignment",
        ));
    }
    Ok(placement)
}

/// Encode a checked target scalar in the little-endian byte order of every
/// freestanding target. The checker has already validated its value range.
fn scalar_bytes(expr: &Expr, ty: &Type) -> Option<Vec<u8>> {
    let Type::Named(name, _) = ty else {
        return None;
    };
    let (bits, len) = match &expr.kind {
        ExprKind::Group(inner) => return scalar_bytes(inner, ty),
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => {
            let mut bytes = scalar_bytes(expr, ty)?;
            if name.starts_with("float") {
                *bytes.last_mut()? ^= 0x80;
            } else {
                let mut carry = true;
                for byte in &mut bytes {
                    (*byte, carry) = (!*byte).overflowing_add(u8::from(carry));
                }
            }
            return Some(bytes);
        }
        ExprKind::Bool(v) if name == "bool" => return Some(vec![u8::from(*v)]),
        ExprKind::Float(v) if name == "float32" => return Some((*v as f32).to_le_bytes().to_vec()),
        ExprKind::Float(v) if name == "float64" => return Some(v.to_le_bytes().to_vec()),
        ExprKind::Int(v) => {
            let len = match name.as_str() {
                "int8" | "uint8" => 1,
                "int16" | "uint16" => 2,
                "int32" | "uint32" => 4,
                "int64" | "uint64" => 8,
                _ => return None,
            };
            (*v, len)
        }
        _ => return None,
    };
    Some(bits.to_le_bytes()[..len].to_vec())
}

pub(crate) fn check(info: &ConstantInfo) -> Result<()> {
    if info.decl.placement.is_none() {
        return Ok(());
    }
    if crate::freestanding_check_target().is_none() {
        return Err(Diagnostic::coded_at(
            "AU2094",
            info.decl.span,
            "machine attributes require a freestanding build",
        ));
    }
    if info.state.is_none() && scalar_bytes(&info.decl.value, &info.ty).is_none() {
        return Err(Diagnostic::coded_at(
            "AU2095",
            info.decl.value.span,
            "placed module data needs a scalar literal initializer",
        ));
    }
    Ok(())
}

pub(crate) fn collect(program: &Program) -> Vec<PlacedData> {
    program
        .constant_init_plan
        .iter()
        .filter_map(|c| {
            let placement = c.decl.placement.as_deref()?.clone();
            let (symbol, bytes, state_key) = if c.state.is_some() {
                use std::fmt::Write;
                let key = format!("{}::{}", c.module_name, c.decl.name);
                // Match the stable symbol of an unannotated module cell.
                let mut symbol = String::from("__aura_state_");
                for byte in key.bytes() {
                    write!(symbol, "{byte:02x}").unwrap();
                }
                (symbol, Vec::new(), Some(key))
            } else {
                (
                    format!("__aura_data.{}.{}", c.module_name, c.decl.name),
                    scalar_bytes(&c.decl.value, &c.ty)?,
                    None,
                )
            };
            Some(PlacedData {
                symbol,
                bytes,
                state_key,
                placement,
            })
        })
        .collect()
}
