//! Trait contracts, applicability, bounds, and method resolution.

use super::{
    collect_type_params_from_type, is_builtin_type, lower_type_with_self, merged_type_param_scope,
    preserves_qualified_builtin_type_name, substitute_trait_bound, substitute_trait_bounds,
    substitute_type, trait_impl_specificity, type_param_scope, type_pattern_matches, BTreeMap,
    BTreeSet, BinaryOp, BlockFlow, BuiltinMember, ClosureOwner, Diagnostic, FunctionChecker,
    FunctionDecl, FunctionSignature, HashMap, ImplDecl, LocalBinding, ReceiverKind, Result,
    TraitBound, TraitDecl, TraitImplInfo, TraitImplMethodInfo, TraitInfo, TraitMethodInfo, Type,
    TypeRef, UnaryOp,
};

pub(super) type TraitMethodMatch<'a> = (
    &'a TraitImplInfo,
    &'a TraitImplMethodInfo,
    HashMap<String, Type>,
);

pub(crate) fn unary_operator_trait(op: UnaryOp) -> Option<(&'static str, &'static str)> {
    match op {
        UnaryOp::Neg => Some(("Neg", "neg")),
        UnaryOp::Not => Some(("Not", "not")),
        UnaryOp::BitNot => None,
    }
}

pub(crate) fn binary_operator_trait(op: BinaryOp) -> Option<(&'static str, &'static str)> {
    match op {
        BinaryOp::Add => Some(("Add", "add")),
        BinaryOp::Sub => Some(("Sub", "sub")),
        BinaryOp::Mul => Some(("Mul", "mul")),
        BinaryOp::Div => Some(("Div", "div")),
        BinaryOp::FloorDiv => Some(("FloorDiv", "floor_div")),
        BinaryOp::Mod => Some(("Mod", "mod")),
        BinaryOp::Less => Some(("Ord", "lt")),
        BinaryOp::LessEq => Some(("Ord", "le")),
        BinaryOp::Greater => Some(("Ord", "gt")),
        BinaryOp::GreaterEq => Some(("Ord", "ge")),
        BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Pow
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr => None,
    }
}

/// Rejects a trait implementation that would shadow a builtin method of its
/// target. The rule covers every builtin target, not only the runtime handles:
/// a shadowed builtin name is silently ignored at every call site, so the
/// program does something other than what its source says.
pub(super) fn reject_builtin_trait_method_collisions(
    impl_decl: &ImplDecl,
    for_type: &Type,
    methods: &BTreeMap<String, TraitImplMethodInfo>,
) -> Result<()> {
    let Type::Named(target_name, _) = for_type else {
        return Ok(());
    };
    if !is_builtin_type(target_name) && !preserves_qualified_builtin_type_name(target_name) {
        return Ok(());
    }

    for (method_name, method) in methods {
        if BuiltinMember::resolve(target_name, method_name).is_none() {
            continue;
        }

        let explicit_method = impl_decl
            .methods
            .iter()
            .find(|candidate| candidate.name.as_str() == method_name.as_str());
        let primary_span = explicit_method.map_or(impl_decl.span, |method| method.span);
        let mut diagnostic = Diagnostic::coded_at(
            "AU2006",
            primary_span,
            format!(
                "trait method `{method_name}` collides with builtin method \
                 `{target_name}.{method_name}`"
            ),
        )
        .with_help(
            "rename the trait method; builtin methods cannot be shadowed by trait implementations",
        );
        if explicit_method.is_none() {
            diagnostic = diagnostic.with_secondary(
                method.decl.span,
                "colliding default trait method is declared here",
            );
        }
        return Err(diagnostic);
    }

    Ok(())
}

pub(crate) fn self_type_substitutions(
    trait_decl: &TraitDecl,
    trait_args: &[Type],
    self_ty: Type,
) -> HashMap<String, Type> {
    let mut substitutions = trait_decl
        .type_params
        .iter()
        .cloned()
        .zip(trait_args.iter().cloned())
        .collect::<HashMap<_, _>>();
    substitutions.insert("Self".to_string(), self_ty);
    substitutions
}

pub(super) fn lower_trait_bounds(
    bounds: &BTreeMap<String, Vec<TypeRef>>,
    traits: &BTreeMap<String, TraitInfo>,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_param_scope: &BTreeMap<String, ()>,
) -> Result<BTreeMap<String, Vec<TraitBound>>> {
    lower_trait_bounds_with_self(
        bounds,
        traits,
        type_names,
        type_arities,
        canonical_type_names,
        type_param_scope,
        None,
    )
}

pub(super) fn lower_supertraits(
    supertraits: &[TypeRef],
    traits: &BTreeMap<String, TraitInfo>,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_param_scope: &BTreeMap<String, ()>,
    self_type: Option<&Type>,
) -> Result<Vec<TraitBound>> {
    let mut lowered = Vec::new();
    for supertrait in supertraits {
        let Some((trait_name, trait_type_args)) = supertrait.named_parts() else {
            return Err(Diagnostic::at(
                supertrait.span,
                "a supertrait must be a named trait type",
            ));
        };
        let Some(trait_info) = traits.get(trait_name) else {
            return Err(Diagnostic::at(
                supertrait.span,
                format!("unknown trait `{}`", trait_name),
            ));
        };
        if trait_type_args.len() != trait_info.decl.type_params.len() {
            return Err(Diagnostic::at(
                supertrait.span,
                format!(
                    "trait `{}` expects {} type arguments, found {}",
                    trait_name,
                    trait_info.decl.type_params.len(),
                    trait_type_args.len()
                ),
            ));
        }
        let trait_args = trait_type_args
            .iter()
            .map(|arg| {
                lower_type_with_self(
                    arg,
                    type_names,
                    type_arities,
                    canonical_type_names,
                    type_param_scope,
                    self_type,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        lowered.push(TraitBound {
            trait_name: trait_name.to_string(),
            trait_args,
        });
    }
    Ok(lowered)
}

pub(super) fn lower_trait_bounds_with_self(
    bounds: &BTreeMap<String, Vec<TypeRef>>,
    traits: &BTreeMap<String, TraitInfo>,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_param_scope: &BTreeMap<String, ()>,
    self_type: Option<&Type>,
) -> Result<BTreeMap<String, Vec<TraitBound>>> {
    let mut lowered = BTreeMap::new();
    for (type_param, trait_bounds) in bounds {
        let mut names = Vec::new();
        for bound in trait_bounds {
            let Some((trait_name, trait_type_args)) = bound.named_parts() else {
                return Err(Diagnostic::at(
                    bound.span,
                    "a type parameter bound must be a named trait type",
                ));
            };
            let Some(trait_info) = traits.get(trait_name) else {
                return Err(Diagnostic::at(
                    bound.span,
                    format!("unknown trait `{}`", trait_name),
                ));
            };
            if trait_type_args.len() != trait_info.decl.type_params.len() {
                return Err(Diagnostic::at(
                    bound.span,
                    format!(
                        "trait `{}` expects {} type arguments, found {}",
                        trait_name,
                        trait_info.decl.type_params.len(),
                        trait_type_args.len()
                    ),
                ));
            }
            let trait_args = trait_type_args
                .iter()
                .map(|arg| {
                    lower_type_with_self(
                        arg,
                        type_names,
                        type_arities,
                        canonical_type_names,
                        type_param_scope,
                        self_type,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            names.push(TraitBound {
                trait_name: trait_name.to_string(),
                trait_args,
            });
        }
        lowered.insert(type_param.clone(), names);
    }
    Ok(lowered)
}

pub(crate) fn merge_trait_bounds(
    left: &BTreeMap<String, Vec<TraitBound>>,
    right: &BTreeMap<String, Vec<TraitBound>>,
) -> BTreeMap<String, Vec<TraitBound>> {
    let mut merged = left.clone();
    for (type_param, bounds) in right {
        merged
            .entry(type_param.clone())
            .or_default()
            .extend(bounds.iter().cloned());
    }
    merged
}

#[derive(Clone)]
pub(super) struct ResolvedTraitMethodInfo {
    pub(super) module_name: String,
    pub(super) decl: FunctionDecl,
    pub(super) signature: FunctionSignature,
    pub(super) type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    pub(super) rng_clone_safe_types: Vec<Type>,
    pub(super) array_equality_safe_types: Vec<Type>,
}

impl<'a> FunctionChecker<'a> {
    pub(super) fn check_trait_method(
        &self,
        trait_info: &TraitInfo,
        method_info: &TraitMethodInfo,
    ) -> Result<()> {
        let method = &method_info.decl;
        self.validate_view_return_contract(method)?;
        if method.body.is_empty() {
            return Ok(());
        }

        let trait_type_param_scope = type_param_scope(&trait_info.decl.type_params);
        let self_placeholder = Type::TypeParam("Self".to_string());
        let method_type_param_scope =
            merged_type_param_scope(&trait_type_param_scope, &method.type_params);
        let mut type_param_bounds = method_info.type_param_bounds.clone();
        let self_bounds = type_param_bounds.entry("Self".to_string()).or_default();
        self_bounds.push(TraitBound {
            trait_name: trait_info.decl.name.clone(),
            trait_args: trait_info
                .decl
                .type_params
                .iter()
                .cloned()
                .map(Type::TypeParam)
                .collect(),
        });
        let return_type = method_info.signature.return_type.clone();
        let checker = self
            .with_type_params(method_type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone())
            .with_closure_owner(ClosureOwner::TraitMethod {
                trait_name: trait_info.decl.name.clone(),
                method_name: method.name.clone(),
            })
            .with_implicit_param_borrows(
                &method.params,
                &method_info.signature.params,
                &method_info.signature.param_passings,
            );
        let mut locals = HashMap::new();
        checker.seed_module_scope(&mut locals);
        checker.seed_imported_modules(&mut locals);
        if let Some(receiver_kind) = method.receiver {
            locals.insert(
                "self".to_string(),
                LocalBinding {
                    ty: self_placeholder,
                    assignable: false,
                    mutable_place: receiver_kind == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing: receiver_kind,
                    borrow_origin: (receiver_kind != ReceiverKind::Value)
                        .then(|| "self".to_string()),
                    borrowed_at: (receiver_kind != ReceiverKind::Value).then_some(method.span),
                    match_borrow_place: None,
                    stale_match_borrow_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
                    shared_match_places: BTreeMap::new(),
                    captured: false,
                    view: None,
                    closure_loans: Vec::new(),
                },
            );
        }
        for ((param, ty), passing) in method
            .params
            .iter()
            .zip(method_info.signature.params.iter())
            .zip(method_info.signature.param_passings.iter().copied())
        {
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty: ty.clone(),
                    assignable: false,
                    mutable_place: passing == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing,
                    borrow_origin: (passing != ReceiverKind::Value).then(|| param.name.clone()),
                    borrowed_at: (passing != ReceiverKind::Value).then_some(param.span),
                    match_borrow_place: None,
                    stale_match_borrow_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
                    shared_match_places: BTreeMap::new(),
                    captured: false,
                    view: None,
                    closure_loans: Vec::new(),
                },
            );
        }

        let flow = checker.check_block(&method.body, &mut locals, &return_type, 0, true)?;
        if return_type != Type::Unit && flow != BlockFlow::AlwaysReturns {
            return Err(Diagnostic::at(
                method.span,
                format!("method `{}` is missing a return", method.name),
            ));
        }
        Ok(())
    }

    pub(super) fn check_trait_impl_supertraits(&self, trait_impl: &TraitImplInfo) -> Result<()> {
        let Some(trait_info) = self.traits.get(&trait_impl.trait_name) else {
            return Ok(());
        };
        let substitutions = self_type_substitutions(
            &trait_info.decl,
            &trait_impl.trait_args,
            trait_impl.for_type.clone(),
        );
        for supertrait in trait_info
            .supertraits
            .iter()
            .map(|supertrait| substitute_trait_bound(supertrait, &substitutions))
        {
            let implemented_elsewhere = self
                .trait_impls_in_scope()
                .filter(|candidate| {
                    !(candidate.trait_name == trait_impl.trait_name
                        && candidate.trait_args == trait_impl.trait_args
                        && candidate.for_type == trait_impl.for_type
                        && candidate.module_name == trait_impl.module_name)
                })
                .any(|candidate| {
                    let Some(substitutions) =
                        self.trait_impl_substitutions(candidate, &trait_impl.for_type)
                    else {
                        return false;
                    };
                    let implemented = self.resolved_trait_bound_for_impl(candidate, &substitutions);
                    self.trait_bound_closure(&implemented, &trait_impl.for_type)
                        .into_iter()
                        .any(|candidate| candidate == supertrait)
                });
            if !implemented_elsewhere {
                return Err(Diagnostic::at(
                    trait_impl.decl.span,
                    format!(
                        "impl of `{}` for `{}` requires supertrait `{}`",
                        trait_impl.trait_name, trait_impl.for_type, supertrait
                    ),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn check_trait_impl_method(
        &self,
        trait_name: &str,
        for_type: &Type,
        impl_type_params: &[String],
        impl_type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        method_info: &TraitImplMethodInfo,
    ) -> Result<()> {
        let method = &method_info.decl;
        self.validate_view_return_contract(method)?;
        let impl_type_param_scope = type_param_scope(impl_type_params);
        let type_param_scope = merged_type_param_scope(&impl_type_param_scope, &method.type_params);
        let type_param_bounds = merge_trait_bounds(
            impl_type_param_bounds,
            &lower_trait_bounds_with_self(
                &method.type_param_bounds,
                self.traits,
                self.type_names,
                self.type_arities,
                self.canonical_type_names,
                &type_param_scope,
                Some(for_type),
            )?,
        );
        let return_type = lower_type_with_self(
            &method.return_type,
            self.type_names,
            self.type_arities,
            self.canonical_type_names,
            &type_param_scope,
            Some(for_type),
        )?;
        let checker = self
            .with_type_params(type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone())
            .with_closure_owner(ClosureOwner::TraitImplMethod {
                trait_name: trait_name.to_string(),
                for_type: for_type.to_string(),
                method_name: method.name.clone(),
            })
            .with_implicit_param_borrows(
                &method.params,
                &method_info.signature.params,
                &method_info.signature.param_passings,
            );
        checker.check_param_defaults(
            &method.params,
            &type_param_scope,
            Some(for_type),
            false,
            "impl method",
        )?;
        let mut locals = HashMap::new();
        checker.seed_module_scope(&mut locals);
        checker.seed_imported_modules(&mut locals);
        if let Some(receiver_kind) = method.receiver {
            locals.insert(
                "self".to_string(),
                LocalBinding {
                    ty: for_type.clone(),
                    assignable: false,
                    mutable_place: receiver_kind == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing: receiver_kind,
                    borrow_origin: (receiver_kind != ReceiverKind::Value)
                        .then(|| "self".to_string()),
                    borrowed_at: (receiver_kind != ReceiverKind::Value).then_some(method.span),
                    match_borrow_place: None,
                    stale_match_borrow_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
                    shared_match_places: BTreeMap::new(),
                    captured: false,
                    view: None,
                    closure_loans: Vec::new(),
                },
            );
        }
        for ((param, ty), passing) in method
            .params
            .iter()
            .zip(method_info.signature.params.iter())
            .zip(method_info.signature.param_passings.iter().copied())
        {
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty: ty.clone(),
                    assignable: false,
                    mutable_place: passing == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing,
                    borrow_origin: (passing != ReceiverKind::Value).then(|| param.name.clone()),
                    borrowed_at: (passing != ReceiverKind::Value).then_some(param.span),
                    match_borrow_place: None,
                    stale_match_borrow_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
                    shared_match_places: BTreeMap::new(),
                    captured: false,
                    view: None,
                    closure_loans: Vec::new(),
                },
            );
        }
        let flow = checker.check_block(&method.body, &mut locals, &return_type, 0, true)?;
        if return_type != Type::Unit && flow != BlockFlow::AlwaysReturns {
            return Err(Diagnostic::at(
                method.span,
                format!("method `{}` is missing a return", method.name),
            ));
        }
        Ok(())
    }

    pub(super) fn operator_method_from_type_param(
        &self,
        type_param_name: &str,
        trait_name: &str,
        method_name: &str,
        rhs: Option<&Type>,
    ) -> Result<Option<ResolvedTraitMethodInfo>> {
        let Some(trait_info) = self.traits.get(trait_name) else {
            return Ok(None);
        };
        let Some(method) = trait_info.methods.get(method_name) else {
            return Err(Diagnostic::new(format!(
                "operator trait `{}` must define method `{}`",
                trait_name, method_name
            )));
        };
        let mut matches = Vec::new();
        let self_ty = Type::TypeParam(type_param_name.to_string());
        for bound in self
            .type_param_bounds
            .get(type_param_name)
            .into_iter()
            .flatten()
        {
            for bound in self.trait_bound_closure(bound, &self_ty) {
                if bound.trait_name != trait_name {
                    continue;
                }
                match rhs {
                    Some(rhs_ty)
                        if !bound.trait_args.is_empty() && &bound.trait_args[0] == rhs_ty => {}
                    None if bound.trait_args.len() == 1 => {}
                    _ => continue,
                }
                let trait_substitutions =
                    self_type_substitutions(&trait_info.decl, &bound.trait_args, self_ty.clone());
                matches.push(ResolvedTraitMethodInfo {
                    module_name: trait_info.module_name.clone(),
                    decl: method.decl.clone(),
                    signature: FunctionSignature {
                        params: method
                            .signature
                            .params
                            .iter()
                            .map(|param| substitute_type(param, &trait_substitutions))
                            .collect(),
                        param_passings: method.signature.param_passings.clone(),
                        return_type: substitute_type(
                            &method.signature.return_type,
                            &trait_substitutions,
                        ),
                        rng_clone_safe_type_params: method
                            .signature
                            .rng_clone_safe_type_params
                            .iter()
                            .filter(|name| method.decl.type_params.contains(name))
                            .cloned()
                            .collect(),
                        array_equality_safe_type_params: method
                            .signature
                            .array_equality_safe_type_params
                            .iter()
                            .filter(|name| method.decl.type_params.contains(name))
                            .cloned()
                            .collect(),
                    },
                    type_param_bounds: substitute_trait_bounds(
                        &method.type_param_bounds,
                        &trait_substitutions,
                    ),
                    rng_clone_safe_types: method
                        .signature
                        .rng_clone_safe_type_params
                        .iter()
                        .filter(|name| !method.decl.type_params.contains(name))
                        .map(|name| {
                            substitute_type(&Type::TypeParam(name.clone()), &trait_substitutions)
                        })
                        .collect(),
                    array_equality_safe_types: method
                        .signature
                        .array_equality_safe_type_params
                        .iter()
                        .filter(|name| !method.decl.type_params.contains(name))
                        .map(|name| {
                            substitute_type(&Type::TypeParam(name.clone()), &trait_substitutions)
                        })
                        .collect(),
                });
            }
        }
        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.pop()),
            _ => Err(Diagnostic::new(format!(
                "operator trait `{}` is ambiguous for type parameter `{}`",
                trait_name, type_param_name
            ))),
        }
    }

    pub(super) fn operator_method_for_concrete_type(
        &self,
        span: crate::diag::Span,
        receiver_ty: &Type,
        trait_name: &str,
        method_name: &str,
        rhs: Option<&Type>,
    ) -> Result<Option<(ResolvedTraitMethodInfo, HashMap<String, Type>)>> {
        let mut matches = Vec::new();
        for trait_impl in self
            .trait_impls_in_scope()
            .filter(|trait_impl| trait_impl.trait_name == trait_name)
        {
            let Some(method) = trait_impl.methods.get(method_name) else {
                continue;
            };
            let mut type_params = BTreeSet::new();
            collect_type_params_from_type(&trait_impl.for_type, &mut type_params);
            for trait_arg in &trait_impl.trait_args {
                collect_type_params_from_type(trait_arg, &mut type_params);
            }
            let mut substitutions = HashMap::new();
            if !type_pattern_matches(
                &trait_impl.for_type,
                receiver_ty,
                &type_params,
                &mut substitutions,
            ) {
                continue;
            }
            match rhs {
                Some(rhs_ty) if !trait_impl.trait_args.is_empty() => {
                    if !type_pattern_matches(
                        &trait_impl.trait_args[0],
                        rhs_ty,
                        &type_params,
                        &mut substitutions,
                    ) {
                        continue;
                    }
                }
                None if trait_impl.trait_args.len() == 1 => {}
                _ => continue,
            }
            let mut valid = true;
            for (type_param, bounds) in &trait_impl.type_param_bounds {
                let Some(actual_ty) = substitutions.get(type_param) else {
                    valid = false;
                    break;
                };
                for impl_bound in bounds {
                    let resolved_bound = substitute_trait_bound(impl_bound, &substitutions);
                    if !self.type_implements_trait_bound(actual_ty, &resolved_bound) {
                        valid = false;
                        break;
                    }
                }
                if !valid {
                    break;
                }
            }
            if !valid {
                continue;
            }
            matches.push((
                trait_impl_specificity(trait_impl),
                ResolvedTraitMethodInfo {
                    module_name: trait_impl.module_name.clone(),
                    decl: method.decl.clone(),
                    signature: FunctionSignature {
                        params: method
                            .signature
                            .params
                            .iter()
                            .map(|param| substitute_type(param, &substitutions))
                            .collect(),
                        param_passings: method.signature.param_passings.clone(),
                        return_type: substitute_type(&method.signature.return_type, &substitutions),
                        rng_clone_safe_type_params: method
                            .signature
                            .rng_clone_safe_type_params
                            .iter()
                            .filter(|name| method.decl.type_params.contains(name))
                            .cloned()
                            .collect(),
                        array_equality_safe_type_params: method
                            .signature
                            .array_equality_safe_type_params
                            .iter()
                            .filter(|name| method.decl.type_params.contains(name))
                            .cloned()
                            .collect(),
                    },
                    type_param_bounds: substitute_trait_bounds(
                        &method.type_param_bounds,
                        &substitutions,
                    ),
                    rng_clone_safe_types: method
                        .signature
                        .rng_clone_safe_type_params
                        .iter()
                        .filter(|name| !method.decl.type_params.contains(name))
                        .map(|name| substitute_type(&Type::TypeParam(name.clone()), &substitutions))
                        .collect(),
                    array_equality_safe_types: method
                        .signature
                        .array_equality_safe_type_params
                        .iter()
                        .filter(|name| !method.decl.type_params.contains(name))
                        .map(|name| substitute_type(&Type::TypeParam(name.clone()), &substitutions))
                        .collect(),
                },
                substitutions,
            ));
        }
        if matches.is_empty() {
            return Ok(None);
        }
        matches.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        let best_score = matches[0].0;
        let mut best_matches = matches
            .into_iter()
            .filter(|(score, _, _)| *score == best_score)
            .collect::<Vec<_>>();
        match best_matches.len() {
            1 => {
                let (_, method, substitutions) = best_matches
                    .pop()
                    .expect("best operator trait impl should exist");
                Ok(Some((method, substitutions)))
            }
            _ => Err(Diagnostic::at(
                span,
                format!(
                    "operator trait `{}` is ambiguous for type `{}`",
                    trait_name, receiver_ty
                ),
            )),
        }
    }

    pub(super) fn trait_impls_in_scope(&self) -> impl Iterator<Item = &TraitImplInfo> + '_ {
        self.trait_impls.iter().chain(
            self.module_registry
                .values()
                .flat_map(|namespace| namespace.trait_impls.iter()),
        )
    }

    pub(super) fn trait_impl_substitutions(
        &self,
        trait_impl: &TraitImplInfo,
        actual: &Type,
    ) -> Option<HashMap<String, Type>> {
        let type_params = trait_impl
            .type_params
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut substitutions = HashMap::new();
        if !type_pattern_matches(
            &trait_impl.for_type,
            actual,
            &type_params,
            &mut substitutions,
        ) {
            return None;
        }
        for (type_param, bounds) in &trait_impl.type_param_bounds {
            let actual_ty = substitutions.get(type_param)?;
            for bound in bounds {
                let resolved_bound = substitute_trait_bound(bound, &substitutions);
                if !self.type_implements_trait_bound(actual_ty, &resolved_bound) {
                    return None;
                }
            }
        }
        Some(substitutions)
    }

    pub(super) fn collect_trait_bound_closure(
        &self,
        bound: &TraitBound,
        self_ty: &Type,
        seen: &mut BTreeSet<String>,
        closure: &mut Vec<TraitBound>,
    ) {
        let key = format!("{} for {}", bound, self_ty);
        if !seen.insert(key) {
            return;
        }
        closure.push(bound.clone());
        let Some(trait_info) = self.traits.get(&bound.trait_name) else {
            return;
        };
        let substitutions =
            self_type_substitutions(&trait_info.decl, &bound.trait_args, self_ty.clone());
        for supertrait in &trait_info.supertraits {
            let resolved = substitute_trait_bound(supertrait, &substitutions);
            self.collect_trait_bound_closure(&resolved, self_ty, seen, closure);
        }
    }

    pub(super) fn trait_bound_closure(
        &self,
        bound: &TraitBound,
        self_ty: &Type,
    ) -> Vec<TraitBound> {
        let mut closure = Vec::new();
        let mut seen = BTreeSet::new();
        self.collect_trait_bound_closure(bound, self_ty, &mut seen, &mut closure);
        closure
    }

    pub(super) fn resolved_trait_bound_for_impl(
        &self,
        trait_impl: &TraitImplInfo,
        substitutions: &HashMap<String, Type>,
    ) -> TraitBound {
        TraitBound {
            trait_name: trait_impl.trait_name.clone(),
            trait_args: trait_impl
                .trait_args
                .iter()
                .map(|arg| substitute_type(arg, substitutions))
                .collect(),
        }
    }

    pub(super) fn type_implements_trait_bound(&self, ty: &Type, bound: &TraitBound) -> bool {
        self.trait_impls_in_scope().any(|trait_impl| {
            let Some(substitutions) = self.trait_impl_substitutions(trait_impl, ty) else {
                return false;
            };
            let implemented = self.resolved_trait_bound_for_impl(trait_impl, &substitutions);
            self.trait_bound_closure(&implemented, ty)
                .into_iter()
                .any(|candidate| candidate == *bound)
        })
    }

    pub(super) fn assert_type_satisfies_bounds(
        &self,
        ty: &Type,
        bounds: &[TraitBound],
        span: crate::diag::Span,
    ) -> Result<()> {
        if matches!(ty, Type::Function { .. }) && !bounds.is_empty() {
            return Err(Diagnostic::coded_at(
                "AU2005",
                span,
                "function values do not participate in trait or trait-object dispatch in this language version",
            )
            .with_help(
                "pass the concrete function type directly; callable trait objects are outside the current function-values feature",
            ));
        }
        for bound in bounds {
            match ty {
                Type::TypeParam(name) => {
                    let current_bounds = self
                        .type_param_bounds
                        .get(name)
                        .cloned()
                        .unwrap_or_default();
                    let self_ty = Type::TypeParam(name.clone());
                    let satisfies = current_bounds.into_iter().any(|current| {
                        self.trait_bound_closure(&current, &self_ty)
                            .into_iter()
                            .any(|candidate| candidate == *bound)
                    });
                    if !satisfies {
                        return Err(Diagnostic::at(
                            span,
                            format!(
                                "type parameter `{}` does not satisfy trait bound `{}`",
                                name, bound
                            ),
                        ));
                    }
                }
                _ => {
                    if !self.type_implements_trait_bound(ty, bound) {
                        return Err(Diagnostic::at(
                            span,
                            format!("type `{}` does not implement trait `{}`", ty, bound),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn trait_method_from_type_param(
        &self,
        type_param_name: &str,
        method_name: &str,
    ) -> Result<ResolvedTraitMethodInfo> {
        let mut matches = Vec::new();
        let self_ty = Type::TypeParam(type_param_name.to_string());
        for bound in self
            .type_param_bounds
            .get(type_param_name)
            .into_iter()
            .flatten()
        {
            for bound in self.trait_bound_closure(bound, &self_ty) {
                if let Some(trait_info) = self.traits.get(&bound.trait_name) {
                    if let Some(method) = trait_info.methods.get(method_name) {
                        let trait_substitutions = self_type_substitutions(
                            &trait_info.decl,
                            &bound.trait_args,
                            self_ty.clone(),
                        );
                        matches.push(ResolvedTraitMethodInfo {
                            module_name: trait_info.module_name.clone(),
                            decl: method.decl.clone(),
                            signature: FunctionSignature {
                                params: method
                                    .signature
                                    .params
                                    .iter()
                                    .map(|param| substitute_type(param, &trait_substitutions))
                                    .collect(),
                                param_passings: method.signature.param_passings.clone(),
                                return_type: substitute_type(
                                    &method.signature.return_type,
                                    &trait_substitutions,
                                ),
                                rng_clone_safe_type_params: method
                                    .signature
                                    .rng_clone_safe_type_params
                                    .iter()
                                    .filter(|name| method.decl.type_params.contains(name))
                                    .cloned()
                                    .collect(),
                                array_equality_safe_type_params: method
                                    .signature
                                    .array_equality_safe_type_params
                                    .iter()
                                    .filter(|name| method.decl.type_params.contains(name))
                                    .cloned()
                                    .collect(),
                            },
                            type_param_bounds: substitute_trait_bounds(
                                &method.type_param_bounds,
                                &trait_substitutions,
                            ),
                            rng_clone_safe_types: method
                                .signature
                                .rng_clone_safe_type_params
                                .iter()
                                .map(|name| {
                                    substitute_type(
                                        &Type::TypeParam(name.clone()),
                                        &trait_substitutions,
                                    )
                                })
                                .collect(),
                            array_equality_safe_types: method
                                .signature
                                .array_equality_safe_type_params
                                .iter()
                                .filter(|name| !method.decl.type_params.contains(name))
                                .map(|name| {
                                    substitute_type(
                                        &Type::TypeParam(name.clone()),
                                        &trait_substitutions,
                                    )
                                })
                                .collect(),
                        });
                    }
                }
            }
        }
        match matches.len() {
            1 => Ok(matches.remove(0)),
            0 => Err(Diagnostic::new(format!(
                "type parameter `{}` has no method `{}` in its trait bounds",
                type_param_name, method_name
            ))),
            _ => Err(Diagnostic::new(format!(
                "method `{}` is ambiguous for type parameter `{}`",
                method_name, type_param_name
            ))),
        }
    }

    pub(super) fn trait_method_for_concrete_type(
        &self,
        ty: &Type,
        method_name: &str,
        span: crate::diag::Span,
    ) -> Result<Option<TraitMethodMatch<'_>>> {
        let mut matches = Vec::new();
        for trait_impl in self.trait_impls_in_scope() {
            let Some(substitutions) = self.trait_impl_substitutions(trait_impl, ty) else {
                continue;
            };
            let Some(method) = trait_impl.methods.get(method_name) else {
                continue;
            };
            matches.push((
                trait_impl_specificity(trait_impl),
                trait_impl,
                method,
                substitutions,
            ));
        }

        if matches.is_empty() {
            return Ok(None);
        }

        matches.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        let best_score = matches[0].0;
        let mut best_matches = matches
            .into_iter()
            .filter(|(score, _, _, _)| *score == best_score)
            .collect::<Vec<_>>();
        match best_matches.len() {
            1 => {
                let (_, trait_impl, method, substitutions) = best_matches
                    .pop()
                    .expect("best trait impl match should exist");
                Ok(Some((trait_impl, method, substitutions)))
            }
            _ => Err(Diagnostic::at(
                span,
                format!(
                    "method `{}` is ambiguous for type `{}` because multiple trait impls match with the same specificity",
                    method_name, ty
                ),
            )),
        }
    }

    pub(super) fn has_from_conversion(
        &self,
        source_ty: &Type,
        target_ty: &Type,
        span: crate::diag::Span,
    ) -> Result<bool> {
        for trait_impl in self.trait_impls_in_scope() {
            if trait_impl.trait_name != "From" || trait_impl.trait_args.len() != 1 {
                continue;
            }
            let Some(method) = trait_impl.methods.get("from") else {
                continue;
            };
            let Some(mut substitutions) = self.trait_impl_substitutions(trait_impl, target_ty)
            else {
                continue;
            };
            if substitute_type(&trait_impl.trait_args[0], &substitutions) != *source_ty {
                continue;
            }
            let operation = "implicit `From.from` conversion";
            self.enforce_rng_clone_obligations_before_method_inference(
                operation,
                &method.signature.rng_clone_safe_type_params,
                &substitutions,
                &method.decl.type_params,
                span,
            )?;
            substitutions = self.infer_method_type_substitutions(
                operation,
                &method.decl.type_params,
                &method.signature.params,
                &method.type_param_bounds,
                &method.signature.rng_clone_safe_type_params,
                &method.signature.array_equality_safe_type_params,
                std::slice::from_ref(source_ty),
                substitutions,
                span,
            )?;
            let _ = substitutions;
            return Ok(true);
        }
        Ok(false)
    }
}
