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
                trait_args: bound.trait_args,
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

impl MirTraitImpl {
    /// Match the type arguments selected by a delegated call using the
    /// substitutions already inferred from the receiver.
    pub(crate) fn matches_trait_args(
        &self,
        args: &[Type],
        substitutions: &HashMap<String, Type>,
    ) -> bool {
        if args.is_empty() {
            return true;
        }
        if self.trait_args.len() != args.len() {
            return false;
        }
        let mut params = BTreeSet::new();
        for ty in &self.trait_args {
            collect_type_params_from_type(ty, &mut params);
        }
        let mut matched = substitutions.clone();
        self.trait_args.iter().zip(args).all(|(pattern, arg)| {
            if crate::sema::has_unresolved_type_params(arg) {
                let mut open = BTreeSet::new();
                collect_type_params_from_type(arg, &mut open);
                let implemented = substitute_type(pattern, &matched);
                if crate::sema::type_pattern_matches(arg, &implemented, &open, &mut HashMap::new())
                {
                    return true;
                }
            }
            crate::sema::type_pattern_matches(pattern, arg, &params, &mut matched)
        })
    }
}

impl Lowerer<'_> {
    /// Follow receiverless forwarders after substitution. Each step selects a
    /// field by its concrete class layout and checks its exact trait bound.
    /// The leaf names an existing implementation function on both back ends.
    pub(super) fn associated_alias<'m>(
        &'m self,
        mut owner: &'m crate::sema::TraitImplInfo,
        method: &crate::sema::TraitImplMethodInfo,
        actual: Type,
    ) -> Option<String> {
        let name = &method.decl.name;
        let mut actual = actual;
        let mut seen = BTreeSet::new();
        loop {
            let method = owner.methods.get(name)?;
            if !crate::sema::delegation::deferred_associated(owner, method) {
                return Some(format!(
                    "{}{} for {}.{}",
                    owner.trait_name,
                    format_trait_args(&owner.trait_args),
                    owner.for_type,
                    name
                ));
            }
            if !seen.insert((
                owner.module_name.clone(),
                owner.trait_name.clone(),
                actual.to_string(),
            )) {
                return None;
            }
            let field = &owner.decl.delegation.as_ref()?.0;
            let Type::Named(class_name, arguments) = &actual else {
                return None;
            };
            let class = self.resolve_class_info(class_name)?;
            let field_ty = substitute_type(
                &class.fields.get(field)?.ty,
                &crate::sema::substitutions_from_decl_type_args(&class.decl.type_params, arguments),
            );
            let substitutions = self.trait_impl_substitutions(owner, &actual)?;
            let bound = TraitBound {
                trait_name: owner.trait_name.clone(),
                trait_args: owner
                    .trait_args
                    .iter()
                    .map(|arg| substitute_type(arg, &substitutions))
                    .collect(),
            };
            owner = self
                .trait_impls_in_scope()
                .filter(|candidate| {
                    self.trait_impl_substitutions_for_bound(candidate, &field_ty, &bound)
                        .is_some()
                })
                .max_by_key(|candidate| crate::sema::trait_impl_specificity(candidate))?;
            actual = field_ty;
        }
    }
}

pub(super) fn associated_alias_for_bound(
    program: &Program,
    trait_name: &str,
    ty: &Type,
    name: &str,
) -> Option<String> {
    let mut candidates = program.trait_impls.iter().chain(
        program
            .module_registry
            .values()
            .flat_map(|namespace| &namespace.trait_impls),
    );
    if !candidates.any(|owner| {
        owner.decl.delegation.is_some()
            && owner
                .methods
                .get(name)
                .is_some_and(|method| crate::sema::delegation::deferred_associated(owner, method))
    }) {
        return None;
    }
    let resolver = Lowerer::new(
        program,
        "",
        &program.module_name,
        Type::Unit,
        BTreeMap::new(),
    );
    let owner = resolver
        .trait_impls_in_scope()
        .filter(|candidate| {
            runtime_trait_name(program, &candidate.module_name, &candidate.trait_name) == trait_name
                && resolver.trait_impl_substitutions(candidate, ty).is_some()
        })
        .max_by_key(|candidate| crate::sema::trait_impl_specificity(candidate))?;
    let method = owner.methods.get(name)?;
    crate::sema::delegation::deferred_associated(owner, method)
        .then(|| resolver.associated_alias(owner, method, ty.clone()))
        .flatten()
}
