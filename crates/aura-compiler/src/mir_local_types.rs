//! Complete local types (Batch 7 step 8c).
//!
//! Lowering records the type of almost every local where it creates it. A
//! few temporaries come from paths that name no type, such as a `print`
//! result, a trait call on a bounded generic receiver, or a copy of another
//! temporary. This pass gives each of them the type of its rvalue, so every
//! assigned local has a recorded type. The validator then requires one, and
//! no backend infers the type of a temporary.

use super::{
    type_contains_unknown, CallTarget, Instruction, Lowerer, MirArg, MirFunction, MirParam,
    Operand, Rvalue,
};
use crate::ast::{BinaryOp, UnaryOp};
use crate::call::BuiltinMember;
use crate::runtime_ops::{builtin_function_for_name, RuntimeOp};
use crate::sema::{substitute_type, substitutions_from_decl_type_args, Type};
use std::collections::{BTreeSet, HashMap};

impl Lowerer<'_> {
    /// Records a type for each assigned local that has none, or whose type
    /// names `Unknown`. A local assigned more than once takes the type of
    /// its last assignment, and the pass repeats until no type changes,
    /// since one temporary often copies another.
    pub(super) fn complete_local_types(&mut self, params: &[MirParam]) {
        let param_types = params
            .iter()
            .map(|param| (param.name.as_str(), &param.ty))
            .collect::<HashMap<_, _>>();
        let pending = self
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .filter_map(|instruction| match instruction {
                Instruction::Assign { target, value }
                    if !target.contains('.')
                        && !param_types.contains_key(target.as_str())
                        && self
                            .local_types
                            .get(target)
                            .is_none_or(type_contains_unknown) =>
                {
                    Some((target.clone(), value.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for _ in 0..=pending.len() {
            let mut changed = false;
            for (target, value) in &pending {
                let Some(ty) = self.completed_rvalue_type(value, &param_types) else {
                    continue;
                };
                if type_contains_unknown(&ty) {
                    continue;
                }
                if self.local_types.get(target) != Some(&ty) {
                    self.local_types.insert(target.clone(), ty);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn completed_rvalue_type(&self, value: &Rvalue, params: &HashMap<&str, &Type>) -> Option<Type> {
        match value {
            Rvalue::Use(operand) | Rvalue::CallableAdapt { value: operand, .. } => {
                self.completed_operand_type(operand, params)
            }
            Rvalue::Binary {
                op, left, right, ..
            } => {
                let duration = Type::named("Duration");
                match op {
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Less
                    | BinaryOp::LessEq
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEq
                    | BinaryOp::And
                    | BinaryOp::Or => Some(Type::named("bool")),
                    // A Duration scales by a number on either side.
                    BinaryOp::Mul
                        if self.completed_operand_type(right, params).as_ref()
                            == Some(&duration) =>
                    {
                        Some(duration)
                    }
                    _ => self.completed_operand_type(left, params),
                }
            }
            Rvalue::Call { callee, args } => self.completed_call_type(callee, args, params),
            Rvalue::VariantPayload {
                scrutinee,
                variant_name,
                index,
            } => {
                let scrutinee_ty = self.completed_operand_type(scrutinee, params)?;
                let Type::Named(enum_name, _) = &scrutinee_ty else {
                    return None;
                };
                self.variant_payload_types(Some(&scrutinee_ty), enum_name, variant_name)?
                    .get(*index)
                    .cloned()
            }
            Rvalue::Try { value, .. } => match self.completed_operand_type(value, params)? {
                Type::Named(name, mut args) if name == "Result" && args.len() == 2 => {
                    Some(args.remove(0))
                }
                _ => None,
            },
            Rvalue::EnumVariant { enum_name, .. } => Some(Type::named(enum_name)),
            Rvalue::Member { object, field } => {
                let object_ty = self.completed_operand_type(object, params)?;
                self.completed_field_type(&object_ty, field)
            }
            Rvalue::Unary { op, value, .. } => match op {
                UnaryOp::Not => Some(Type::named("bool")),
                UnaryOp::Neg | UnaryOp::BitNot => self.completed_operand_type(value, params),
            },
            Rvalue::Cast { ty, .. } => Some(ty.clone()),
            Rvalue::UnionTagTest { .. } | Rvalue::NoneTest { .. } => Some(Type::named("bool")),
            Rvalue::UnionTakePayload { member_type, .. } => Some(member_type.clone()),
            Rvalue::UnionInject { union_type, .. } => Some(union_type.clone()),
            Rvalue::Closure { signature, .. } => Some(signature.clone()),
            Rvalue::FormatString { .. } => Some(Type::named("str")),
            Rvalue::Construct { class_name, .. } => Some(Type::named(class_name)),
            Rvalue::TupleElement { element_type, .. }
            | Rvalue::TupleTakeElement { element_type, .. } => Some(element_type.clone()),
            _ => None,
        }
    }

    fn completed_call_type(
        &self,
        callee: &CallTarget,
        args: &[MirArg],
        params: &HashMap<&str, &Type>,
    ) -> Option<Type> {
        let arg_types = || {
            args.iter()
                .map(|argument| self.completed_operand_type(&argument.value, params))
                .collect::<Vec<_>>()
        };
        match callee {
            CallTarget::Name(name) => {
                let user_function = self.resolve_function_info(name).is_some();
                builtin_function_for_name(name, |_| user_function)?.result_type(None, &arg_types())
            }
            CallTarget::Member { object, field, .. }
            | CallTarget::TraitMember { object, field, .. } => {
                let receiver = self.completed_operand_type(object, params)?;
                self.completed_member_call_type(&receiver, field)
            }
            CallTarget::Runtime { op, receiver, .. } => {
                let receiver = match receiver {
                    Some(object) => Some(self.completed_operand_type(object, params)?),
                    None => None,
                };
                op.result_type(receiver.as_ref(), &arg_types())
            }
            CallTarget::Extern(call) => Some(call.return_type.clone()),
            // A call through a callable value has the contract's pointee
            // type when the contract returns a view (C9).
            CallTarget::Value(function) => match self.completed_operand_type(function, params)? {
                Type::Function { return_type, .. } | Type::Closure { return_type, .. } => {
                    Some(crate::sema::returned_view_pointee(&return_type).clone())
                }
                Type::Callable(callable) => {
                    Some(crate::sema::returned_view_pointee(&callable.return_type).clone())
                }
                _ => None,
            },
        }
    }

    fn completed_member_call_type(&self, receiver: &Type, field: &str) -> Option<Type> {
        if let Some((_, _, method, substitutions)) =
            self.bounded_trait_method_for_receiver(receiver, field)
        {
            return Some(substitute_type(
                &method.signature.return_type,
                &substitutions,
            ));
        }
        if let Type::Union(union) = receiver {
            if field == "clone" {
                return Some(receiver.clone());
            }
            for member in &union.members {
                if let Some(ty) = self.completed_member_call_type(member, field) {
                    return Some(ty);
                }
            }
            return None;
        }
        if let Type::Named(name, args) = receiver {
            if let Some(class) = self.resolve_class_info(name) {
                if let Some(method) = class.methods.get(field) {
                    let substitutions =
                        substitutions_from_decl_type_args(&class.decl.type_params, args);
                    return Some(substitute_type(
                        &method.signature.return_type,
                        &substitutions,
                    ));
                }
            }
            if let Some(op) = RuntimeOp::resolve_member(receiver, field, |_| false) {
                return op.result_type(Some(receiver), &[]);
            }
        }
        if let Some(ty) = self.builtin_runtime_member_return_type(receiver, field) {
            return Some(ty);
        }
        let (method, substitutions) = self.trait_method_for_receiver(receiver, field)?;
        Some(substitute_type(
            &method.signature.return_type,
            &substitutions,
        ))
    }

    fn completed_operand_type(
        &self,
        operand: &Operand,
        params: &HashMap<&str, &Type>,
    ) -> Option<Type> {
        let (Operand::Place(place) | Operand::MovePlace(place)) = operand else {
            return match operand {
                Operand::Int(value) => i64::try_from(*value).is_ok().then(|| Type::named("int64")),
                _ => self.infer_operand_type(operand),
            };
        };
        let mut segments = place.split('.');
        let root = segments.next()?;
        let mut ty = self
            .local_types
            .get(root)
            .or_else(|| params.get(root).copied())
            .cloned()?;
        for segment in segments {
            ty = self.completed_field_type(&ty, segment)?;
        }
        Some(ty)
    }

    fn completed_field_type(&self, ty: &Type, field: &str) -> Option<Type> {
        match ty {
            Type::Tuple(elements) => elements.get(field.parse::<usize>().ok()?).cloned(),
            Type::Union(union) => union
                .members
                .get(super::union_payload_projection_index(field)?)
                .cloned(),
            Type::Named(name, args) => {
                let class = self.resolve_class_info(name)?;
                let substitutions =
                    substitutions_from_decl_type_args(&class.decl.type_params, args);
                class
                    .fields
                    .get(field)
                    .map(|field| substitute_type(&field.ty, &substitutions))
            }
            _ => None,
        }
    }
}

/// Every local an `Assign` writes as a whole is a parameter or has a
/// recorded type that names no `Unknown`, so no backend infers one.
pub(super) fn validate_assigned_local_types(
    function: &MirFunction,
) -> std::result::Result<(), String> {
    let params = function
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect::<BTreeSet<_>>();
    let local_types = function
        .local_types
        .iter()
        .map(|local| (local.name.as_str(), &local.ty))
        .collect::<HashMap<_, _>>();
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        let Instruction::Assign { target, .. } = instruction else {
            continue;
        };
        if target.contains('.') || params.contains(target.as_str()) {
            continue;
        }
        match local_types.get(target.as_str()) {
            None => {
                return Err(format!(
                    "invalid MIR function `{}` assigns untyped local `{target}`",
                    function.name
                ))
            }
            Some(ty) if type_contains_unknown(ty) => {
                return Err(format!(
                    "invalid MIR function `{}` assigns local `{target}` of unknown type `{ty}`",
                    function.name
                ))
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// The collection type of a builtin member parameter, for an argument whose
/// own type is open, such as an empty literal.
pub(super) fn builtin_member_argument_type(
    member: BuiltinMember,
    index: usize,
    class_args: &[Type],
) -> Option<Type> {
    let list = |element: Type| Type::Named("list".to_string(), vec![element]);
    let bytes = || list(Type::named("uint8"));
    let headers = || {
        Type::Named(
            "dict".to_string(),
            vec![Type::named("str"), Type::named("str")],
        )
    };
    Some(match (member, index) {
        (BuiltinMember::StringJoin, 0) => list(Type::named("str")),
        (BuiltinMember::VecExtend, 0) => list(class_args.first()?.clone()),
        (BuiltinMember::MapExtend, 0) => Type::Named("dict".to_string(), class_args.to_vec()),
        (
            BuiltinMember::FileWriteBytes
            | BuiltinMember::TcpStreamWriteBytes
            | BuiltinMember::WebSocketSendBytes
            | BuiltinMember::ProcessPipeWriteBytes,
            0,
        )
        | (BuiltinMember::UdpSocketSendBytes | BuiltinMember::HttpExchangeRespondBytes, 1) => {
            bytes()
        }
        (BuiltinMember::HttpExchangeRespondText | BuiltinMember::HttpExchangeRespondBytes, 2)
        | (BuiltinMember::ProcessSupervisorStart, 3) => headers(),
        (BuiltinMember::ProcessSupervisorStart, 1) => list(Type::named("str")),
        _ => return None,
    })
}

#[cfg(test)]
#[path = "mir_local_types_tests.rs"]
mod tests;
