//! A delegated implementation is an ordinary implementation whose missing
//! methods call the selected field. Build real parent implementations too:
//! a bound on a parent, or a trait-object upcast, uses that parent's table.

use super::{
    BTreeMap, BTreeSet, ClassInfo, Diagnostic, Expr, ExprKind, FunctionChecker, HashMap, ImplDecl,
    Item, Module, Result, Stmt, TraitBound, TraitInfo, Type, TypeRef, TypeSourceRef,
};
use crate::ast::{Argument, ExprStmt, ReturnStmt, ViewKind};
use crate::diag::{Span, GENERATED_COLUMN_STRIDE};

pub(super) fn view_field_error(module: &Module, class: &str, field: &str) -> Option<Diagnostic> {
    module.items.iter().find_map(|item| {
        let Item::Impl(decl) = item else { return None };
        let (name, span) = decl.delegation.as_ref()?;
        (name == field && decl.for_type.named_parts()?.0 == class).then(|| {
            Diagnostic::coded_at(
                "AU2063",
                *span,
                format!("cannot delegate to view field `{field}`"),
            )
        })
    })
}

/// Add impls for parents missing from a delegation's target. Keep the
/// trait identity in the declaring module through each step of the walk.
pub(super) fn expand_parents(
    module: &mut Module,
    traits: &BTreeMap<String, TraitInfo>,
    current_module: &str,
    lower: impl Fn(&TypeRef, &ImplDecl) -> Result<Type>,
) -> Result<()> {
    let mut pending = module
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            Item::Impl(decl) if decl.delegation.is_some() => Some(index),
            _ => None,
        })
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(());
    }
    let key = |decl: &ImplDecl| -> Result<_> {
        let name = traits.get(&decl.trait_name).map_or_else(
            || decl.trait_name.clone(),
            |info| format!("{}.{}", info.module_name, info.decl.name),
        );
        let args = decl
            .trait_args
            .iter()
            .map(|arg| lower(arg, decl))
            .collect::<Result<Vec<_>>>()?;
        let target = lower(&decl.for_type, decl)?;
        let mut parameters = BTreeSet::new();
        super::collect_type_params_from_type(&target, &mut parameters);
        for arg in &args {
            super::collect_type_params_from_type(arg, &mut parameters);
        }
        let mut order = decl.type_params.clone();
        order.extend(
            parameters
                .into_iter()
                .filter(|name| !decl.type_params.contains(name)),
        );
        let normalized = order
            .into_iter()
            .enumerate()
            .map(|(index, name)| (name, Type::TypeParam(format!("$impl{index}"))))
            .collect();
        let key_type = |ty: &Type| {
            super::substitute_type(ty, &normalized).canonical_key(current_module, &BTreeMap::new())
        };
        Ok((key_type(&Type::Named(name, args)), key_type(&target)))
    };
    let mut positions = HashMap::new();
    for (index, item) in module.items.iter().enumerate() {
        if let Item::Impl(decl) = item {
            let id = key(decl)?;
            let previous = positions.insert(id, index);
            if let (Some((_, span)), Some(previous)) = (&decl.delegation, previous) {
                if matches!(&module.items[previous], Item::Impl(other) if other.delegation.is_some())
                {
                    return Err(Diagnostic::coded_at(
                        "AU2062",
                        *span,
                        format!(
                            "trait `{}` is delegated twice for `{}`",
                            decl.trait_name,
                            lower(&decl.for_type, decl)?
                        ),
                    ));
                }
            }
        }
    }

    // Work on one root at a time so a method written inside its block can
    // override a method declared several levels up the parent chain.
    for root in std::mem::take(&mut pending) {
        let Item::Impl(root_decl) = &module.items[root] else {
            unreachable!()
        };
        let overrides = root_decl.methods.clone();
        let mut used = BTreeSet::new();
        let mut work = vec![root];
        while let Some(index) = work.pop() {
            let Item::Impl(decl) = &module.items[index] else {
                unreachable!()
            };
            let Some(info) = traits.get(&decl.trait_name) else {
                continue;
            };
            let mut decl = decl.clone();
            decl.methods = overrides
                .iter()
                .filter(|method| info.methods.contains_key(&method.name))
                .cloned()
                .collect();
            used.extend(decl.methods.iter().map(|method| method.name.clone()));
            let substitutions = info
                .decl
                .type_params
                .iter()
                .cloned()
                .zip(decl.trait_args.iter().cloned())
                .collect();
            for parent in &info.decl.supertraits {
                let Some((name, args)) = parent.named_parts() else {
                    continue;
                };
                let qualified = format!("{}.{}", info.module_name, name);
                let name = if info.module_name != current_module && traits.contains_key(&qualified)
                {
                    &qualified
                } else {
                    name
                };
                let Some(parent_info) = traits.get(name) else {
                    continue;
                };
                if super::trait_objects::is_transfer_marker(parent_info) {
                    continue;
                }
                let written =
                    parent_info.decl.type_params.len() - parent_info.decl.assoc_types.len();
                let mut inherited = decl.clone();
                inherited.trait_name = name.to_owned();
                inherited.trait_args = args
                    .iter()
                    .take(written)
                    .map(|arg| super::assoc_bounds::substitute(arg, &substitutions, decl.span))
                    .collect();
                inherited.assoc_types.retain(|binding| {
                    parent_info
                        .decl
                        .assoc_types
                        .iter()
                        .any(|assoc| assoc.name == binding.name)
                });
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    positions.entry(key(&inherited)?)
                {
                    let next = module.items.len();
                    entry.insert(next);
                    module.items.push(Item::Impl(inherited));
                    work.push(next);
                }
            }
            module.items[index] = Item::Impl(decl);
        }
        if let Item::Impl(decl) = &mut module.items[root] {
            // Let ordinary impl checking report an unknown override.
            decl.methods.extend(
                overrides
                    .into_iter()
                    .filter(|method| !used.contains(&method.name)),
            );
        }
    }
    Ok(())
}

/// The field's associated bindings must already be available. Stable order
/// retains the ordinary parent-before-child ordering within each layer.
pub(super) fn order_after_field_impls(
    decls: &mut Vec<&ImplDecl>,
    classes: &BTreeMap<String, ClassInfo>,
    traits: &BTreeMap<String, TraitInfo>,
) {
    if !decls.iter().any(|decl| decl.delegation.is_some()) {
        return;
    }
    let key = |class: &str, trait_name: &str| {
        let identity = traits.get(trait_name).map_or_else(
            || trait_name.to_owned(),
            |info| format!("{}.{}", info.module_name, info.decl.name),
        );
        (class.to_owned(), identity)
    };
    let mut ready = Vec::new();
    let mut pending = std::mem::take(decls);
    while !pending.is_empty() {
        let waiting = pending
            .iter()
            .filter_map(|decl| {
                decl.for_type
                    .named_parts()
                    .map(|(name, _)| key(name, &decl.trait_name))
            })
            .collect::<BTreeSet<_>>();
        let (next, rest): (Vec<_>, Vec<_>) = pending.into_iter().partition(|decl| {
            let Some((name, args)) = decl.for_type.named_parts() else {
                return true;
            };
            if let Some(info) = traits.get(&decl.trait_name) {
                if info
                    .supertraits
                    .iter()
                    .any(|parent| waiting.contains(&key(name, &parent.trait_name)))
                {
                    return false;
                }
            }
            let Some((field, _)) = &decl.delegation else {
                return true;
            };
            let Some(class) = classes.get(name) else {
                return true;
            };
            let field_class = class.fields.get(field).and_then(|field| match &field.ty {
                Type::Named(name, _) => Some(name.as_str()),
                Type::TypeParam(param) => class
                    .decl
                    .type_params
                    .iter()
                    .position(|name| name == param)
                    .and_then(|index| args.get(index)?.named_parts().map(|(name, _)| name)),
                _ => None,
            });
            !field_class.is_some_and(|name| waiting.contains(&key(name, &decl.trait_name)))
        });
        if next.is_empty() {
            ready.extend(rest);
            break;
        }
        ready.extend(next);
        pending = rest;
    }
    *decls = ready;
}

pub(super) fn elaborate(
    checker: &FunctionChecker<'_>,
    decl: &mut ImplDecl,
    info: &TraitInfo,
    target: &Type,
    args: &[Type],
    open: &[String],
    serial: &mut usize,
) -> Result<HashMap<String, Type>> {
    let (name, span) = decl.delegation.as_ref().unwrap();
    let field = checker
        .class_field_types(target)
        .into_iter()
        .flatten()
        .find(|(field, _)| field == name)
        .map(|(_, ty)| ty)
        .ok_or_else(|| {
            Diagnostic::coded_at(
                "AU2060",
                *span,
                format!("class `{target}` has no field `{name}`"),
            )
        })?;
    let mut bound = TraitBound {
        trait_name: decl.trait_name.clone(),
        trait_args: args.to_vec(),
    };
    let mut found = HashMap::new();
    if !open.is_empty() {
        let receiver = "$delegated".to_owned();
        found.insert(receiver.clone(), field.clone());
        checker.infer_bound_only_type_args(
            open,
            &BTreeMap::from([(receiver, vec![bound.clone()])]),
            &mut found,
        );
        bound = super::substitute_trait_bound(&bound, &found);
    }
    if !checker.type_implements_trait_bound(&field, &bound) {
        return Err(Diagnostic::coded_at(
            "AU2061",
            *span,
            format!(
                "field `{name}` of type `{field}` does not implement `{}`",
                decl.trait_name
            ),
        ));
    }
    add_forwarders(decl, info, &bound.trait_args, target, &field, serial)?;
    Ok(found)
}

fn add_forwarders(
    decl: &mut ImplDecl,
    info: &TraitInfo,
    args: &[Type],
    target: &Type,
    field_type: &Type,
    serial: &mut usize,
) -> Result<()> {
    let Some((field, position)) = &decl.delegation else {
        return Ok(());
    };
    let mut fresh = || {
        *serial += 1;
        Span::new(
            position.line,
            position.column + *serial * GENERATED_COLUMN_STRIDE,
        )
    };
    for (name, method) in &info.methods {
        if decl.methods.iter().any(|existing| existing.name == *name) {
            continue;
        }
        let resolved = super::traits::resolved_trait_method(info, method, args, target);
        let mut forwarder = resolved.decl;
        forwarder.span = fresh();
        for (param, ty) in forwarder.params.iter_mut().zip(&resolved.signature.params) {
            param.ty = ty.source_type_ref(param.span)?;
        }
        forwarder.return_type = resolved
            .signature
            .return_type
            .source_type_ref(forwarder.span)?;
        let receiver = if forwarder.receiver.is_some() {
            Expr {
                kind: ExprKind::Member {
                    object: Box::new(Expr {
                        kind: ExprKind::Name("self".to_string()),
                        span: fresh(),
                    }),
                    field: field.clone(),
                },
                span: fresh(),
            }
        } else {
            type_receiver(field_type, fresh())?
        };
        let mut callee = Expr {
            kind: ExprKind::Member {
                object: Box::new(receiver),
                field: name.clone(),
            },
            span: fresh(),
        };
        if !forwarder.type_params.is_empty() {
            callee = Expr {
                kind: ExprKind::Specialize {
                    expr: Box::new(callee),
                    type_args: forwarder
                        .type_params
                        .iter()
                        .map(|name| TypeRef::named(name, vec![], false, fresh()))
                        .collect(),
                },
                span: fresh(),
            };
        }
        let args = forwarder
            .params
            .iter()
            .map(|param| Argument {
                name: param.keyword_only.then(|| param.name.clone()),
                value: Expr {
                    kind: ExprKind::Name(param.name.clone()),
                    span: fresh(),
                },
                span: fresh(),
            })
            .collect();
        let call = Expr {
            kind: ExprKind::Call {
                callee: Box::new(callee),
                args,
            },
            span: fresh(),
        };
        forwarder.body = vec![if resolved.signature.return_type == Type::Unit {
            Stmt::Expr(ExprStmt {
                expr: call,
                span: fresh(),
                discarded: false,
            })
        } else {
            Stmt::Return(ReturnStmt {
                value: Some(call),
                view: forwarder.view_return.as_ref().map(|view| {
                    if view.mutable {
                        ViewKind::Mutable
                    } else {
                        ViewKind::Shared
                    }
                }),
                span: fresh(),
            })
        }];
        decl.methods.push(forwarder);
    }
    Ok(())
}

fn type_receiver(ty: &Type, span: Span) -> Result<Expr> {
    let reference = ty.source_type_ref(span)?;
    let (path, args) = reference.named_parts().ok_or_else(|| {
        Diagnostic::at(span, "an associated method requires a named delegate type")
    })?;
    let mut parts = path.split('.');
    let mut expr = Expr {
        kind: ExprKind::Name(parts.next().unwrap().to_owned()),
        span,
    };
    for field in parts {
        expr = Expr {
            kind: ExprKind::Member {
                object: Box::new(expr),
                field: field.to_owned(),
            },
            span,
        };
    }
    if !args.is_empty() {
        expr = Expr {
            kind: ExprKind::Specialize {
                expr: Box::new(expr),
                type_args: args.to_vec(),
            },
            span,
        };
    }
    Ok(expr)
}

/// Locate the method that owns this synthesized call. Ordinary source calls
/// never scan implementations. The module distinguishes identical positions
/// in separate files.
pub(crate) fn generated_call<'a>(
    implementations: impl Iterator<Item = &'a super::TraitImplInfo>,
    module: &str,
    span: Span,
) -> Option<(&'a super::TraitImplInfo, &'a super::TraitImplMethodInfo)> {
    if span.column < GENERATED_COLUMN_STRIDE {
        return None;
    }
    implementations.filter(|implementation| implementation.module_name == module && implementation.decl.delegation.is_some())
        .find_map(|implementation| {
            implementation.methods.values().find(|method| {
                if method.decl.span.column < GENERATED_COLUMN_STRIDE { return false }
                let call = match method.decl.body.as_slice() {
                    [Stmt::Return(stmt)] => stmt.value.as_ref(),
                    [Stmt::Expr(stmt)] => Some(&stmt.expr),
                    _ => None,
                };
                call.is_some_and(|value| value.span == span || matches!(&value.kind, ExprKind::Call { callee, .. } if callee.span == span))

            }).map(|method| (implementation, method))
        })
}

pub(crate) fn forwarded_receiver(callee: &Expr) -> Option<&Expr> {
    let member = match &callee.kind {
        ExprKind::Specialize { expr, .. } => expr.as_ref(),
        _ => callee,
    };
    match &member.kind {
        ExprKind::Member { object, .. } => Some(object),
        _ => None,
    }
}

impl FunctionChecker<'_> {
    pub(super) fn check_delegated_call(
        &self,
        callee: &Expr,
        args: &[Argument],
        span: Span,
        locals: &mut HashMap<String, super::LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Option<Type>> {
        let Some((implementation, method)) =
            generated_call(self.trait_impls.iter(), self.module_name, span)
        else {
            return Ok(None);
        };
        let Some(receiver) = forwarded_receiver(callee) else {
            return Ok(None);
        };
        let info = &self.traits[&implementation.trait_name];
        let field = &implementation.decl.delegation.as_ref().unwrap().0;
        let source = self
            .class_field_types(&implementation.for_type)
            .unwrap()
            .into_iter()
            .find(|(name, _)| name == field)
            .unwrap()
            .1;
        let name = &method.decl.name;
        let resolved = super::traits::resolved_trait_method(
            info,
            &info.methods[name],
            &implementation.trait_args,
            &source,
        );
        let borrows = if method.decl.receiver.is_some() {
            self.type_of_member_object_expr(receiver, locals)?;
            self.prepare_method_receiver_borrows(
                name,
                method.decl.receiver,
                receiver,
                span,
                locals,
            )?
        } else {
            Vec::new()
        };
        let signature = &resolved.signature;
        let seed = resolved
            .decl
            .type_params
            .iter()
            .map(|param| (param.clone(), Type::TypeParam(param.clone())))
            .collect();
        self.type_check_callable_args_seeded(
            name,
            &resolved.decl.type_params,
            &resolved.decl.params,
            &signature.param_passings,
            &signature.params,
            &signature.return_type,
            &resolved.type_param_bounds,
            &signature.rng_clone_safe_type_params,
            &signature.array_equality_safe_type_params,
            args,
            span,
            locals,
            expected,
            seed,
            borrows,
            super::ClosureArgumentPolicy::Reject,
        )
        .map(|checked| Some(checked.return_type))
    }
}

#[cfg(test)]
#[path = "delegation_tests.rs"]
mod tests;

/// An associated method of a generic delegation has no receiver at run
/// time to identify the field's implementation. Resolve this method when
/// the caller supplies the class type arguments.
pub(crate) fn deferred_associated(
    owner: &super::TraitImplInfo,
    method: &super::TraitImplMethodInfo,
) -> bool {
    owner.decl.delegation.is_some()
        && !owner.type_params.is_empty()
        && method.decl.receiver.is_none()
        && method.decl.span.column >= GENERATED_COLUMN_STRIDE
}

impl FunctionChecker<'_> {
    /// `Wrapper[T].make()` forwarded to a field of type `T` in a generic
    /// function has the same specialization requirement as `T.make()`.
    pub(super) fn associated_delegate_param(
        &self,
        object: &Expr,
        field: &str,
        locals: &HashMap<String, super::LocalBinding>,
    ) -> Option<String> {
        let (class, type_args) = self.peel_specialization(object);
        let ExprKind::Name(name) = &class.kind else {
            return None;
        };
        let type_args = type_args?;
        if locals.contains_key(name) || self.resolve_class_info(name)?.methods.contains_key(field) {
            return None;
        }
        let actual = Type::Named(name.clone(), self.lower_explicit_type_args(type_args).ok()?);
        let (owner, method, _) = self
            .trait_method_for_concrete_type(&actual, field, object.span)
            .ok()??;
        if !deferred_associated(owner, method) {
            return None;
        }
        let stored = &owner.decl.delegation.as_ref()?.0;
        self.class_field_types(&actual)?
            .into_iter()
            .find_map(|(name, ty)| match ty {
                Type::TypeParam(param) if &name == stored => Some(param),
                _ => None,
            })
    }
}
