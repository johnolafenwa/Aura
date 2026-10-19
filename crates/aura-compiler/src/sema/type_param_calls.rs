//! Associated trait methods called through a bounded type parameter, such as
//! `T.make(seed)` in `def build[T: Make](seed: int64) -> T`.
//!
//! Generic bodies are erased, and both backends dispatch a bound method on
//! its runtime receiver. An associated method has no receiver, so the checker
//! records each such call and each use of a generic function with its checked
//! type arguments. MIR lowering then specializes, per concrete type, every
//! function that needs a type to pick an implementation.

use super::{
    find_namespace_in_modules, BTreeMap, BTreeSet, ClosureOwner, Diagnostic, Expr, FunctionChecker,
    HashMap, LocalBinding, ModuleNamespace, Result, Type,
};

/// Source position of a recorded call or function use.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TypeParamCallId {
    pub module_name: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeParamCall {
    /// `type_param.method(...)` inside the free function `owner`.
    Associated {
        owner: String,
        type_param: String,
        trait_name: String,
        method: String,
    },
    /// A use of the generic free function `callee` of `callee_module` with
    /// its checked type arguments, by type parameter, in declaration order.
    /// `owner` is the enclosing free function, or `None` outside one.
    Generic {
        owner: Option<String>,
        callee_module: String,
        callee: String,
        type_args: Vec<(String, Type)>,
        kind: GenericUse,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenericUse {
    Call,
    Value,
    Task,
}

/// The free functions, as `(module, name)`, whose bodies need a concrete
/// type for one of their type parameters: they call an associated method
/// through it, or pass it on to such a function.
pub(crate) fn specialized_functions<'a>(
    records: impl IntoIterator<Item = (&'a TypeParamCallId, &'a TypeParamCall)>,
) -> BTreeSet<(String, String)> {
    let records = records.into_iter().collect::<Vec<_>>();
    let mut specialized = BTreeSet::new();
    for (id, record) in &records {
        if let TypeParamCall::Associated { owner, .. } = record {
            specialized.insert((id.module_name.clone(), owner.clone()));
        }
    }
    loop {
        let mut changed = false;
        for (id, record) in &records {
            let TypeParamCall::Generic {
                owner: Some(owner),
                callee_module,
                callee,
                type_args,
                ..
            } = record
            else {
                continue;
            };
            if specialized.contains(&(callee_module.clone(), callee.clone()))
                && type_args
                    .iter()
                    .any(|(_, ty)| super::has_unresolved_type_params(ty))
            {
                changed |= specialized.insert((id.module_name.clone(), owner.clone()));
            }
        }
        if !changed {
            return specialized;
        }
    }
}

/// Every record this program can reach: its own and those exported by the
/// modules it imports.
pub(crate) fn collect_type_param_calls(
    own: &BTreeMap<TypeParamCallId, TypeParamCall>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
) -> BTreeMap<TypeParamCallId, TypeParamCall> {
    fn visit(
        modules: &BTreeMap<String, ModuleNamespace>,
        records: &mut BTreeMap<TypeParamCallId, TypeParamCall>,
        seen: &mut BTreeSet<String>,
    ) {
        for namespace in modules.values() {
            if seen.insert(namespace.path.clone()) {
                for (id, record) in &namespace.type_param_calls {
                    records.entry(id.clone()).or_insert_with(|| record.clone());
                }
            }
            visit(&namespace.modules, records, seen);
            visit(&namespace.imported_modules, records, seen);
        }
    }
    let mut records = own.clone();
    let mut seen = BTreeSet::new();
    visit(module_registry, &mut records, &mut seen);
    visit(imported_modules, &mut records, &mut seen);
    records
}

/// The recorded call or use at a source position, from this program or an
/// imported module.
pub(crate) fn lookup_type_param_call(
    own: &BTreeMap<TypeParamCallId, TypeParamCall>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
    id: &TypeParamCallId,
) -> Option<TypeParamCall> {
    own.get(id).cloned().or_else(|| {
        find_namespace_in_modules(module_registry, &id.module_name)
            .or_else(|| find_namespace_in_modules(imported_modules, &id.module_name))
            .and_then(|namespace| namespace.type_param_calls.get(id).cloned())
    })
}

impl FunctionChecker<'_> {
    fn type_param_call_id(&self, span: crate::diag::Span) -> TypeParamCallId {
        TypeParamCallId {
            module_name: self.module_name.to_string(),
            line: span.line,
            column: span.column,
        }
    }

    fn free_function_owner(&self) -> Option<String> {
        match &self.closure_owner {
            ClosureOwner::Function(name) => Some(name.clone()),
            _ => None,
        }
    }

    /// `T.method(...)` names a type parameter in scope that no local binding
    /// shadows.
    pub(super) fn names_type_param(
        &self,
        object: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<String> {
        match &object.kind {
            super::ExprKind::Name(name)
                if !locals.contains_key(name) && self.type_params.contains_key(name) =>
            {
                Some(name.clone())
            }
            _ => None,
        }
    }

    /// The associated method a `T.method(...)` callee names, when `object`
    /// is a type parameter whose bounds provide one without `self`.
    pub(super) fn type_param_associated_method(
        &self,
        object: &Expr,
        method_name: &str,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<super::traits::ResolvedTraitMethodInfo> {
        let type_param = self.names_type_param(object, locals)?;
        self.trait_method_from_type_param(&type_param, method_name)
            .ok()
            .filter(|method| method.decl.receiver.is_none())
    }

    /// Checks `type_param.method(args)` against the associated method the
    /// type parameter's bounds provide, and records it for specialization.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_of_type_param_associated_call(
        &self,
        type_param: &str,
        method_name: &str,
        explicit_type_args: Option<&[crate::ast::TypeRef]>,
        args: &[crate::ast::Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        let (trait_name, method) = self
            .trait_method_from_type_param_with_trait(type_param, method_name)
            .map_err(|error| Diagnostic::coded_at("AU2002", span, error.message))?;
        if method.decl.receiver.is_some() {
            return Err(Diagnostic::coded_at(
                "AU2005",
                span,
                format!(
                    "method `{method_name}` of trait `{trait_name}` takes `self`; call it on a value of type `{type_param}`"
                ),
            ));
        }
        if explicit_type_args.is_some() || !method.decl.type_params.is_empty() {
            return Err(Diagnostic::coded_at(
                "AU2005",
                span,
                format!(
                    "associated method `{trait_name}.{method_name}` has type parameters, and calling it through type parameter `{type_param}` is not supported"
                ),
            ));
        }
        let Some(owner) = self.free_function_owner() else {
            return Err(Diagnostic::coded_at(
                "AU2005",
                span,
                format!(
                    "`{type_param}.{method_name}(...)` is supported only in module-level functions"
                ),
            )
            .with_help("move the call into a module-level generic function and call that"));
        };
        let result = self.type_check_callable_args(
            &format!("associated method `{type_param}.{method_name}`"),
            &[],
            &method.decl.params,
            &method.signature.param_passings,
            &method.signature.params,
            &method.signature.return_type,
            &BTreeMap::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            args,
            span,
            locals,
            expected,
            HashMap::new(),
        )?;
        self.type_param_calls.borrow_mut().insert(
            self.type_param_call_id(span),
            TypeParamCall::Associated {
                owner,
                type_param: type_param.to_string(),
                trait_name,
                method: method_name.to_string(),
            },
        );
        Ok(result)
    }

    /// Checks a call of the free function `function` and records its type
    /// arguments when it is generic.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_check_recorded_function_call(
        &self,
        function: &super::FunctionInfo,
        callee_name: &str,
        args: &[crate::ast::Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
        seed_substitutions: HashMap<String, Type>,
    ) -> Result<Type> {
        let checked = self.type_check_callable_args_detailed(
            callee_name,
            &function.decl.type_params,
            &function.decl.params,
            &function.signature.param_passings,
            &function.signature.params,
            &function.signature.return_type,
            &function.type_param_bounds,
            &function.signature.rng_clone_safe_type_params,
            &function.signature.array_equality_safe_type_params,
            args,
            span,
            locals,
            expected,
            seed_substitutions,
            super::ClosureArgumentPolicy::Reject,
        )?;
        self.record_generic_function_use(function, checked.type_args, span, GenericUse::Call);
        Ok(checked.return_type)
    }

    /// Records a use of the generic free function `function` with its
    /// checked type arguments.
    pub(super) fn record_generic_function_use(
        &self,
        function: &super::FunctionInfo,
        type_args: Vec<Type>,
        span: crate::diag::Span,
        kind: GenericUse,
    ) {
        if function.decl.type_params.is_empty() {
            return;
        }
        self.type_param_calls.borrow_mut().insert(
            self.type_param_call_id(span),
            TypeParamCall::Generic {
                owner: self.free_function_owner(),
                callee_module: function.module_name.clone(),
                callee: function.decl.name.clone(),
                type_args: function
                    .decl
                    .type_params
                    .iter()
                    .cloned()
                    .zip(type_args)
                    .collect(),
                kind,
            },
        );
    }
}

/// Rejects the uses MIR cannot specialize: a function that needs a concrete
/// type, used with a type parameter outside a module-level function or as a
/// task target.
pub(crate) fn validate_type_param_calls(
    own: &BTreeMap<TypeParamCallId, TypeParamCall>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
) -> Result<()> {
    let records = collect_type_param_calls(own, module_registry, imported_modules);
    let specialized = specialized_functions(&records);
    for (id, record) in own {
        let TypeParamCall::Generic {
            owner,
            callee_module,
            callee,
            type_args,
            kind,
        } = record
        else {
            continue;
        };
        if !specialized.contains(&(callee_module.clone(), callee.clone())) {
            continue;
        }
        let span = crate::diag::Span::new(id.line, id.column);
        if *kind == GenericUse::Task {
            return Err(Diagnostic::coded_at(
                "AU2005",
                span,
                format!(
                    "function `{callee}` calls an associated method through a type parameter, so it cannot start a task"
                ),
            )
            .with_help("start the task with a module-level function that calls it"));
        }
        if owner.is_none()
            && type_args
                .iter()
                .any(|(_, ty)| super::has_unresolved_type_params(ty))
        {
            return Err(Diagnostic::coded_at(
                "AU2005",
                span,
                format!(
                    "function `{callee}` calls an associated method through a type parameter, so it needs a concrete type here"
                ),
            )
            .with_help("call it from a module-level generic function, or pass a concrete type"));
        }
    }
    Ok(())
}
