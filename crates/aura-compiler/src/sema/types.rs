//! Semantic type identity, lowering, substitution, and unification.

use super::{
    check_callable_positions, fmt, function_type_mismatch_message, is_array_dtype,
    is_builtin_copy_named_type, is_builtin_type, preserves_qualified_builtin_type_name,
    resolve_param_passing, BTreeMap, BTreeSet, CallableType, ClosureCallKind, ClosureCapture,
    Deserialize, Diagnostic, FunctionParamContract, HashMap, ReceiverKind, Result, Serialize,
    TraitImplInfo, TypeRef,
};

/// Name inventory and transparent alias templates used by semantic type lowering.
/// AST declarations remain intact for source-facing tooling.
#[derive(Clone, Debug, Default)]
pub struct TypeDefinitions {
    pub(super) module_name: String,
    names: BTreeMap<String, crate::diag::Span>,
    pub(super) aliases: BTreeMap<String, crate::ast::TypeAliasDecl>,
    pub(super) checked_aliases: BTreeMap<String, super::AliasInfo>,
    pub(super) imported_aliases: BTreeMap<String, super::AliasInfo>,
    pub(super) non_value_names: BTreeSet<String>,
    alias_templates: std::cell::RefCell<BTreeMap<String, Type>>,
    budget: super::type_budget::ExpansionBudget,
    pub(crate) union_injections:
        std::rc::Rc<std::cell::RefCell<BTreeMap<super::UnionInjectionId, super::UnionInjection>>>,
    pub(crate) narrowed_reads:
        std::rc::Rc<std::cell::RefCell<BTreeMap<super::NarrowedReadId, super::NarrowedRead>>>,
    pub(super) union_probe_work: std::rc::Rc<std::cell::Cell<usize>>,
}

impl std::ops::Deref for TypeDefinitions {
    type Target = BTreeMap<String, crate::diag::Span>;
    fn deref(&self) -> &Self::Target {
        &self.names
    }
}

impl std::ops::DerefMut for TypeDefinitions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.names
    }
}

impl From<BTreeMap<String, crate::diag::Span>> for TypeDefinitions {
    fn from(names: BTreeMap<String, crate::diag::Span>) -> Self {
        Self {
            module_name: String::new(),
            names,
            aliases: BTreeMap::new(),
            checked_aliases: BTreeMap::new(),
            imported_aliases: BTreeMap::new(),
            non_value_names: BTreeSet::new(),
            alias_templates: std::cell::RefCell::new(BTreeMap::new()),
            budget: Default::default(),
            union_injections: Default::default(),
            narrowed_reads: Default::default(),
            union_probe_work: Default::default(),
        }
    }
}

impl TypeDefinitions {
    pub(crate) fn expansion_budget(&self) -> &super::type_budget::ExpansionBudget {
        &self.budget
    }

    /// Validate declaration dependencies, not instantiated argument strings.
    /// In particular Grow[T] = Grow[list[T]] closes a one-node cycle.
    pub(super) fn validate_alias_cycles(&self, declaration_order: &[String]) -> Result<()> {
        fn dependencies<'a>(
            ty: &'a TypeRef,
            aliases: &BTreeMap<String, crate::ast::TypeAliasDecl>,
            parameters: &[String],
        ) -> Vec<(&'a str, crate::diag::Span)> {
            let mut work = vec![ty];
            let mut found = Vec::new();
            while let Some(ty) = work.pop() {
                match &ty.kind {
                    crate::ast::TypeRefKind::Named { name, args } => {
                        if !parameters.contains(name) && aliases.contains_key(name) {
                            found.push((name.as_str(), ty.span));
                        }
                        work.extend(args.iter().rev());
                    }
                    crate::ast::TypeRefKind::Tuple(members)
                    | crate::ast::TypeRefKind::Union(members) => work.extend(members.iter().rev()),
                    crate::ast::TypeRefKind::Function {
                        params,
                        return_type,
                        ..
                    } => {
                        work.push(return_type);
                        work.extend(params.iter().rev().map(|param| &param.ty));
                    }
                    crate::ast::TypeRefKind::Callable { signature, .. } => work.push(signature),
                }
            }
            found
        }
        let graph = self
            .aliases
            .iter()
            .map(|(name, alias)| {
                let mut edges = dependencies(&alias.target, &self.aliases, &alias.type_params);
                for bounds in alias.type_param_bounds.values() {
                    for bound in bounds {
                        edges.extend(dependencies(bound, &self.aliases, &alias.type_params));
                    }
                }
                (name.as_str(), edges)
            })
            .collect::<BTreeMap<_, _>>();
        let mut complete = BTreeSet::new();
        for root in declaration_order {
            if complete.contains(root.as_str()) {
                continue;
            }
            let mut path = vec![(root.as_str(), 0usize)];
            let mut active = BTreeMap::from([(root.as_str(), 0usize)]);
            while let Some((name, next)) = path.last_mut() {
                let edges = &graph[name];
                if *next == edges.len() {
                    complete.insert(*name);
                    active.remove(name);
                    path.pop();
                    continue;
                }
                let (dependency, closing_span) = edges[*next];
                *next += 1;
                if let Some(&start) = active.get(dependency) {
                    let mut names = path[start..]
                        .iter()
                        .map(|(name, _)| *name)
                        .collect::<Vec<_>>();
                    names.push(dependency);
                    let mut error = Diagnostic::coded_at(
                        "AU2012",
                        closing_span,
                        format!("cyclic type alias: {}", names.join(" -> ")),
                    );
                    for (name, _) in &path[start..] {
                        error = error.with_secondary(
                            self.aliases[*name].span,
                            format!("alias `{name}` declared here"),
                        );
                    }
                    return Err(error);
                }
                if !complete.contains(dependency) {
                    active.insert(dependency, path.len());
                    path.push((dependency, 0));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TraitBound {
    pub trait_name: String,
    pub trait_args: Vec<Type>,
}

impl fmt::Display for TraitBound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.trait_args.is_empty() {
            write!(f, "{}", self.trait_name)
        } else {
            write!(
                f,
                "{}[{}]",
                self.trait_name,
                self.trait_args
                    .iter()
                    .map(Type::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Type {
    Union(Box<UnionType>),
    Named(String, Vec<Type>),
    Tuple(Vec<Type>),
    Function {
        params: Vec<FunctionParamContract>,
        return_type: Box<Type>,
    },
    Closure {
        params: Box<Vec<FunctionParamContract>>,
        return_type: Box<Type>,
        /// Kept indirect so closure-only ownership metadata does not inflate
        /// every `Type`, and consequently every typed runtime collection.
        ///
        /// `Box<Vec<_>>` remains transparent to Serde, preserving the
        /// established semantic-interface and MIR cache schema.
        captures: Box<Vec<ClosureCapture>>,
        call_kind: ClosureCallKind,
    },
    /// `Callable[...]` / `TaskCallable[...]` owned storage with an erased
    /// environment; boxed so `Type` keeps its size budget.
    Callable(Box<CallableType>),
    /// The result position of a callable value whose call returns a view of
    /// one explicit ordinary argument (C9, Q22 A): `-> view [mut] T from
    /// name`, encoded by the origin parameter's ordinal. It is legal only as
    /// the `return_type` of `Function`, `Closure`, or `Callable`; a call
    /// through such a value has the pointee type and binds a returned view.
    ReturnedView(Box<ReturnedViewType>),
    TypeParam(String),
    Module(String),
    Unit,
}

/// A stored callable's argument-origin returned-view contract (C9).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReturnedViewType {
    pub mutable: bool,
    pub pointee: Type,
    /// Ordinal of the origin parameter in the callable's parameter list.
    pub origin: usize,
}

/// Normalized members and their defining-module structural identities.
/// Keys determine tags; written aliases and source ordering do not.
#[derive(Clone, Debug, Serialize)]
pub struct UnionType {
    pub(crate) members: Vec<Type>,
    pub(crate) keys: Vec<String>,
    pub(crate) module_name: String,
}

impl<'de> Deserialize<'de> for UnionType {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct EncodedUnion {
            members: Vec<Type>,
            keys: Vec<String>,
            module_name: String,
        }
        let encoded = EncodedUnion::deserialize(deserializer)?;
        let invalid =
            |reason: &str| serde::de::Error::custom(format!("invalid union metadata: {reason}"));
        if encoded.members.len() < 2 {
            return Err(invalid("a stored union requires at least two members"));
        }
        if encoded.members.len() != encoded.keys.len() {
            return Err(invalid("member and key counts differ"));
        }
        if encoded.keys.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(invalid("keys must be strictly ordered and unique"));
        }
        for (member, key) in encoded.members.iter().zip(&encoded.keys) {
            if matches!(member, Type::Union(_)) {
                return Err(invalid("nested members must be flattened"));
            }
            super::type_budget::ExpansionBudget::default()
                .check_canonical_key(
                    member,
                    &encoded.module_name,
                    &BTreeMap::new(),
                    crate::diag::Span::new(1, 1),
                )
                .map_err(|error| invalid(&error.message))?;
            if member.canonical_key(&encoded.module_name, &BTreeMap::new()) != *key {
                return Err(invalid("member key does not match its resolved type"));
            }
        }
        Ok(Self {
            members: encoded.members,
            keys: encoded.keys,
            module_name: encoded.module_name,
        })
    }
}

impl Type {
    pub(crate) fn source_type_ref(&self, span: crate::diag::Span) -> Result<TypeRef> {
        use crate::ast::{FunctionTypeParam, ParamMode, TypeRefKind};
        let kind = match self {
            Type::Named(name, args) => TypeRefKind::Named {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| arg.source_type_ref(span))
                    .collect::<Result<Vec<_>>>()?,
            },
            Type::ReturnedView(_) => {
                return Err(Diagnostic::at(
                    span,
                    "a returned-view contract has no standalone type spelling",
                ))
            }
            Type::TypeParam(name) => TypeRefKind::Named {
                name: name.clone(),
                args: Vec::new(),
            },
            Type::Unit => TypeRefKind::Named {
                name: "None".into(),
                args: Vec::new(),
            },
            Type::Tuple(members) => TypeRefKind::Tuple(
                members
                    .iter()
                    .map(|member| member.source_type_ref(span))
                    .collect::<Result<Vec<_>>>()?,
            ),
            Type::Union(union) => TypeRefKind::Union(
                union
                    .members
                    .iter()
                    .map(|member| member.source_type_ref(span))
                    .collect::<Result<Vec<_>>>()?,
            ),
            Type::Function {
                params,
                return_type,
            } => TypeRefKind::Function {
                params: params
                    .iter()
                    .map(|param| {
                        Ok(FunctionTypeParam {
                            name: (!param.name.is_empty()).then(|| param.name.clone()),
                            mode: match param.passing {
                                ReceiverKind::Value => ParamMode::Own,
                                ReceiverKind::BorrowMut => ParamMode::BorrowMut,
                                ReceiverKind::Borrow => ParamMode::Default,
                            },
                            ty: param.ty.source_type_ref(span)?,
                            keyword_only: param.keyword_only,
                            has_default: param.has_default,
                            span,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
                return_type: Box::new(returned_view_pointee(return_type).source_type_ref(span)?),
                view_return: returned_view_source(params, return_type, span),
            },
            Type::Callable(callable) => {
                let signature = callable.contract().source_type_ref(span)?;
                return Ok(TypeRef::callable(
                    callable.task,
                    match callable.call_kind {
                        ClosureCallKind::Repeatable => ReceiverKind::Borrow,
                        ClosureCallKind::MutableRepeatable => ReceiverKind::BorrowMut,
                        ClosureCallKind::Consuming => ReceiverKind::Value,
                    },
                    signature,
                    span,
                ));
            }
            Type::Module(_) | Type::Closure { .. } => {
                return Err(Diagnostic::coded_at(
                    "AU2010",
                    span,
                    format!("`{self}` is not a complete source type"),
                ))
            }
        };
        Ok(TypeRef {
            kind,
            indirect: false,
            span,
        })
    }
    pub(crate) fn canonical_key(
        &self,
        module_name: &str,
        canonical_names: &BTreeMap<String, String>,
    ) -> String {
        fn key(ty: &Type, module: &str, names: &BTreeMap<String, String>) -> serde_json::Value {
            use serde_json::json;
            match ty {
                Type::Unit => json!(["~unit"]),
                Type::Module(name) => json!(["module", name]),
                Type::TypeParam(name) => json!(["parameter", name]),
                Type::ReturnedView(view) => json!([
                    "returned_view",
                    view.mutable,
                    view.origin,
                    key(&view.pointee, module, names)
                ]),
                Type::Named(name, args) => {
                    let resolved = names.get(name).unwrap_or(name);
                    let nominal =
                        if is_builtin_type(resolved) || resolved.contains('.') || module.is_empty()
                        {
                            resolved.clone()
                        } else {
                            format!("{module}.{resolved}")
                        };
                    json!([
                        "named",
                        nominal,
                        args.iter()
                            .map(|arg| key(arg, module, names))
                            .collect::<Vec<_>>()
                    ])
                }
                Type::Tuple(elements) => json!([
                    "tuple",
                    elements
                        .iter()
                        .map(|arg| key(arg, module, names))
                        .collect::<Vec<_>>()
                ]),
                Type::Union(union) => json!([
                    "union",
                    union
                        .members
                        .iter()
                        .map(|member| key(member, &union.module_name, &BTreeMap::new()))
                        .collect::<Vec<_>>()
                ]),
                Type::Function {
                    params,
                    return_type,
                } => json!([
                    "function",
                    params
                        .iter()
                        .map(|p| json!([
                            p.name,
                            p.passing,
                            p.has_default,
                            p.keyword_only,
                            key(&p.ty, module, names)
                        ]))
                        .collect::<Vec<_>>(),
                    key(return_type, module, names)
                ]),
                Type::Callable(callable) => json!([
                    "callable",
                    callable.task,
                    callable.call_kind,
                    callable
                        .params
                        .iter()
                        .map(|p| json!([
                            p.name,
                            p.passing,
                            p.has_default,
                            p.keyword_only,
                            key(&p.ty, module, names)
                        ]))
                        .collect::<Vec<_>>(),
                    key(&callable.return_type, module, names)
                ]),
                Type::Closure {
                    params,
                    return_type,
                    captures,
                    call_kind,
                } => json!([
                    "closure",
                    call_kind,
                    params
                        .iter()
                        .map(|p| json!([
                            p.name,
                            p.passing,
                            p.has_default,
                            p.keyword_only,
                            key(&p.ty, module, names)
                        ]))
                        .collect::<Vec<_>>(),
                    key(return_type, module, names),
                    captures
                        .iter()
                        .map(|capture| json!([
                            capture.name,
                            capture.mode,
                            key(&capture.ty, module, names)
                        ]))
                        .collect::<Vec<_>>()
                ]),
            }
        }
        format!(
            "aura-type-key-v1:{}",
            key(self, module_name, canonical_names)
        )
    }

    pub(crate) fn normalize_union(
        members: Vec<Type>,
        module_name: &str,
        canonical_names: &BTreeMap<String, String>,
    ) -> Result<Type> {
        Self::normalize_union_with_budget(
            members,
            module_name,
            canonical_names,
            &super::type_budget::ExpansionBudget::default(),
            crate::diag::Span::new(1, 1),
        )
    }

    fn normalize_union_with_budget(
        members: Vec<Type>,
        module_name: &str,
        canonical_names: &BTreeMap<String, String>,
        budget: &super::type_budget::ExpansionBudget,
        span: crate::diag::Span,
    ) -> Result<Type> {
        let mut ordered = BTreeMap::new();
        for member in members {
            if let Type::Union(union) = member {
                if union.members.len() != union.keys.len() {
                    return Err(Diagnostic::coded("AU2010", "invalid union member metadata"));
                }
                for (member, key) in union.members.into_iter().zip(union.keys) {
                    ordered.entry(key).or_insert(member);
                }
            } else {
                budget.check_canonical_key(&member, module_name, canonical_names, span)?;
                ordered
                    .entry(member.canonical_key(module_name, canonical_names))
                    .or_insert(member);
            }
        }
        match ordered.len() {
            0 => Err(Diagnostic::coded(
                "AU2010",
                "a union requires at least one member",
            )),
            1 => Ok(ordered.into_values().next().expect("one normalized member")),
            _ => {
                let (keys, members) = ordered.into_iter().unzip();
                Ok(Type::Union(Box::new(UnionType {
                    members,
                    keys,
                    module_name: module_name.to_string(),
                })))
            }
        }
    }
}

/// Renders a complete callable contract: slot names, the keyword-only
/// boundary, capabilities, types, and default availability.
fn write_contract_params(
    f: &mut fmt::Formatter<'_>,
    params: &[FunctionParamContract],
) -> fmt::Result {
    let mut boundary_written = false;
    for (index, param) in params.iter().enumerate() {
        if index > 0 {
            write!(f, ", ")?;
        }
        if param.keyword_only && !boundary_written {
            write!(f, "*, ")?;
            boundary_written = true;
        }
        if !param.name.is_empty() {
            write!(f, "{}: ", param.name)?;
        }
        match param.passing {
            ReceiverKind::Borrow => {}
            ReceiverKind::BorrowMut => write!(f, "mut ")?,
            ReceiverKind::Value => write!(f, "own ")?,
        }
        write!(f, "{}", param.ty)?;
        if param.has_default {
            write!(f, " = ...")?;
        }
    }
    Ok(())
}

/// Wraps a callable contract's result in its argument-origin view contract
/// (C9) when the written type names one. The checker's type lowering has
/// already rejected an origin that is not a named parameter, so a later
/// lowering of the same reference falls back to the plain result only for
/// source it never sees checked.
pub(crate) fn wrap_returned_view(
    params: &[FunctionParamContract],
    return_type: Type,
    view_return: Option<&crate::ast::ViewReturn>,
) -> Type {
    let Some(view_return) = view_return else {
        return return_type;
    };
    let Some(origin) = params
        .iter()
        .position(|param| !param.name.is_empty() && param.name == view_return.origin)
    else {
        return return_type;
    };
    Type::ReturnedView(Box::new(ReturnedViewType {
        mutable: view_return.mutable,
        pointee: return_type,
        origin,
    }))
}

/// The value type a call through a callable contract produces: the pointee
/// of a returned-view contract, or the result itself.
pub(crate) fn returned_view_pointee(return_type: &Type) -> &Type {
    match return_type {
        Type::ReturnedView(view) => &view.pointee,
        other => other,
    }
}

/// The written `-> view [mut] T from name` form of a returned-view result.
pub(crate) fn returned_view_source(
    params: &[FunctionParamContract],
    return_type: &Type,
    span: crate::diag::Span,
) -> Option<crate::ast::ViewReturn> {
    let Type::ReturnedView(view) = return_type else {
        return None;
    };
    Some(crate::ast::ViewReturn {
        mutable: view.mutable,
        origin: params
            .get(view.origin)
            .map(|param| param.name.clone())
            .unwrap_or_default(),
        span,
    })
}

/// Writes `) -> R`, parenthesizing a union result, or `) -> view [mut] T
/// from name` for a stored argument-origin view contract (C9).
fn write_return_contract(
    f: &mut fmt::Formatter<'_>,
    params: &[FunctionParamContract],
    return_type: &Type,
) -> fmt::Result {
    match return_type {
        Type::ReturnedView(view) => {
            let origin = params
                .get(view.origin)
                .map(|param| param.name.as_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("?");
            write!(
                f,
                ") -> view {}{} from {origin}",
                if view.mutable { "mut " } else { "" },
                view.pointee
            )
        }
        Type::Union(_) => write!(f, ") -> ({return_type})"),
        _ => write!(f, ") -> {return_type}"),
    }
}

/// The call kind a written `Callable[...]` prefix denotes: bare `def` is
/// Shared, `mut def` Mutable, `own def` Consuming (C1).
pub(crate) fn closure_call_kind_for(call_kind: crate::ast::ReceiverKind) -> ClosureCallKind {
    match call_kind {
        crate::ast::ReceiverKind::Borrow => ClosureCallKind::Repeatable,
        crate::ast::ReceiverKind::BorrowMut => ClosureCallKind::MutableRepeatable,
        crate::ast::ReceiverKind::Value => ClosureCallKind::Consuming,
    }
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Union(left), Self::Union(right)) => left.keys == right.keys,
            (Self::Named(left_name, left_args), Self::Named(right_name, right_args)) => {
                left_name == right_name && left_args == right_args
            }
            (Self::Tuple(left), Self::Tuple(right)) => left == right,
            (
                Self::Function {
                    params: left_params,
                    return_type: left_return,
                },
                Self::Function {
                    params: right_params,
                    return_type: right_return,
                },
            ) => {
                left_params.len() == right_params.len()
                    && left_params
                        .iter()
                        .zip(right_params.iter())
                        .all(|(left, right)| left.ty == right.ty && left.passing == right.passing)
                    && left_return == right_return
            }
            (
                Self::Closure {
                    params: left_params,
                    return_type: left_return,
                    captures: left_captures,
                    call_kind: left_call_kind,
                },
                Self::Closure {
                    params: right_params,
                    return_type: right_return,
                    captures: right_captures,
                    call_kind: right_call_kind,
                },
            ) => {
                left_params.len() == right_params.len()
                    && left_params
                        .iter()
                        .zip(right_params.iter())
                        .all(|(left, right)| left.ty == right.ty && left.passing == right.passing)
                    && left_return == right_return
                    && left_captures == right_captures
                    && left_call_kind == right_call_kind
            }
            (Self::Callable(left), Self::Callable(right)) => {
                left.task == right.task
                    && left.call_kind == right.call_kind
                    && left.params.len() == right.params.len()
                    && left
                        .params
                        .iter()
                        .zip(right.params.iter())
                        .all(|(left, right)| left.ty == right.ty && left.passing == right.passing)
                    && left.return_type == right.return_type
            }
            (Self::ReturnedView(left), Self::ReturnedView(right)) => {
                left.mutable == right.mutable
                    && left.origin == right.origin
                    && left.pointee == right.pointee
            }
            (Self::TypeParam(left), Self::TypeParam(right))
            | (Self::Module(left), Self::Module(right)) => left == right,
            (Self::Unit, Self::Unit) => true,
            _ => false,
        }
    }
}

impl Eq for Type {}

impl Type {
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into(), Vec::new())
    }

    pub fn is_copy(&self) -> bool {
        match self {
            Type::Union(_) => false,
            Type::Unit => true,
            Type::Module(_) => false,
            Type::ReturnedView(_) => false,
            Type::TypeParam(_) => false,
            Type::Tuple(elements) => elements.iter().all(Type::is_copy),
            Type::Function { .. } => true,
            Type::Closure { .. } | Type::Callable(_) => false,
            Type::Named(name, args) if name == "Task" && args.len() == 1 => args[0].is_copy(),
            Type::Named(name, args) => is_builtin_copy_named_type(name, args),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Union(union) => {
                for (index, member) in union.members.iter().enumerate() {
                    if index != 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{member}")?;
                }
                Ok(())
            }
            Type::Unit => write!(f, "None"),
            Type::Module(name) => write!(f, "module {}", name),
            Type::ReturnedView(view) => write!(
                f,
                "view {}{} from #{}",
                if view.mutable { "mut " } else { "" },
                view.pointee,
                view.origin
            ),
            Type::TypeParam(name) => write!(f, "{}", name),
            Type::Tuple(elements) => {
                write!(f, "(")?;
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", element)?;
                }
                if elements.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }
            Type::Function {
                params,
                return_type,
            } => {
                write!(f, "def(")?;
                write_contract_params(f, params)?;
                write_return_contract(f, params, return_type)
            }
            Type::Callable(callable) => {
                write!(
                    f,
                    "{}[{}def(",
                    callable.constructor_name(),
                    callable.call_kind.spelling()
                )?;
                write_contract_params(f, &callable.params)?;
                write_return_contract(f, &callable.params, &callable.return_type)?;
                write!(f, "]")
            }
            Type::Closure {
                params,
                return_type,
                call_kind,
                ..
            } => {
                match call_kind {
                    ClosureCallKind::Consuming => write!(f, "consuming ")?,
                    ClosureCallKind::MutableRepeatable => write!(f, "mutable ")?,
                    ClosureCallKind::Repeatable => {}
                }
                write!(f, "closure def(")?;
                write_contract_params(f, params)?;
                write_return_contract(f, params, return_type)
            }
            Type::Named(name, args) if args.is_empty() => write!(f, "{}", name),
            Type::Named(name, args) => {
                write!(f, "{}[", name)?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, "]")
            }
        }
    }
}

pub(super) fn lower_type(
    type_ref: &TypeRef,
    type_names: &TypeDefinitions,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_params: &BTreeMap<String, ()>,
) -> Result<Type> {
    lower_type_with_self(
        type_ref,
        type_names,
        type_arities,
        canonical_type_names,
        type_params,
        None,
    )
}

pub(super) fn lower_type_with_self(
    type_ref: &TypeRef,
    type_names: &TypeDefinitions,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_params: &BTreeMap<String, ()>,
    self_type: Option<&Type>,
) -> Result<Type> {
    let (name, type_args) = match &type_ref.kind {
        crate::ast::TypeRefKind::Union(members) => {
            let members = members
                .iter()
                .map(|member| {
                    let invalid = || {
                        let label = match &member.kind {
                            crate::ast::TypeRefKind::Named { name, .. } => name.as_str(),
                            _ => "type expression",
                        };
                        Diagnostic::coded_at(
                            "AU2010",
                            member.span,
                            format!("invalid or incomplete union member `{label}`"),
                        )
                    };
                    if let crate::ast::TypeRefKind::Named { name, .. } = &member.kind {
                        if !type_params.contains_key(name)
                            && type_names.non_value_names.contains(name)
                        {
                            return Err(invalid());
                        }
                    }
                    lower_type_with_self(
                        member,
                        type_names,
                        type_arities,
                        canonical_type_names,
                        type_params,
                        self_type,
                    )
                    .map_err(|error| {
                        if error.code == "AU2002" {
                            invalid()
                        } else {
                            error
                        }
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            return Type::normalize_union_with_budget(
                members,
                &type_names.module_name,
                canonical_type_names,
                &type_names.budget,
                type_ref.span,
            );
        }
        crate::ast::TypeRefKind::Callable {
            task,
            call_kind,
            signature,
        } => {
            let contract = lower_type_with_self(
                signature,
                type_names,
                type_arities,
                canonical_type_names,
                type_params,
                self_type,
            )?;
            let Type::Function {
                params,
                return_type,
            } = contract
            else {
                return Err(Diagnostic::at(
                    type_ref.span,
                    "an owned callable type wraps a `def(...) -> ...` contract",
                ));
            };
            if *task && matches!(return_type.as_ref(), Type::ReturnedView(_)) {
                return Err(Diagnostic::coded_at(
                    "AU3008",
                    type_ref.span,
                    "a task callable cannot return a view; the child's result must be an owned value",
                ));
            }
            return Ok(Type::Callable(Box::new(CallableType {
                task: *task,
                call_kind: closure_call_kind_for(*call_kind),
                params,
                return_type: *return_type,
            })));
        }
        crate::ast::TypeRefKind::Tuple(elements) => {
            return elements
                .iter()
                .map(|element| {
                    lower_type_with_self(
                        element,
                        type_names,
                        type_arities,
                        canonical_type_names,
                        type_params,
                        self_type,
                    )
                })
                .collect::<Result<Vec<_>>>()
                .map(Type::Tuple);
        }
        crate::ast::TypeRefKind::Function {
            params,
            return_type,
            view_return,
        } => {
            if let Some(view_return) = view_return {
                if view_return.origin == "self" {
                    return Err(Diagnostic::coded_at(
                        "AU3010",
                        view_return.span,
                        "a function type cannot return a view from `self`; only one named parameter can be a stored callable's view origin",
                    ));
                }
                let origin_param = params
                    .iter()
                    .find(|param| param.name.as_deref() == Some(view_return.origin.as_str()));
                match origin_param {
                    None => {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            view_return.span,
                            format!(
                                "returned-view origin `{}` is not a named parameter of this function type",
                                view_return.origin
                            ),
                        ))
                    }
                    Some(param) if view_return.mutable && param.mode != crate::ast::ParamMode::BorrowMut => {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            view_return.span,
                            format!(
                                "a `view mut` result requires its origin parameter `{}` to be `mut`",
                                view_return.origin
                            ),
                        ))
                    }
                    Some(_) => {}
                }
            }
            let params = params
                .iter()
                .map(|param| {
                    let ty = lower_type_with_self(
                        &param.ty,
                        type_names,
                        type_arities,
                        canonical_type_names,
                        type_params,
                        self_type,
                    )?;
                    Ok(FunctionParamContract {
                        keyword_only: param.keyword_only,
                        name: param.name.clone().unwrap_or_default(),
                        ty,
                        passing: resolve_param_passing(param.mode),
                        has_default: param.has_default,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let return_type = lower_type_with_self(
                return_type,
                type_names,
                type_arities,
                canonical_type_names,
                type_params,
                self_type,
            )?;
            let return_type = wrap_returned_view(&params, return_type, view_return.as_ref());
            return Ok(Type::Function {
                params,
                return_type: Box::new(return_type),
            });
        }
        crate::ast::TypeRefKind::Named { name, args } => (name, args),
    };
    let type_name = match name.as_str() {
        "str" => "str",
        "int" => "int64",
        name => name,
    };

    if type_name == "Self" {
        if !type_args.is_empty() {
            return Err(Diagnostic::at(
                type_ref.span,
                "`Self` does not take generic arguments",
            ));
        }
        let Some(self_type) = self_type else {
            return Err(Diagnostic::at(
                type_ref.span,
                "`Self` is only available inside class methods, trait methods, and impl methods",
            ));
        };
        return Ok(self_type.clone());
    }

    if type_params.contains_key(type_name) {
        if !type_args.is_empty() {
            return Err(Diagnostic::at(
                type_ref.span,
                format!(
                    "type parameter `{}` does not take type arguments",
                    type_name
                ),
            ));
        }
        return Ok(Type::TypeParam(type_name.to_string()));
    }

    if type_name == "None" {
        if !type_args.is_empty() {
            return Err(Diagnostic::at(
                type_ref.span,
                "`None` does not take generic arguments",
            ));
        }
        return Ok(Type::Unit);
    }

    let args = type_args
        .iter()
        .map(|arg| {
            lower_type_with_self(
                arg,
                type_names,
                type_arities,
                canonical_type_names,
                type_params,
                self_type,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    if let Some(alias) = type_names.imported_aliases.get(type_name) {
        if args.len() != alias.decl.type_params.len() {
            return Err(Diagnostic::coded_at(
                "AU2002",
                type_ref.span,
                format!(
                    "`{type_name}` expects exactly {} type arguments, found {}",
                    alias.decl.type_params.len(),
                    args.len()
                ),
            ));
        }
        let _depth = type_names.budget.enter(type_ref.span)?;
        let substitutions = alias.decl.type_params.iter().cloned().zip(args).collect();
        type_names
            .budget
            .check_substitution(&alias.target, &substitutions, type_ref.span)?;
        type_names.budget.check_substitution_key(
            &alias.target,
            &substitutions,
            &type_names.module_name,
            canonical_type_names,
            type_ref.span,
        )?;
        return Ok(substitute_alias_type(
            &alias.target,
            &substitutions,
            &type_names.module_name,
            canonical_type_names,
        ));
    }
    if let Some(alias) = type_names.aliases.get(type_name) {
        if args.len() != alias.type_params.len() {
            return Err(Diagnostic::coded_at(
                "AU2002",
                type_ref.span,
                format!(
                    "`{type_name}` expects exactly {} type argument{}, found {}",
                    alias.type_params.len(),
                    if alias.type_params.len() == 1 {
                        ""
                    } else {
                        "s"
                    },
                    args.len()
                ),
            ));
        }
        let _depth = type_names.budget.enter(type_ref.span)?;
        let substitutions = alias.type_params.iter().cloned().zip(args).collect();
        {
            let templates = type_names.alias_templates.borrow();
            if let Some(template) = templates.get(type_name) {
                type_names
                    .budget
                    .check_substitution(template, &substitutions, type_ref.span)?;
                type_names.budget.check_substitution_key(
                    template,
                    &substitutions,
                    &type_names.module_name,
                    canonical_type_names,
                    type_ref.span,
                )?;
                return Ok(substitute_alias_type(
                    template,
                    &substitutions,
                    &type_names.module_name,
                    canonical_type_names,
                ));
            }
        }
        let template = lower_type_with_self(
            &alias.target,
            type_names,
            type_arities,
            canonical_type_names,
            &type_param_scope(&alias.type_params),
            None,
        )?;
        type_names
            .budget
            .check_substitution(&template, &HashMap::new(), type_ref.span)?;
        type_names
            .budget
            .check_substitution(&template, &substitutions, type_ref.span)?;
        type_names.budget.check_substitution_key(
            &template,
            &substitutions,
            &type_names.module_name,
            canonical_type_names,
            type_ref.span,
        )?;
        let result = substitute_alias_type(
            &template,
            &substitutions,
            &type_names.module_name,
            canonical_type_names,
        );
        type_names
            .alias_templates
            .borrow_mut()
            .insert(type_name.to_string(), template);
        return Ok(result);
    }

    if type_name == "Option" {
        if args.len() != 1 {
            return Err(Diagnostic::at(
                type_ref.span,
                "`Option` expects exactly one type argument",
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if type_name == "Result" {
        if args.len() != 2 {
            return Err(Diagnostic::at(
                type_ref.span,
                "`Result` expects exactly two type arguments",
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if type_name == "Queue"
        || type_name == "Task"
        || type_name == "SendError"
        || type_name == "QueueReceive"
        || type_name == "TaskResult"
        || type_name == "WaitAny"
        || type_name == "WaitAll"
        || type_name == "list"
        || type_name == "set"
        || type_name == "Array"
    {
        if args.len() != 1 {
            return Err(Diagnostic::at(
                type_ref.span,
                format!("`{}` expects exactly one type argument", type_name),
            ));
        }
        if type_name == "Array" && !is_array_dtype(&args[0]) {
            return Err(Diagnostic::coded_at(
                "AU2002",
                type_ref.span,
                format!(
                    "Array dtype must be one of `int32`, `int64`, `float32`, or `float64`, found `{}`",
                    args[0]
                ),
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if type_name == "SelectOutcome" {
        if args.len() != 2 {
            return Err(Diagnostic::at(
                type_ref.span,
                "`SelectOutcome` expects exactly two type arguments",
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if type_name == "dict" {
        if args.len() != 2 {
            return Err(Diagnostic::at(
                type_ref.span,
                "`dict` expects exactly two type arguments",
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if type_name == "TaskGroup" || type_name == "Duration" {
        if !args.is_empty() {
            return Err(Diagnostic::at(
                type_ref.span,
                format!("`{}` does not take type arguments", type_name),
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if let Some(expected_arity) = type_arities.get(type_name) {
        if args.len() != *expected_arity {
            return Err(Diagnostic::at(
                type_ref.span,
                format!(
                    "`{}` expects exactly {} type argument{}, found {}",
                    type_name,
                    expected_arity,
                    if *expected_arity == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
        }
    } else if (is_builtin_type(type_name) || type_names.contains_key(type_name)) && !args.is_empty()
    {
        return Err(Diagnostic::at(
            type_ref.span,
            format!("`{}` does not take type arguments", type_name),
        ));
    }

    if is_builtin_type(type_name) || type_names.contains_key(type_name) {
        let canonical_name = if preserves_qualified_builtin_type_name(type_name) {
            type_name.to_string()
        } else if let Some(canonical_name) = canonical_type_names.get(type_name) {
            canonical_name.clone()
        } else {
            type_name.to_string()
        };
        Ok(Type::Named(canonical_name, args))
    } else {
        Err(Diagnostic::at(
            type_ref.span,
            format!("unknown type `{}`", name),
        ))
    }
}

pub(super) fn type_param_scope(type_params: &[String]) -> BTreeMap<String, ()> {
    type_params
        .iter()
        .cloned()
        .map(|name| (name, ()))
        .collect::<BTreeMap<_, _>>()
}

pub(super) fn merged_type_param_scope(
    parent: &BTreeMap<String, ()>,
    added: &[String],
) -> BTreeMap<String, ()> {
    let mut merged = parent.clone();
    for name in added {
        merged.insert(name.clone(), ());
    }
    merged
}

pub(super) fn collect_type_ref_type_params(
    type_ref: &TypeRef,
    type_names: &TypeDefinitions,
    collected: &mut BTreeSet<String>,
    include_self: bool,
) {
    match &type_ref.kind {
        crate::ast::TypeRefKind::Callable { signature, .. } => {
            collect_type_ref_type_params(signature, type_names, collected, true);
        }
        crate::ast::TypeRefKind::Tuple(elements) | crate::ast::TypeRefKind::Union(elements) => {
            for element in elements {
                collect_type_ref_type_params(element, type_names, collected, true);
            }
        }
        crate::ast::TypeRefKind::Function {
            params,
            return_type,
            ..
        } => {
            for param in params {
                collect_type_ref_type_params(&param.ty, type_names, collected, true);
            }
            collect_type_ref_type_params(return_type, type_names, collected, true);
        }
        crate::ast::TypeRefKind::Named { name, args } => {
            if include_self
                && args.is_empty()
                && !type_ref.indirect
                // `None` is the surface spelling of the unit type, not an
                // undeclared impl type parameter. It is lowered specially
                // before the ordinary builtin-type path.
                && name != "None"
                && !is_builtin_type(name)
                && !type_names.contains_key(name)
            {
                collected.insert(name.clone());
            }
            for arg in args {
                collect_type_ref_type_params(arg, type_names, collected, true);
            }
        }
    }
}

pub(crate) fn substitute_type(ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
    substitute_type_in_context(ty, substitutions, None)
}

pub(crate) fn substitute_alias_type(
    ty: &Type,
    substitutions: &HashMap<String, Type>,
    module_name: &str,
    canonical_names: &BTreeMap<String, String>,
) -> Type {
    substitute_type_in_context(ty, substitutions, Some((module_name, canonical_names)))
}

fn substitute_type_in_context(
    ty: &Type,
    substitutions: &HashMap<String, Type>,
    context: Option<(&str, &BTreeMap<String, String>)>,
) -> Type {
    match ty {
        Type::Union(union) => {
            let empty = BTreeMap::new();
            let (module, names) = context.unwrap_or((&union.module_name, &empty));
            Type::normalize_union(
                union
                    .members
                    .iter()
                    .map(|member| substitute_type_in_context(member, substitutions, context))
                    .collect(),
                module,
                names,
            )
            .expect("substitution preserves nonempty normalized union members")
        }
        Type::Unit => Type::Unit,
        Type::Module(name) => Type::Module(name.clone()),
        Type::ReturnedView(view) => Type::ReturnedView(Box::new(ReturnedViewType {
            mutable: view.mutable,
            pointee: substitute_type_in_context(&view.pointee, substitutions, context),
            origin: view.origin,
        })),
        Type::TypeParam(name) => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| Type::TypeParam(name.clone())),
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| substitute_type_in_context(element, substitutions, context))
                .collect(),
        ),
        Type::Function {
            params,
            return_type,
        } => Type::Function {
            params: params
                .iter()
                .map(|param| FunctionParamContract {
                    keyword_only: param.keyword_only,
                    name: param.name.clone(),
                    ty: substitute_type_in_context(&param.ty, substitutions, context),
                    passing: param.passing,
                    has_default: param.has_default,
                })
                .collect(),
            return_type: Box::new(substitute_type_in_context(
                return_type,
                substitutions,
                context,
            )),
        },
        Type::Callable(callable) => Type::Callable(Box::new(CallableType {
            task: callable.task,
            call_kind: callable.call_kind,
            params: callable
                .params
                .iter()
                .map(|param| FunctionParamContract {
                    keyword_only: param.keyword_only,
                    name: param.name.clone(),
                    ty: substitute_type_in_context(&param.ty, substitutions, context),
                    passing: param.passing,
                    has_default: param.has_default,
                })
                .collect(),
            return_type: substitute_type_in_context(&callable.return_type, substitutions, context),
        })),
        Type::Closure {
            params,
            return_type,
            captures,
            call_kind,
        } => Type::Closure {
            params: Box::new(
                params
                    .iter()
                    .map(|param| FunctionParamContract {
                        keyword_only: param.keyword_only,
                        name: param.name.clone(),
                        ty: substitute_type_in_context(&param.ty, substitutions, context),
                        passing: param.passing,
                        has_default: param.has_default,
                    })
                    .collect(),
            ),
            return_type: Box::new(substitute_type_in_context(
                return_type,
                substitutions,
                context,
            )),
            captures: Box::new(
                captures
                    .iter()
                    .map(|capture| ClosureCapture {
                        name: capture.name.clone(),
                        ty: substitute_type_in_context(&capture.ty, substitutions, context),
                        mode: capture.mode,
                        span: capture.span,
                        mutated: capture.mutated,
                    })
                    .collect(),
            ),
            call_kind: *call_kind,
        },
        Type::Named(name, args) => {
            let substituted_args = args
                .iter()
                .map(|arg| substitute_type_in_context(arg, substitutions, context))
                .collect::<Vec<_>>();
            Type::Named(name.clone(), substituted_args)
        }
    }
}

pub(crate) fn substitute_trait_bound(
    bound: &TraitBound,
    substitutions: &HashMap<String, Type>,
) -> TraitBound {
    TraitBound {
        trait_name: bound.trait_name.clone(),
        trait_args: bound
            .trait_args
            .iter()
            .map(|arg| substitute_type(arg, substitutions))
            .collect(),
    }
}

pub(super) fn substitute_trait_bounds(
    bounds: &BTreeMap<String, Vec<TraitBound>>,
    substitutions: &HashMap<String, Type>,
) -> BTreeMap<String, Vec<TraitBound>> {
    bounds
        .iter()
        .map(|(type_param, type_bounds)| {
            (
                type_param.clone(),
                type_bounds
                    .iter()
                    .map(|bound| substitute_trait_bound(bound, substitutions))
                    .collect(),
            )
        })
        .collect()
}

pub(super) fn collect_type_params_from_type(ty: &Type, collected: &mut BTreeSet<String>) {
    match ty {
        Type::Union(union) => {
            for member in &union.members {
                collect_type_params_from_type(member, collected);
            }
        }
        Type::TypeParam(name) => {
            collected.insert(name.clone());
        }
        Type::Named(_, args) => {
            for arg in args {
                collect_type_params_from_type(arg, collected);
            }
        }
        Type::Tuple(elements) => {
            for element in elements {
                collect_type_params_from_type(element, collected);
            }
        }
        Type::Function {
            params,
            return_type,
            ..
        } => {
            for param in params {
                collect_type_params_from_type(&param.ty, collected);
            }
            collect_type_params_from_type(return_type, collected);
        }
        Type::Callable(callable) => {
            for param in &callable.params {
                collect_type_params_from_type(&param.ty, collected);
            }
            collect_type_params_from_type(&callable.return_type, collected);
        }
        Type::Closure {
            params,
            return_type,
            captures,
            ..
        } => {
            for param in params.iter() {
                collect_type_params_from_type(&param.ty, collected);
            }
            collect_type_params_from_type(return_type, collected);
            for capture in captures.iter() {
                collect_type_params_from_type(&capture.ty, collected);
            }
        }
        Type::Unit | Type::Module(_) => {}
        Type::ReturnedView(view) => collect_type_params_from_type(&view.pointee, collected),
    }
}

pub(crate) fn type_pattern_specificity(ty: &Type) -> usize {
    match ty {
        Type::Union(union) => {
            1 + union
                .members
                .iter()
                .map(type_pattern_specificity)
                .sum::<usize>()
        }
        Type::TypeParam(_) => 0,
        Type::Named(_, args) => 1 + args.iter().map(type_pattern_specificity).sum::<usize>(),
        Type::Tuple(elements) => 1 + elements.iter().map(type_pattern_specificity).sum::<usize>(),
        Type::Function {
            params,
            return_type,
            ..
        } => {
            1 + params
                .iter()
                .map(|param| type_pattern_specificity(&param.ty))
                .sum::<usize>()
                + type_pattern_specificity(return_type)
        }
        Type::Callable(callable) => {
            1 + callable
                .params
                .iter()
                .map(|param| type_pattern_specificity(&param.ty))
                .sum::<usize>()
                + type_pattern_specificity(&callable.return_type)
        }
        Type::Closure {
            params,
            return_type,
            ..
        } => {
            1 + params
                .iter()
                .map(|param| type_pattern_specificity(&param.ty))
                .sum::<usize>()
                + type_pattern_specificity(return_type)
        }
        Type::Module(_) | Type::Unit => 1,
        Type::ReturnedView(view) => 1 + type_pattern_specificity(&view.pointee),
    }
}

pub(crate) fn trait_impl_specificity_parts(for_type: &Type, trait_args: &[Type]) -> usize {
    type_pattern_specificity(for_type)
        + trait_args
            .iter()
            .map(type_pattern_specificity)
            .sum::<usize>()
}

pub(crate) fn trait_impl_specificity(trait_impl: &TraitImplInfo) -> usize {
    trait_impl_specificity_parts(&trait_impl.for_type, &trait_impl.trait_args)
}

pub(crate) fn type_pattern_matches(
    pattern: &Type,
    actual: &Type,
    type_params: &BTreeSet<String>,
    substitutions: &mut HashMap<String, Type>,
) -> bool {
    match pattern {
        Type::Union(union) => {
            let mut trial = substitutions.clone();
            match unify_union_pattern(union, actual, &mut trial, Some(type_params)) {
                Ok(()) => {
                    *substitutions = trial;
                    true
                }
                Err(_) => false,
            }
        }
        Type::TypeParam(name) if type_params.contains(name) => {
            if let Some(existing) = substitutions.get(name) {
                // Later evidence must be admitted by the first observation's
                // complete callable contract; no common contract is invented.
                existing == actual && check_callable_positions(existing, actual).is_ok()
            } else {
                substitutions.insert(name.clone(), actual.clone());
                true
            }
        }
        Type::TypeParam(_) => pattern == actual,
        Type::ReturnedView(view) => matches!(
            actual,
            Type::ReturnedView(other)
                if other.mutable == view.mutable
                    && other.origin == view.origin
                    && type_pattern_matches(&view.pointee, &other.pointee, type_params, substitutions)
        ),
        Type::Named(name, pattern_args) => {
            let Type::Named(actual_name, actual_args) = actual else {
                return false;
            };
            if name != actual_name || pattern_args.len() != actual_args.len() {
                return false;
            }
            pattern_args
                .iter()
                .zip(actual_args.iter())
                .all(|(pattern_arg, actual_arg)| {
                    type_pattern_matches(pattern_arg, actual_arg, type_params, substitutions)
                })
        }
        Type::Tuple(pattern_elements) => {
            let Type::Tuple(actual_elements) = actual else {
                return false;
            };
            pattern_elements.len() == actual_elements.len()
                && pattern_elements
                    .iter()
                    .zip(actual_elements)
                    .all(|(pattern, actual)| {
                        type_pattern_matches(pattern, actual, type_params, substitutions)
                    })
        }
        Type::Function {
            params,
            return_type,
        } => {
            let Type::Function {
                params: actual_params,
                return_type: actual_return,
            } = actual
            else {
                return false;
            };
            params.len() == actual_params.len()
                && params
                    .iter()
                    .zip(actual_params.iter())
                    .all(|(pattern, actual)| {
                        pattern.passing == actual.passing
                            && type_pattern_matches(
                                &pattern.ty,
                                &actual.ty,
                                type_params,
                                substitutions,
                            )
                    })
                && type_pattern_matches(return_type, actual_return, type_params, substitutions)
        }
        Type::Callable(callable) => {
            let Type::Callable(actual_callable) = actual else {
                return false;
            };
            callable.task == actual_callable.task
                && callable.call_kind == actual_callable.call_kind
                && callable.params.len() == actual_callable.params.len()
                && callable
                    .params
                    .iter()
                    .zip(actual_callable.params.iter())
                    .all(|(pattern, actual)| {
                        pattern.passing == actual.passing
                            && type_pattern_matches(
                                &pattern.ty,
                                &actual.ty,
                                type_params,
                                substitutions,
                            )
                    })
                && type_pattern_matches(
                    &callable.return_type,
                    &actual_callable.return_type,
                    type_params,
                    substitutions,
                )
        }
        Type::Closure {
            params,
            return_type,
            captures,
            call_kind,
        } => {
            let Type::Closure {
                params: actual_params,
                return_type: actual_return,
                captures: actual_captures,
                call_kind: actual_call_kind,
            } = actual
            else {
                return false;
            };
            call_kind == actual_call_kind
                && captures.len() == actual_captures.len()
                && captures
                    .iter()
                    .zip(actual_captures.iter())
                    .all(|(pattern, actual)| {
                        pattern.name == actual.name
                            && pattern.mode == actual.mode
                            && type_pattern_matches(
                                &pattern.ty,
                                &actual.ty,
                                type_params,
                                substitutions,
                            )
                    })
                && params.len() == actual_params.len()
                && params
                    .iter()
                    .zip(actual_params.iter())
                    .all(|(pattern, actual)| {
                        pattern.passing == actual.passing
                            && type_pattern_matches(
                                &pattern.ty,
                                &actual.ty,
                                type_params,
                                substitutions,
                            )
                    })
                && type_pattern_matches(return_type, actual_return, type_params, substitutions)
        }
        Type::Module(path) => matches!(actual, Type::Module(actual_path) if actual_path == path),
        Type::Unit => *actual == Type::Unit,
    }
}

pub(crate) fn has_unresolved_type_params(ty: &Type) -> bool {
    match ty {
        Type::Union(union) => union.members.iter().any(has_unresolved_type_params),
        Type::Unit => false,
        Type::Module(_) => false,
        Type::ReturnedView(view) => has_unresolved_type_params(&view.pointee),
        Type::TypeParam(_) => true,
        Type::Tuple(elements) => elements.iter().any(has_unresolved_type_params),
        Type::Function {
            params,
            return_type,
            ..
        } => {
            params
                .iter()
                .any(|param| has_unresolved_type_params(&param.ty))
                || has_unresolved_type_params(return_type)
        }
        Type::Callable(callable) => {
            callable
                .params
                .iter()
                .any(|param| has_unresolved_type_params(&param.ty))
                || has_unresolved_type_params(&callable.return_type)
        }
        Type::Closure {
            params,
            return_type,
            captures,
            ..
        } => {
            params
                .iter()
                .any(|param| has_unresolved_type_params(&param.ty))
                || has_unresolved_type_params(return_type)
                || captures
                    .iter()
                    .any(|capture| has_unresolved_type_params(&capture.ty))
        }
        Type::Named(_, args) => args.iter().any(has_unresolved_type_params),
    }
}

pub(crate) fn substitutions_from_decl_type_args(
    type_params: &[String],
    actual_args: &[Type],
) -> HashMap<String, Type> {
    type_params
        .iter()
        .cloned()
        .zip(actual_args.iter().cloned())
        .collect()
}

/// Unifies a union pattern that may still mention type parameters with an
/// actual type (ADR-0052 A7). Concrete pattern members must be members of
/// the actual type after the known substitutions are applied; the remaining
/// actual members bind the single unbound parameter member to their
/// normalized union. Two unbound parameters cannot be told apart from a
/// union argument, and an empty remainder leaves the parameter unresolved;
/// both require explicit specialization rather than inverted normalization.
pub(super) fn unify_union_pattern(
    pattern: &UnionType,
    actual: &Type,
    substitutions: &mut HashMap<String, Type>,
    inferable: Option<&BTreeSet<String>>,
) -> Result<()> {
    let rendered_pattern = Type::Union(Box::new(pattern.clone()));
    let substituted = substitute_type(&rendered_pattern, substitutions);
    let mut concrete: Vec<Type> = Vec::new();
    let mut params: Vec<String> = Vec::new();
    for member in &pattern.members {
        match member {
            Type::TypeParam(name)
                if !substitutions.contains_key(name)
                    && inferable.is_none_or(|names| names.contains(name)) =>
            {
                if !params.contains(name) {
                    params.push(name.clone());
                }
            }
            other => {
                let resolved = substitute_type(other, substitutions);
                match resolved {
                    Type::Union(inner) => concrete.extend(inner.members.iter().cloned()),
                    resolved => concrete.push(resolved),
                }
            }
        }
    }
    let (actual_members, actual_keys): (Vec<Type>, Vec<String>) = match actual {
        Type::Union(actual_union) => (actual_union.members.clone(), actual_union.keys.clone()),
        other => (vec![other.clone()], vec![String::new()]),
    };
    let mut remainder_members = Vec::new();
    let mut remainder_keys = Vec::new();
    for (member, key) in actual_members.iter().zip(actual_keys.iter()) {
        if concrete.iter().any(|candidate| candidate == member) {
            continue;
        }
        remainder_members.push(member.clone());
        remainder_keys.push(key.clone());
    }
    if params.is_empty() {
        // Every parameter member is already bound (or belongs to an enclosing
        // scope): the substituted pattern must match exactly.
        return if &substituted == actual {
            Ok(())
        } else {
            Err(Diagnostic::new(format!(
                "expected `{substituted}`, found `{actual}`"
            )))
        };
    }
    if params.len() > 1 && !remainder_members.is_empty() {
        return Err(Diagnostic::coded(
            "AU2010",
            format!(
                "cannot infer `{}` from `{actual}`; the union members cannot be assigned to more than one type parameter, so specialize the callable explicitly",
                params.join("` and `")
            ),
        ));
    }
    if remainder_members.is_empty() {
        return Err(Diagnostic::coded(
            "AU2010",
            format!(
                "cannot infer `{}` from `{actual}`; every member is already named by `{rendered_pattern}`, so specialize the callable explicitly",
                params.join("` and `")
            ),
        ));
    }
    let bound = if remainder_members.len() == 1 {
        remainder_members.remove(0)
    } else if matches!(actual, Type::Union(_)) {
        Type::Union(Box::new(UnionType {
            members: remainder_members,
            keys: remainder_keys,
            module_name: pattern.module_name.clone(),
        }))
    } else {
        unreachable!("a non-union actual contributes at most one remainder member")
    };
    let name = params.remove(0);
    let bound = match substitutions.get(&name) {
        Some(existing) if existing != &bound => {
            return Err(Diagnostic::new(format!(
                "conflicting inferred types for `{name}`: `{existing}` and `{bound}`"
            )))
        }
        Some(existing) => {
            check_callable_positions(existing, &bound).map_err(|reason| {
                Diagnostic::new(format!(
                    "conflicting inferred callable contracts for `{name}`: {reason}"
                ))
            })?;
            existing.clone()
        }
        None => bound,
    };
    substitutions.insert(name, bound);
    Ok(())
}

pub(super) fn unify_type_pattern(
    pattern: &Type,
    actual: &Type,
    substitutions: &mut HashMap<String, Type>,
) -> Result<()> {
    match pattern {
        Type::Union(union) => unify_union_pattern(union, actual, substitutions, None),
        Type::ReturnedView(view) => match actual {
            Type::ReturnedView(other)
                if other.mutable == view.mutable && other.origin == view.origin =>
            {
                unify_type_pattern(&view.pointee, &other.pointee, substitutions)
            }
            _ => Err(Diagnostic::new(format!(
                "expected `{pattern}`, found `{actual}`"
            ))),
        },
        Type::Unit => {
            if actual == &Type::Unit {
                Ok(())
            } else {
                Err(Diagnostic::new(format!(
                    "expected `None`, found `{}`",
                    actual
                )))
            }
        }
        Type::Module(name) => {
            if actual == &Type::Module(name.clone()) {
                Ok(())
            } else {
                Err(Diagnostic::new(format!(
                    "expected `module {}`, found `{}`",
                    name, actual
                )))
            }
        }
        Type::TypeParam(name) => {
            if let Some(existing) = substitutions.get(name) {
                if existing == actual {
                    check_callable_positions(existing, actual).map_err(|reason| {
                        Diagnostic::new(format!(
                            "conflicting inferred callable contracts for `{name}`: {reason}"
                        ))
                    })
                } else {
                    Err(Diagnostic::new(format!(
                        "conflicting inferred types for `{}`: `{}` and `{}`",
                        name, existing, actual
                    )))
                }
            } else {
                substitutions.insert(name.clone(), actual.clone());
                Ok(())
            }
        }
        Type::Named(name, args) => {
            let Type::Named(actual_name, actual_args) = actual else {
                return Err(Diagnostic::new(format!(
                    "expected `{}`, found `{}`",
                    pattern, actual
                )));
            };
            if name != actual_name || args.len() != actual_args.len() {
                return Err(Diagnostic::new(format!(
                    "expected `{}`, found `{}`",
                    pattern, actual
                )));
            }
            for (pattern_arg, actual_arg) in args.iter().zip(actual_args.iter()) {
                unify_type_pattern(pattern_arg, actual_arg, substitutions)?;
            }
            Ok(())
        }
        Type::Tuple(elements) => {
            let Type::Tuple(actual_elements) = actual else {
                return Err(Diagnostic::new(format!(
                    "expected `{}`, found `{}`",
                    pattern, actual
                )));
            };
            if elements.len() != actual_elements.len() {
                return Err(Diagnostic::new(format!(
                    "expected `{}`, found `{}`",
                    pattern, actual
                )));
            }
            for (element, actual_element) in elements.iter().zip(actual_elements) {
                unify_type_pattern(element, actual_element, substitutions)?;
            }
            Ok(())
        }
        Type::Function {
            params,
            return_type,
        } => {
            let (actual_params, actual_return) = match actual {
                Type::Function {
                    params,
                    return_type,
                } => (params.as_slice(), return_type.as_ref()),
                Type::Closure {
                    params,
                    return_type,
                    ..
                } => (params.as_slice(), return_type.as_ref()),
                Type::Callable(callable) => (callable.params.as_slice(), &callable.return_type),
                _ => {
                    return Err(Diagnostic::new(format!(
                        "expected `{pattern}`, found `{actual}`"
                    )))
                }
            };
            if params.len() != actual_params.len()
                || params
                    .iter()
                    .zip(actual_params.iter())
                    .any(|(expected, actual)| expected.passing != actual.passing)
            {
                return Err(Diagnostic::new(function_type_mismatch_message(
                    pattern, actual,
                )));
            }
            for (param, actual_param) in params.iter().zip(actual_params.iter()) {
                unify_type_pattern(&param.ty, &actual_param.ty, substitutions)?;
            }
            unify_type_pattern(return_type, actual_return, substitutions)
        }
        Type::Callable(callable) => {
            let Type::Callable(actual_callable) = actual else {
                return Err(Diagnostic::new(format!(
                    "expected `{pattern}`, found `{actual}`"
                )));
            };
            if callable.task != actual_callable.task
                || callable.call_kind != actual_callable.call_kind
                || callable.params.len() != actual_callable.params.len()
                || callable
                    .params
                    .iter()
                    .zip(actual_callable.params.iter())
                    .any(|(expected, actual)| expected.passing != actual.passing)
            {
                return Err(Diagnostic::new(format!(
                    "expected `{pattern}`, found `{actual}`"
                )));
            }
            for (param, actual_param) in callable.params.iter().zip(actual_callable.params.iter()) {
                unify_type_pattern(&param.ty, &actual_param.ty, substitutions)?;
            }
            unify_type_pattern(
                &callable.return_type,
                &actual_callable.return_type,
                substitutions,
            )
        }
        Type::Closure {
            params,
            return_type,
            captures,
            call_kind,
        } => {
            let Type::Closure {
                params: actual_params,
                return_type: actual_return,
                captures: actual_captures,
                call_kind: actual_call_kind,
            } = actual
            else {
                return Err(Diagnostic::new(format!(
                    "expected `{pattern}`, found `{actual}`"
                )));
            };
            if call_kind != actual_call_kind
                || params.len() != actual_params.len()
                || captures.len() != actual_captures.len()
                || params
                    .iter()
                    .zip(actual_params.iter())
                    .any(|(expected, actual)| expected.passing != actual.passing)
                || captures
                    .iter()
                    .zip(actual_captures.iter())
                    .any(|(expected, actual)| {
                        expected.name != actual.name || expected.mode != actual.mode
                    })
            {
                return Err(Diagnostic::new(format!(
                    "expected `{pattern}`, found `{actual}`"
                )));
            }
            for (param, actual_param) in params.iter().zip(actual_params.iter()) {
                unify_type_pattern(&param.ty, &actual_param.ty, substitutions)?;
            }
            for (capture, actual_capture) in captures.iter().zip(actual_captures.iter()) {
                unify_type_pattern(&capture.ty, &actual_capture.ty, substitutions)?;
            }
            unify_type_pattern(return_type, actual_return, substitutions)
        }
    }
}

/// Payload shapes of the retained built-in nominal enums. Shared by checking
/// and MIR metadata so backends consume one structural representation.
pub(crate) fn builtin_enum_variants(ty: &Type) -> Option<Vec<(String, Vec<Type>)>> {
    match ty {
        Type::Named(name, args) if name == "Option" && args.len() == 1 => Some(vec![
            ("Some".to_string(), vec![args[0].clone()]),
            ("None".to_string(), Vec::new()),
        ]),
        Type::Named(name, args) if name == "Result" && args.len() == 2 => Some(vec![
            ("Ok".to_string(), vec![args[0].clone()]),
            ("Err".to_string(), vec![args[1].clone()]),
        ]),
        Type::Named(name, args) if name == "SendError" && args.len() == 1 => Some(vec![
            ("Closed".to_string(), vec![args[0].clone()]),
            ("Cancelled".to_string(), vec![args[0].clone()]),
            ("TimedOut".to_string(), vec![args[0].clone()]),
            ("Full".to_string(), vec![args[0].clone()]),
        ]),
        Type::Named(name, args) if name == "QueueReceive" && args.len() == 1 => Some(vec![
            ("Item".to_string(), vec![args[0].clone()]),
            ("Closed".to_string(), Vec::new()),
            ("TimedOut".to_string(), Vec::new()),
            ("Cancelled".to_string(), Vec::new()),
        ]),
        Type::Named(name, args) if name == "TaskResult" && args.len() == 1 => Some(vec![
            ("Ready".to_string(), vec![args[0].clone()]),
            ("Error".to_string(), vec![Type::named("str")]),
            ("TimedOut".to_string(), Vec::new()),
            ("Cancelled".to_string(), Vec::new()),
        ]),
        Type::Named(name, args) if name == "WaitAny" && args.len() == 1 => Some(vec![
            (
                "Ready".to_string(),
                vec![Type::named("int64"), args[0].clone()],
            ),
            (
                "Error".to_string(),
                vec![Type::named("int64"), Type::named("str")],
            ),
            ("TimedOut".to_string(), Vec::new()),
            ("Cancelled".to_string(), Vec::new()),
        ]),
        Type::Named(name, args) if name == "WaitAll" && args.len() == 1 => Some(vec![
            (
                "Ready".to_string(),
                vec![Type::Named("list".to_string(), vec![args[0].clone()])],
            ),
            (
                "Error".to_string(),
                vec![Type::named("int64"), Type::named("str")],
            ),
            ("TimedOut".to_string(), Vec::new()),
            ("Cancelled".to_string(), Vec::new()),
        ]),
        Type::Named(name, args) if name == "SelectOutcome" && args.len() == 2 => Some(vec![
            (
                "Queue".to_string(),
                vec![
                    Type::named("int64"),
                    Type::Named("QueueReceive".to_string(), vec![args[0].clone()]),
                ],
            ),
            (
                "Task".to_string(),
                vec![
                    Type::named("int64"),
                    Type::Named("TaskResult".to_string(), vec![args[1].clone()]),
                ],
            ),
            ("Deadline".to_string(), vec![Type::named("int64")]),
            ("Cancelled".to_string(), Vec::new()),
        ]),
        _ => None,
    }
}
