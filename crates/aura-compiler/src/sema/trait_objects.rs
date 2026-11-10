//! Trait objects (`dyn Trait`): a value of some type that implements one
//! trait, chosen at run time.
//!
//! A trait object type carries the trait's canonical name (see
//! `types::TraitTypeName`). The checker converts a value to a trait object
//! at a typed boundary, the same boundaries that inject a union member, and
//! records the conversion in the injection table so lowering does not decide
//! it again: the record's member type is the source type.
//!
//! A method is callable through a trait object when one call can serve
//! every implementing type: it takes a receiver, it has no type parameters
//! of its own, and `Self` appears nowhere else in its signature.
use super::traits::{resolved_trait_method, ResolvedTraitMethodInfo};
use super::*;

/// One trait a trait object exposes, with its type arguments.
pub(super) struct ExposedTrait<'a> {
    pub(super) info: &'a TraitInfo,
    pub(super) args: Vec<Type>,
}

/// Why a trait method cannot be called through a trait object.
fn dyn_call_obstacle(method: &TraitMethodInfo) -> Option<String> {
    if method.decl.receiver.is_none() {
        return Some(
            "it takes no `self`, so there is no value to choose an implementation from".to_string(),
        );
    }
    if !method.decl.type_params.is_empty() {
        return Some(
            "it has type parameters of its own, and a trait object carries one implementation per method"
                .to_string(),
        );
    }
    let mentions_self = |ty: &Type| {
        type_mentions_type_param(ty, "Self") || type_mentions_type_param_prefix(ty, "Self.")
    };
    if method.signature.params.iter().any(mentions_self)
        || mentions_self(&method.signature.return_type)
    {
        return Some(
            "its signature names `Self`, and the type behind a trait object is not known at the call"
                .to_string(),
        );
    }
    None
}

fn type_mentions_type_param_prefix(ty: &Type, prefix: &str) -> bool {
    match ty {
        Type::TypeParam(param) => param.starts_with(prefix),
        Type::Named(_, args) => args
            .iter()
            .any(|arg| type_mentions_type_param_prefix(arg, prefix)),
        Type::Tuple(elements) => elements
            .iter()
            .any(|element| type_mentions_type_param_prefix(element, prefix)),
        Type::Union(union) => union
            .members
            .iter()
            .any(|member| type_mentions_type_param_prefix(member, prefix)),
        Type::Function {
            params,
            return_type,
        } => {
            params
                .iter()
                .any(|param| type_mentions_type_param_prefix(&param.ty, prefix))
                || type_mentions_type_param_prefix(return_type, prefix)
        }
        Type::Closure {
            params,
            return_type,
            ..
        } => {
            params
                .iter()
                .any(|param| type_mentions_type_param_prefix(&param.ty, prefix))
                || type_mentions_type_param_prefix(return_type, prefix)
        }
        Type::Callable(callable) => {
            callable
                .params
                .iter()
                .any(|param| type_mentions_type_param_prefix(&param.ty, prefix))
                || type_mentions_type_param_prefix(&callable.return_type, prefix)
        }
        Type::ReturnedView(view) => type_mentions_type_param_prefix(&view.pointee, prefix),
        Type::Module(_) | Type::Unit => false,
    }
}

impl<'a> FunctionChecker<'a> {
    /// The trait a canonical trait name stands for: the bare name of a
    /// trait this module declares, or `module.Name`.
    pub(super) fn trait_info_by_canonical_name(&self, canonical: &str) -> Option<&TraitInfo> {
        let Some((module, leaf)) = canonical.rsplit_once('.') else {
            return self.traits.get(canonical);
        };
        let names_it = |info: &&TraitInfo| info.module_name == module && info.decl.name == leaf;
        self.traits
            .get(canonical)
            .filter(names_it)
            .or_else(|| self.traits.values().find(names_it))
            .or_else(|| {
                find_namespace_in_modules(self.module_registry, module).and_then(|namespace| {
                    namespace
                        .traits
                        .get(leaf)
                        .or_else(|| namespace.all_traits.get(leaf))
                        .filter(names_it)
                })
            })
    }

    /// The trait a bound names, where the bound was written in `module`:
    /// this module, or the module of an imported trait whose supertraits
    /// are being followed.
    fn trait_info_named_from(&self, module: &str, name: &str) -> Option<&TraitInfo> {
        if module == self.module_name || module.is_empty() {
            if let Some(info) = self.traits.get(name) {
                return Some(info);
            }
        }
        if name.contains('.') {
            if let Some(info) = self.trait_info_by_canonical_name(name) {
                return Some(info);
            }
        }
        find_namespace_in_modules(self.module_registry, module)
            .and_then(|namespace| namespace.all_traits.get(name))
            .or_else(|| self.trait_info_by_canonical_name(&format!("{module}.{name}")))
    }

    /// Every trait a trait object type exposes: its own trait first, then
    /// each supertrait, once.
    pub(super) fn trait_object_traits(&self, ty: &Type) -> Vec<ExposedTrait<'_>> {
        let Some((trait_name, trait_args)) = ty.as_trait_object() else {
            return Vec::new();
        };
        let Some(info) = self.trait_info_by_canonical_name(trait_name) else {
            return Vec::new();
        };
        let mut exposed = Vec::new();
        let mut seen = BTreeSet::new();
        let mut pending = vec![(info, trait_args.to_vec())];
        while let Some((info, args)) = pending.pop() {
            let key = format!(
                "{}.{}[{}]",
                info.module_name,
                info.decl.name,
                args.iter()
                    .map(Type::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if !seen.insert(key) {
                continue;
            }
            let substitutions = self_type_substitutions(&info.decl, &args, ty.clone());
            for supertrait in info.supertraits.iter().rev() {
                let resolved = substitute_trait_bound(supertrait, &substitutions);
                if let Some(parent) =
                    self.trait_info_named_from(&info.module_name, &resolved.trait_name)
                {
                    pending.push((parent, resolved.trait_args));
                }
            }
            exposed.push(ExposedTrait { info, args });
        }
        exposed
    }

    /// The method `field` a call on a trait object of type `ty` runs, as
    /// the trait declares it. `Ok(None)` when no exposed trait declares the
    /// method; an error when the method cannot be called through a trait
    /// object.
    pub(super) fn trait_object_method(
        &self,
        ty: &Type,
        field: &str,
        span: crate::diag::Span,
    ) -> Result<Option<ResolvedTraitMethodInfo>> {
        let mut found: Option<(&TraitInfo, ResolvedTraitMethodInfo)> = None;
        for exposed in self.trait_object_traits(ty) {
            let Some(method) = exposed.info.methods.get(field) else {
                continue;
            };
            if let Some((first, _)) = &found {
                return Err(Diagnostic::coded_at(
                    "AU2051",
                    span,
                    format!(
                        "method `{field}` is ambiguous on `{ty}`: traits `{}` and `{}` both declare it",
                        first.decl.name, exposed.info.decl.name
                    ),
                ));
            }
            if let Some(obstacle) = dyn_call_obstacle(method) {
                return Err(Diagnostic::coded_at(
                    "AU2051",
                    span,
                    format!(
                        "method `{field}` of trait `{}` cannot be called on `{ty}`: {obstacle}",
                        exposed.info.decl.name
                    ),
                )
                .with_help(
                    "call it on a value whose type is known, or through a type parameter bounded by the trait",
                ));
            }
            found = Some((
                exposed.info,
                resolved_trait_method(exposed.info, method, &exposed.args, ty),
            ));
        }
        Ok(found.map(|(_, method)| method))
    }

    /// The bound an exposed trait stands for, named as a trait object type
    /// names it.
    fn exposed_bound(&self, exposed: &ExposedTrait<'_>) -> TraitBound {
        let info = exposed.info;
        let trait_name = if info.module_name.is_empty() || info.module_name == self.module_name {
            info.decl.name.clone()
        } else {
            format!("{}.{}", info.module_name, info.decl.name)
        };
        TraitBound {
            trait_name,
            trait_args: exposed.args.clone(),
        }
    }

    /// Whether the trait object type `ty` exposes the trait `bound` names
    /// with the same arguments: its own trait or a supertrait.
    pub(super) fn trait_object_exposes(&self, ty: &Type, bound: &TraitBound) -> bool {
        let wanted = self.trait_bound_identity(bound, self.module_name);
        self.trait_object_traits(ty).iter().any(|exposed| {
            self.trait_bound_identity(&self.exposed_bound(exposed), self.module_name) == wanted
        })
    }

    /// Whether the trait object type `ty` can stand for a type parameter
    /// bounded by `bound`: it exposes the trait, and a call through the
    /// bound can reach every method of the trait and its supertraits.
    pub(super) fn trait_object_satisfies_bound(&self, ty: &Type, bound: &TraitBound) -> bool {
        if !self.trait_object_exposes(ty, bound) {
            return false;
        }
        let as_object = Type::trait_object(bound.trait_name.clone(), bound.trait_args.clone());
        self.trait_object_traits(&as_object).iter().all(|exposed| {
            exposed
                .info
                .methods
                .values()
                .all(|method| dyn_call_obstacle(method).is_none())
        })
    }

    /// Why the trait object type `ty`, which exposes the trait `bound`
    /// names, still cannot stand for a type parameter with that bound: a
    /// method the bound promises cannot be called through a trait object.
    pub(super) fn trait_object_bound_obstacle(
        &self,
        ty: &Type,
        bound: &TraitBound,
    ) -> Option<String> {
        if !ty.is_trait_object() || !self.trait_object_exposes(ty, bound) {
            return None;
        }
        let as_object = Type::trait_object(bound.trait_name.clone(), bound.trait_args.clone());
        self.trait_object_traits(&as_object)
            .iter()
            .find_map(|exposed| {
                exposed.info.methods.iter().find_map(|(name, method)| {
                    dyn_call_obstacle(method).map(|obstacle| {
                        format!(
                            "method `{name}` of trait `{}` cannot be called on a trait object, because {obstacle}",
                            exposed.info.decl.name
                        )
                    })
                })
            })
    }

    /// Whether a value of type `actual` converts to the trait object type
    /// `target` at a typed boundary: its type implements the trait, or it
    /// is a trait object of a trait that has the target's trait as a
    /// supertrait.
    pub(super) fn converts_to_trait_object(&self, actual: &Type, target: &Type) -> bool {
        let Some(bound) = target.trait_object_bound() else {
            return false;
        };
        if matches!(actual, Type::Function { .. } | Type::Closure { .. }) {
            return false;
        }
        if actual.is_trait_object() {
            return self.trait_object_exposes(actual, &bound);
        }
        self.type_implements_trait_bound(actual, &bound)
    }

    /// Types `expr` where a trait object of type `expected` is wanted. A
    /// value of a type that implements the trait converts, and the
    /// conversion is recorded for lowering.
    pub(super) fn type_trait_object_conversion(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        expected: &Type,
    ) -> Result<Type> {
        let actual = self.type_of_expr(expr, locals)?;
        if &actual == expected {
            return Ok(actual);
        }
        if !self.converts_to_trait_object(&actual, expected) {
            // The boundary reports the mismatch in its own words.
            return Ok(actual);
        }
        self.record_trait_object_conversion(expr, expected, actual);
        Ok(expected.clone())
    }

    /// Types `value as dyn Trait`: the conversion a typed boundary makes,
    /// written where no boundary supplies the type.
    pub(super) fn type_trait_object_cast(
        &self,
        expr: &Expr,
        value: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        target: &Type,
    ) -> Result<Type> {
        let actual = self.type_trait_object_conversion(value, locals, target)?;
        if &actual != target {
            return Err(self.with_trait_object_mismatch_help(
                Diagnostic::coded_at(
                    "AU2002",
                    expr.span,
                    format!("cannot convert `{actual}` to `{target}`"),
                ),
                target,
                &actual,
            ));
        }
        Ok(actual)
    }

    /// Rejects `value as dyn Trait` where a `mut` receiver or parameter
    /// needs a place: the conversion makes a new value, so a change made
    /// through it would not reach `value`.
    pub(super) fn reject_mutable_trait_object_cast(&self, expr: &Expr) -> Result<()> {
        let mut selected = expr;
        while let ExprKind::Group(inner) = &selected.kind {
            selected = inner;
        }
        let ExprKind::Cast { ty: target, .. } = &selected.kind else {
            return Ok(());
        };
        if target.as_trait_object().is_none() {
            return Ok(());
        }
        Err(Diagnostic::coded_at(
            "AU2051",
            selected.span,
            "a conversion to a trait object makes a new value, so it cannot be changed in place",
        )
        .with_help("bind the converted value to a `mut` name first, and use that"))
    }

    pub(super) fn record_trait_object_conversion(
        &self,
        expr: &Expr,
        expected: &Type,
        actual: Type,
    ) {
        let id = UnionInjectionId {
            module_name: self.module_name.to_owned(),
            line: expr.span.line,
            column: expr.span.column,
            expected_key: expected.canonical_key(self.module_name, self.canonical_type_names),
        };
        self.union_injections.borrow_mut().insert(
            id,
            UnionInjection {
                union_type: expected.clone(),
                member_type: actual,
                member_index: 0,
            },
        );
    }

    /// The error for a call of `field` on a trait object when no exposed
    /// trait declares that method.
    pub(super) fn trait_object_missing_method(
        &self,
        ty: &Type,
        field: &str,
        span: crate::diag::Span,
    ) -> Diagnostic {
        if field == "clone" {
            return Diagnostic::coded_at(
                "AU3007",
                span,
                format!("cannot clone `{ty}` because a trait object cannot be cloned"),
            )
            .with_note(
                "the type behind a trait object is not known here, so nothing says a copy of it is safe",
            )
            .with_help(
                "borrow it with `view`, or move it; to copy on purpose, add a method to the trait that returns a new value",
            );
        }
        let exposed = self.trait_object_traits(ty);
        let methods = exposed
            .iter()
            .flat_map(|exposed| exposed.info.methods.keys())
            .map(|name| format!("`{name}`"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let diagnostic = Diagnostic::coded_at(
            "AU2001",
            span,
            format!("`{ty}` has no method `{field}`"),
        )
        .with_note(
            "a trait object exposes the methods of its trait, not the methods of the type it holds",
        );
        if methods.is_empty() {
            diagnostic
        } else {
            diagnostic.with_help(format!("its methods are {}", methods.join(", ")))
        }
    }

    /// The error for a read of field `field` on a trait object.
    pub(super) fn trait_object_missing_field(
        &self,
        ty: &Type,
        field: &str,
        span: crate::diag::Span,
    ) -> Diagnostic {
        Diagnostic::coded_at(
            "AU2002",
            span,
            format!("`{ty}` has no field `{field}`"),
        )
        .with_note(
            "a trait object exposes the methods of its trait, not the fields of the type it holds",
        )
        .with_help("add a method to the trait that returns the value")
    }

    /// Adds, to a type mismatch, what a reader who expected a trait object
    /// conversion needs to know.
    pub(super) fn with_trait_object_mismatch_help(
        &self,
        diagnostic: Diagnostic,
        expected: &Type,
        actual: &Type,
    ) -> Diagnostic {
        match self.trait_object_mismatch_help(expected, actual) {
            Some(help) if diagnostic.help.is_empty() => diagnostic.with_help(help),
            _ => diagnostic,
        }
    }

    fn trait_object_mismatch_help(&self, expected: &Type, actual: &Type) -> Option<String> {
        if let Some((trait_name, _)) = expected.as_trait_object() {
            let trait_leaf = trait_name.rsplit('.').next().unwrap_or(trait_name);
            if actual.is_trait_object() {
                return Some(format!(
                    "`{actual}` converts to `{expected}` only when `{trait_leaf}` is its trait or one of its supertraits"
                ));
            }
            if matches!(actual, Type::Function { .. } | Type::Closure { .. }) {
                return Some(format!(
                    "a function value does not implement a trait; put it in a class that implements `{trait_leaf}`"
                ));
            }
            if actual.names_trait_object() || matches!(actual, Type::Union(_)) {
                return None;
            }
            return Some(format!(
                "`{actual}` does not implement trait `{trait_leaf}`; add `impl {trait_leaf} for {actual}:`"
            ));
        }
        if actual.is_trait_object() && !expected.names_trait_object() {
            return Some(format!(
                "a trait object does not convert back to the type it holds; call the methods of its trait on `{actual}`"
            ));
        }
        if expected.names_trait_object()
            && !actual.names_trait_object()
            && !matches!(expected, Type::Union(_))
        {
            return Some(format!(
                "a `{actual}` is not a `{expected}`: a value converts to a trait object one at a time, where it is stored; build the collection with the annotation `{expected}`"
            ));
        }
        None
    }
}

#[cfg(test)]
#[path = "trait_objects_tests.rs"]
mod tests;
