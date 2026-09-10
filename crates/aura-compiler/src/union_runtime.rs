//! Structural union identity shared by the MIR interpreter and the direct
//! backend (ADR-0052 A7).
//!
//! Generic function bodies execute once for every specialization, so a union
//! value built inside such a body carries the symbolic union it was written
//! with (`V | None`), while a concrete caller tests the same value against its
//! specialized union (`int64 | None`). Rather than converting values at every
//! boundary, each union operation aligns the value to the union named by the
//! instruction: the active member is identified by its concrete type (a
//! concrete member of the value's union, or the runtime type of the payload
//! when that member is a type parameter), that identity is located in the
//! instruction's union (a concrete member by identity, otherwise the member
//! that is a type parameter), and the value is retagged to that layout. A
//! payload is never itself a union value: injecting a union-typed `V` payload
//! flattens it into the destination without a phantom outer tag. Any active
//! member that the instruction's union cannot admit is rejected, so forged or
//! mismatched MIR still fails closed.

use crate::runtime_value::{UnionValue, Value};
use crate::sema::{Type, UnionType};

/// How a value must change to take the layout of a target union.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UnionAlignment {
    /// The value already carries the target union.
    Exact,
    /// The value is a union whose active member sits at this target index.
    Retag(usize),
    /// The value is a bare member value that becomes this target member.
    Lift(usize),
}

fn is_type_param(ty: &Type) -> bool {
    matches!(ty, Type::TypeParam(_))
}

/// Backend-independent runtime type of a value, used to identify payloads
/// stored behind a type-parameter member.
pub(crate) fn runtime_member_type(value: &Value) -> Option<Type> {
    match value {
        Value::Union(union) => member_identity(&Value::Union(union.clone())),
        Value::Int(value) => Some(Type::named(value.runtime_type_name().unwrap_or("int64"))),
        Value::Float(_) => Some(Type::named("float64")),
        Value::Bool(_) => Some(Type::named("bool")),
        Value::String(_) => Some(Type::named("str")),
        Value::Tuple(tuple) => Some(Type::Tuple(tuple.element_types.clone())),
        Value::Vec(vector) => Some(Type::Named(
            "list".to_string(),
            vec![vector.element_type.clone()],
        )),
        Value::Array(array) => Some(Type::Named("Array".to_string(), vec![array.element_type()])),
        Value::Set(set) => Some(Type::Named(
            "set".to_string(),
            vec![set.element_type.clone()],
        )),
        Value::Map(map) => Some(Type::Named(
            "dict".to_string(),
            vec![map.key_type.clone(), map.value_type.clone()],
        )),
        Value::Duration(_) => Some(Type::named("Duration")),
        Value::Rng(_) => Some(Type::named("random.Rng")),
        Value::Range(_) => Some(Type::named("Range")),
        Value::Function(function) => Some(function.signature.clone()),
        Value::Unit => Some(Type::Unit),
        Value::FfiHandle(handle) => Some(Type::named(handle.type_name())),
        Value::Instance(instance) => Some(Type::named(&instance.class_name)),
        Value::EnumVariant(variant) => Some(Type::named(&variant.enum_name)),
        _ => None,
    }
}

/// Active-member equality between two values of which at least one is a
/// union: the active member identities must match and the payloads must be
/// equal. Different tags are unequal even when the payloads coincide.
pub(crate) fn union_values_equal(left: &Value, right: &Value) -> bool {
    let (Some(left_identity), Some(right_identity)) =
        (member_identity(left), member_identity(right))
    else {
        return false;
    };
    if !member_matches(&left_identity, &right_identity) {
        return false;
    }
    let payload = |value: &Value| -> Value {
        match value {
            Value::Union(union) => union.payload.clone(),
            other => other.clone(),
        }
    };
    payload(left) == payload(right)
}

/// Whether a value is unit `None` or a union currently holding it.
pub(crate) fn is_none_value(value: &Value) -> bool {
    matches!(member_identity(value), Some(Type::Unit))
}

/// The concrete identity of the member a value currently holds.
pub(crate) fn member_identity(value: &Value) -> Option<Type> {
    match value {
        Value::Union(union) => match union.union_type.union_members()?.get(union.member_index)? {
            Type::TypeParam(_) => runtime_member_type(&union.payload),
            member => Some(member.clone()),
        },
        other => runtime_member_type(other),
    }
}

/// Whether a runtime member identity denotes a concrete union member. A
/// runtime class name is unqualified inside its own module while the member
/// type may carry the module prefix, so nominal names compare by their final
/// segment when exactly one side is qualified.
pub(crate) fn member_matches(active: &Type, member: &Type) -> bool {
    if active == member {
        return true;
    }
    match (active, member) {
        (Type::Named(left, left_args), Type::Named(right, right_args)) => {
            if left_args != right_args {
                return false;
            }
            let left_qualified = left.contains('.');
            let right_qualified = right.contains('.');
            if left_qualified == right_qualified {
                return false;
            }
            let (qualified, bare) = if left_qualified {
                (left, right)
            } else {
                (right, left)
            };
            qualified
                .rsplit_once('.')
                .is_some_and(|(_, tail)| tail == bare)
        }
        _ => false,
    }
}

/// Plans the alignment of `value` to `target`, or reports why the value's
/// active member is not admissible there.
pub(crate) fn plan_union_alignment(
    value: &Value,
    target: &UnionType,
    operation: &str,
) -> Result<UnionAlignment, String> {
    let target_type = Type::Union(Box::new(target.clone()));
    if let Value::Union(union) = value {
        if union.union_type == target_type {
            return Ok(UnionAlignment::Exact);
        }
    }
    let Some(active) = member_identity(value) else {
        return Err(format!(
            "{operation} cannot identify the active union member"
        ));
    };
    let index = target
        .members
        .iter()
        .position(|member| !is_type_param(member) && member_matches(&active, member))
        .or_else(|| target.members.iter().position(is_type_param))
        .ok_or_else(|| format!("{operation} type identity mismatch"))?;
    Ok(match value {
        Value::Union(_) => UnionAlignment::Retag(index),
        _ => UnionAlignment::Lift(index),
    })
}

/// The index the value's active member takes in `target`, without changing
/// the value.
pub(crate) fn aligned_member_index(
    value: &Value,
    target: &UnionType,
    operation: &str,
) -> Result<usize, String> {
    match plan_union_alignment(value, target, operation)? {
        UnionAlignment::Exact => match value {
            Value::Union(union) => Ok(union.member_index),
            _ => unreachable!("an exact alignment is a union value"),
        },
        UnionAlignment::Retag(index) | UnionAlignment::Lift(index) => Ok(index),
    }
}

/// Retags `value` in place to the layout of `target`, lifting a bare member
/// value into the union when needed.
pub(crate) fn align_union_value(
    value: &mut Value,
    target: &UnionType,
    operation: &str,
) -> Result<usize, String> {
    match plan_union_alignment(value, target, operation)? {
        UnionAlignment::Exact => match value {
            Value::Union(union) => Ok(union.member_index),
            _ => unreachable!("an exact alignment is a union value"),
        },
        UnionAlignment::Retag(index) => {
            let Value::Union(union) = value else {
                unreachable!("a retag alignment is a union value")
            };
            union.union_type = Type::Union(Box::new(target.clone()));
            union.member_index = index;
            Ok(index)
        }
        UnionAlignment::Lift(index) => {
            let payload = std::mem::replace(value, Value::Unit);
            *value = Value::Union(Box::new(UnionValue {
                union_type: Type::Union(Box::new(target.clone())),
                member_index: index,
                payload,
            }));
            Ok(index)
        }
    }
}

/// Builds the union value for an injection. A union-typed payload behind a
/// type-parameter member flattens into the destination layout instead of
/// nesting.
pub(crate) fn inject_union_member(
    target: &UnionType,
    member_index: usize,
    payload: Value,
) -> Result<Value, String> {
    let Some(member) = target.members.get(member_index) else {
        return Err("union injection member index is out of range".to_string());
    };
    if let Value::Union(_) = &payload {
        if !is_type_param(member) {
            return Err("union injection cannot nest a union inside a concrete member".to_string());
        }
        let mut flattened = payload;
        align_union_value(&mut flattened, target, "union injection")?;
        return Ok(flattened);
    }
    Ok(Value::Union(Box::new(UnionValue {
        union_type: Type::Union(Box::new(target.clone())),
        member_index,
        payload,
    })))
}

/// Normalizes a value crossing a typed boundary: a union takes the layout of
/// an expected union, a bare member lifts into an expected union, and a union
/// whose active member is the expected non-union type unwraps to its payload.
/// Values that cannot be related to the expected type are returned unchanged
/// so the following operation reports the mismatch.
pub(crate) fn coerce_union_boundary(value: Value, expected: &Type) -> Value {
    match expected {
        Type::Union(target) => {
            let mut value = value;
            if matches!(&value, Value::Union(union) if union.union_type == *expected) {
                return value;
            }
            let admissible = matches!(&value, Value::Union(_))
                || member_identity(&value).is_some_and(|active| {
                    target
                        .members
                        .iter()
                        .any(|member| !is_type_param(member) && member_matches(&active, member))
                });
            if admissible && align_union_value(&mut value, target, "union boundary").is_ok() {
                return value;
            }
            value
        }
        Type::TypeParam(_) => value,
        Type::Named(name, _) if name == "Unknown" => value,
        other => match value {
            Value::Union(union)
                if member_identity(&Value::Union(union.clone()))
                    .is_some_and(|active| member_matches(&active, other)) =>
            {
                union.payload
            }
            value => value,
        },
    }
}

trait UnionMembers {
    fn union_members(&self) -> Option<&[Type]>;
}

impl UnionMembers for Type {
    fn union_members(&self) -> Option<&[Type]> {
        match self {
            Type::Union(union) => Some(&union.members),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "union_runtime_tests.rs"]
mod tests;
