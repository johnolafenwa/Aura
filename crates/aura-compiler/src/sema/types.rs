//! Semantic type identity, lowering, substitution, and unification.

use super::{
    erase_type_callable_contracts, fmt, function_type_mismatch_message, is_array_dtype,
    is_builtin_copy_named_type, is_builtin_type, merge_type_callable_contracts,
    preserves_qualified_builtin_type_name, resolve_param_passing, BTreeMap, BTreeSet,
    ClosureCallKind, ClosureCapture, Deserialize, Diagnostic, FunctionParamContract, HashMap,
    ReceiverKind, Result, Serialize, TraitImplInfo, TypeRef,
};

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
    TypeParam(String),
    Module(String),
    Unit,
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
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
            Type::Unit => true,
            Type::Module(_) => false,
            Type::TypeParam(_) => false,
            Type::Tuple(elements) => elements.iter().all(Type::is_copy),
            Type::Function { .. } => true,
            Type::Closure { .. } => false,
            Type::Named(name, args) if name == "Task" && args.len() == 1 => args[0].is_copy(),
            Type::Named(name, args) => is_builtin_copy_named_type(name, args),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Unit => write!(f, "None"),
            Type::Module(name) => write!(f, "module {}", name),
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
                for (index, param) in params.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    match param.passing {
                        ReceiverKind::Borrow => {}
                        ReceiverKind::BorrowMut => write!(f, "mut ")?,
                        ReceiverKind::Value => write!(f, "own ")?,
                    }
                    write!(f, "{}", param.ty)?;
                }
                write!(f, ") -> {return_type}")
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
                for (index, param) in params.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    match param.passing {
                        ReceiverKind::Borrow => {}
                        ReceiverKind::BorrowMut => write!(f, "mut ")?,
                        ReceiverKind::Value => write!(f, "own ")?,
                    }
                    write!(f, "{}", param.ty)?;
                }
                write!(f, ") -> {return_type}")
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
    type_names: &BTreeMap<String, crate::diag::Span>,
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
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_params: &BTreeMap<String, ()>,
    self_type: Option<&Type>,
) -> Result<Type> {
    let (name, type_args) = match &type_ref.kind {
        crate::ast::TypeRefKind::Union(_) | crate::ast::TypeRefKind::Callable { .. } => {
            return Err(Diagnostic::at(
                type_ref.span,
                "this type form requires Batch 1 semantic lowering",
            ));
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
        } => {
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
                        name: String::new(),
                        ty,
                        passing: resolve_param_passing(param.mode),
                        has_default: false,
                        default_erased: true,
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
        let canonical_name =
            if preserves_qualified_builtin_type_name(type_name) || type_name.contains('.') {
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
    type_names: &BTreeMap<String, crate::diag::Span>,
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
    match ty {
        Type::Unit => Type::Unit,
        Type::Module(name) => Type::Module(name.clone()),
        Type::TypeParam(name) => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| Type::TypeParam(name.clone())),
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| substitute_type(element, substitutions))
                .collect(),
        ),
        Type::Function {
            params,
            return_type,
        } => Type::Function {
            params: params
                .iter()
                .map(|param| FunctionParamContract {
                    name: param.name.clone(),
                    ty: substitute_type(&param.ty, substitutions),
                    passing: param.passing,
                    has_default: param.has_default,
                    default_erased: param.default_erased,
                })
                .collect(),
            return_type: Box::new(substitute_type(return_type, substitutions)),
        },
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
                        name: param.name.clone(),
                        ty: substitute_type(&param.ty, substitutions),
                        passing: param.passing,
                        has_default: param.has_default,
                        default_erased: param.default_erased,
                    })
                    .collect(),
            ),
            return_type: Box::new(substitute_type(return_type, substitutions)),
            captures: Box::new(
                captures
                    .iter()
                    .map(|capture| ClosureCapture {
                        name: capture.name.clone(),
                        ty: substitute_type(&capture.ty, substitutions),
                        mode: capture.mode,
                        span: capture.span,
                    })
                    .collect(),
            ),
            call_kind: *call_kind,
        },
        Type::Named(name, args) => {
            let mut substituted_args = args
                .iter()
                .map(|arg| substitute_type(arg, substitutions))
                .collect::<Vec<_>>();
            if matches!(name.as_str(), "list" | "dict" | "set") {
                substituted_args = substituted_args
                    .iter()
                    .map(erase_type_callable_contracts)
                    .collect();
            }
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
    }
}

pub(crate) fn type_pattern_specificity(ty: &Type) -> usize {
    match ty {
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
        Type::TypeParam(name) if type_params.contains(name) => {
            if let Some(existing) = substitutions.get(name) {
                if existing != actual {
                    false
                } else {
                    let merged = merge_type_callable_contracts(existing, actual);
                    substitutions.insert(name.clone(), merged);
                    true
                }
            } else {
                substitutions.insert(name.clone(), actual.clone());
                true
            }
        }
        Type::TypeParam(_) => pattern == actual,
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

pub(super) fn has_unresolved_type_params(ty: &Type) -> bool {
    match ty {
        Type::Unit => false,
        Type::Module(_) => false,
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

pub(super) fn unify_type_pattern(
    pattern: &Type,
    actual: &Type,
    substitutions: &mut HashMap<String, Type>,
) -> Result<()> {
    match pattern {
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
                    let merged = merge_type_callable_contracts(existing, actual);
                    substitutions.insert(name.clone(), merged);
                    Ok(())
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
                } => (params.as_slice(), return_type),
                Type::Closure {
                    params,
                    return_type,
                    ..
                } => (params.as_slice(), return_type),
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
