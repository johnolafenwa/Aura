//! Calls synthesized for delegation name their trait even when an inherent
//! method has the same spelling. A concrete delegate calls the impl function
//! directly. A receiver that is itself a type parameter uses its bound.

use super::*;
use crate::sema::delegation::{forwarded_receiver, generated_call};

impl Lowerer<'_> {
    pub(super) fn lower_delegated_call(
        &mut self,
        expr: &Expr,
        callee: &Expr,
        args: &[Argument],
    ) -> Option<Operand> {
        let (implementation, method) =
            generated_call(self.trait_impls_in_scope(), self.module_name, expr.span)?;
        let receiver = forwarded_receiver(callee)?;
        let bound = TraitBound {
            trait_name: implementation.trait_name.clone(),
            trait_args: implementation.trait_args.clone(),
        };
        let field = &implementation.decl.delegation.as_ref()?.0;
        let Type::Named(class_name, args_of_class) = &implementation.for_type else {
            return None;
        };
        let class = self.resolve_class_info(class_name)?;
        let ty = substitute_type(
            &class.fields.get(field)?.ty,
            &crate::sema::substitutions_from_decl_type_args(&class.decl.type_params, args_of_class),
        );
        let method = method.clone();
        let trait_name =
            runtime_trait_name(self.program, &implementation.module_name, &bound.trait_name);
        let direct = self
            .trait_impls_in_scope()
            .filter(|candidate| {
                self.trait_impl_substitutions_for_bound(candidate, &ty, &bound)
                    .is_some()
            })
            .max_by_key(|candidate| crate::sema::trait_impl_specificity(candidate))
            .map(|candidate| {
                format!(
                    "{}{} for {}.{}",
                    candidate.trait_name,
                    format_trait_args(&candidate.trait_args),
                    candidate.for_type,
                    method.decl.name
                )
            });
        let object = method
            .decl
            .receiver
            .map(|passing| self.lower_expr_for_passing(receiver, Some(&ty), passing));
        let arguments = self.lower_user_args_with_types(
            &method.decl.name,
            &method.decl.params,
            args,
            callee.span,
            Some(&method.signature.params),
            Some(&method.signature.param_passings),
        );
        let target = if let Some(object) = object {
            let receiver_place = self.lowered_writeback_place(receiver, &object);
            CallTarget::TraitMember {
                object,
                trait_name,
                field: method.decl.name,
                receiver_place,
                dispatch: direct.map(|function| MethodDispatch::Static { function }),
            }
        } else {
            CallTarget::Name(direct?)
        };
        let temp = self.new_typed_temp(method.signature.return_type);
        self.emit(Instruction::Assign {
            target: temp.clone(),
            value: Rvalue::Call {
                callee: target,
                args: arguments,
                type_args: Default::default(),
            },
        });
        Some(Operand::Place(temp))
    }
}

/// The forwarding method's own impl fixes the specialization of a member
/// with several implementations on its receiver (e.g. Scale[str] and
/// Scale[int64]). Reconstruct the target from the MIR declarations when
/// selection by the trait's name alone is ambiguous.
pub(super) fn specialized_dispatch(
    context: &MirLoanValidationContext<'_>, caller: &MirFunction, receiver: &Type, parts: &MemberCallParts<'_>,
) -> Option<MethodDispatch> {
    let trait_name = parts.trait_name?;
    let owner = context.trait_impls.iter().find(|implementation| {
        implementation.trait_name == trait_name && implementation.methods.iter().any(|method|
            method.name == parts.field && method.function_name == caller.name)
    })?;
    let bound_args = &owner.trait_args;
    let mut candidates = context.trait_impls.iter().filter_map(|implementation| {
        if implementation.trait_name != trait_name || implementation.trait_args.len() != bound_args.len() {
            return None;
        }
        let mut params = BTreeSet::new();
        collect_type_params_from_type(&implementation.for_type, &mut params);
        for arg in &implementation.trait_args { collect_type_params_from_type(arg, &mut params); }
        let mut substitutions = HashMap::new();
        if !crate::sema::type_pattern_matches(&implementation.for_type, receiver, &params, &mut substitutions) ||
            !implementation.trait_args.iter().zip(bound_args).all(|(pattern, actual)| crate::sema::type_pattern_matches(pattern, actual, &params, &mut substitutions)) { return None }
        let method = implementation.methods.iter().find(|method| method.name == parts.field)?;
        if method.receiver == Some(MirReceiverKind::BorrowMut) && parts.receiver_place.is_none() { return None }
        Some(method)
    });
    let method = candidates.next()?;
    if candidates.next().is_some() { return None }
    Some(MethodDispatch::Static { function: method.function_name.clone() })
}
