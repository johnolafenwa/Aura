//! Callable metadata, lambda checking, resolution, defaults, and argument binding.

use super::{
    bind_call_arguments, callable_params_from_decl, collect_binding_target_names,
    has_unresolved_type_params, lower_type_with_self, resolve_param_passing,
    substitute_trait_bound, substitute_type, substitutions_from_decl_type_args,
    type_contains_closure_value, type_contains_loan_closure, unify_type_pattern, Argument,
    BTreeMap, BTreeSet, BorrowedCallPlace, CallConvention, ComprehensionOutput, Deserialize,
    Diagnostic, Expr, ExprKind, FunctionChecker, FunctionDecl, FunctionInfo, FunctionSignature,
    HashMap, LambdaParam, LocalBinding, Param, ParamMode, PlacePath, ReceiverKind, Result,
    Serialize, TraitBound, Type, TypeRef,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum ClosureArgumentPolicy {
    Reject,
    RepeatableParameter(&'static str),
}

#[derive(Copy, Clone)]
pub(super) struct LambdaCallableContext<'a> {
    pub(super) params: &'a [FunctionParamContract],
    pub(super) return_type: Option<&'a Type>,
}

#[derive(Copy, Clone)]
pub(super) struct LambdaTypingRequest<'a> {
    pub(super) explicit_captures: Option<&'a [crate::ast::LambdaCapture]>,
    pub(super) params: &'a [LambdaParam],
    pub(super) body: &'a Expr,
    pub(super) span: crate::diag::Span,
    pub(super) expected: Option<&'a Type>,
    pub(super) callable_context: Option<LambdaCallableContext<'a>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FunctionParamContract {
    pub keyword_only: bool,
    /// Empty for a written `def(T) -> R` type, which has no parameter-name
    /// contract. Inferred values retain the declaration name.
    pub name: String,
    pub ty: Type,
    pub passing: ReceiverKind,
    pub has_default: bool,
    /// True when a default may have existed before a type join or storage
    /// boundary erased that promise. This distinguishes an unavailable
    /// default contract (AU2003) from an originally required parameter
    /// omitted at an ordinary call (AU2004).
    #[serde(default)]
    pub default_erased: bool,
}

/// The callable body that lexically owns a lambda expression.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ClosureOwner {
    Function(String),
    ClassMethod {
        class_name: String,
        method_name: String,
    },
    TraitMethod {
        trait_name: String,
        method_name: String,
    },
    TraitImplMethod {
        trait_name: String,
        for_type: String,
        method_name: String,
    },
    TopLevel,
}

/// Stable semantic identity for one lambda expression.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ClosureId {
    pub module_name: String,
    pub owner: ClosureOwner,
    pub line: usize,
    pub column: usize,
}

impl ClosureId {
    pub(super) fn new(module_name: &str, owner: ClosureOwner, span: crate::diag::Span) -> Self {
        Self {
            module_name: module_name.to_string(),
            owner,
            line: span.line,
            column: span.column,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ClosureCaptureMode {
    Copy,
    Move,
    SharedView,
    MutableView,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ClosureCallKind {
    Repeatable,
    MutableRepeatable,
    Consuming,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClosureCapture {
    pub name: String,
    pub ty: Type,
    pub mode: ClosureCaptureMode,
    pub span: crate::diag::Span,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClosureInfo {
    pub id: ClosureId,
    pub span: crate::diag::Span,
    pub params: Vec<FunctionParamContract>,
    pub return_type: Type,
    /// Explicit lists preserve source order; implicit captures use lexical
    /// first-use order.
    pub captures: Vec<ClosureCapture>,
    pub call_kind: ClosureCallKind,
}

impl ClosureInfo {
    pub fn ty(&self) -> Type {
        if self.captures.is_empty() {
            Type::Function {
                params: self.params.clone(),
                return_type: Box::new(self.return_type.clone()),
            }
        } else {
            Type::Closure {
                params: Box::new(self.params.clone()),
                return_type: Box::new(self.return_type.clone()),
                captures: Box::new(self.captures.clone()),
                call_kind: self.call_kind,
            }
        }
    }
}

pub(super) fn default_argument_references_param(
    expr: &Expr,
    param_names: &[String],
) -> Option<String> {
    match &expr.kind {
        ExprKind::Name(name) => param_names
            .iter()
            .find(|param_name| *param_name == name)
            .cloned(),
        ExprKind::Group(inner) | ExprKind::Try(inner) => {
            default_argument_references_param(inner, param_names)
        }
        ExprKind::IsNone { value: expr, .. }
        | ExprKind::Unary { expr, .. }
        | ExprKind::Cast { expr, .. } => default_argument_references_param(expr, param_names),
        ExprKind::Specialize { expr, .. } => default_argument_references_param(expr, param_names),
        ExprKind::Member { object, .. } => default_argument_references_param(object, param_names),
        ExprKind::Index { object, index } => default_argument_references_param(object, param_names)
            .or_else(|| default_argument_references_param(index, param_names)),
        ExprKind::Slice {
            object, start, end, ..
        } => default_argument_references_param(object, param_names)
            .or_else(|| {
                start
                    .as_deref()
                    .and_then(|value| default_argument_references_param(value, param_names))
            })
            .or_else(|| {
                end.as_deref()
                    .and_then(|value| default_argument_references_param(value, param_names))
            }),
        ExprKind::Call { callee, args } => default_argument_references_param(callee, param_names)
            .or_else(|| {
                args.iter().find_map(|argument| {
                    default_argument_references_param(&argument.value, param_names)
                })
            }),
        ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => elements
            .iter()
            .find_map(|element| default_argument_references_param(element, param_names)),
        ExprKind::Map(entries) => entries.iter().find_map(|entry| {
            default_argument_references_param(&entry.key, param_names)
                .or_else(|| default_argument_references_param(&entry.value, param_names))
        }),
        ExprKind::Comprehension { output, clauses } => {
            let mut visible = param_names.to_vec();
            for clause in clauses {
                if let Some(name) = default_argument_references_param(&clause.iterable, &visible) {
                    return Some(name);
                }
                let mut bound = BTreeSet::new();
                collect_binding_target_names(&clause.target, &mut bound);
                visible.retain(|name| !bound.contains(name));
                for filter in &clause.filters {
                    if let Some(name) = default_argument_references_param(filter, &visible) {
                        return Some(name);
                    }
                }
            }
            match output {
                ComprehensionOutput::List(value) | ComprehensionOutput::Set(value) => {
                    default_argument_references_param(value, &visible)
                }
                ComprehensionOutput::Map { key, value } => {
                    default_argument_references_param(key, &visible)
                        .or_else(|| default_argument_references_param(value, &visible))
                }
            }
        }
        ExprKind::FString(parts) => parts.iter().find_map(|part| match part {
            crate::ast::FormatPart::Literal(_) => None,
            crate::ast::FormatPart::Expr(expr) | crate::ast::FormatPart::Formatted { expr, .. } => {
                default_argument_references_param(expr, param_names)
            }
        }),
        ExprKind::Match {
            scrutinee, arms, ..
        } => default_argument_references_param(scrutinee, param_names).or_else(|| {
            arms.iter()
                .find_map(|arm| default_argument_references_param(&arm.value, param_names))
        }),
        ExprKind::Membership {
            value, container, ..
        } => default_argument_references_param(value, param_names)
            .or_else(|| default_argument_references_param(container, param_names)),
        ExprKind::CompareChain { first, links } => {
            default_argument_references_param(first, param_names).or_else(|| {
                links
                    .iter()
                    .find_map(|link| default_argument_references_param(&link.operand, param_names))
            })
        }
        ExprKind::Binary { left, right, .. } => {
            default_argument_references_param(left, param_names)
                .or_else(|| default_argument_references_param(right, param_names))
        }
        ExprKind::Conditional {
            then_expr,
            condition,
            else_expr,
        } => default_argument_references_param(condition, param_names)
            .or_else(|| default_argument_references_param(then_expr, param_names))
            .or_else(|| default_argument_references_param(else_expr, param_names)),
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::DurationNanos(_)
        | ExprKind::BuiltinOmitted => None,
        ExprKind::Lambda { params, body, .. } => {
            let shadowed = params
                .iter()
                .map(|param| &param.name)
                .collect::<BTreeSet<_>>();
            let visible = param_names
                .iter()
                .filter(|name| !shadowed.contains(name))
                .cloned()
                .collect::<Vec<_>>();
            default_argument_references_param(body, &visible)
        }
    }
}

pub(super) fn capturing_closure_branch_mismatch(expected: &Type, actual: &Type) -> bool {
    expected != actual
        && (type_contains_closure_value(expected) || type_contains_closure_value(actual))
}

pub(super) fn capturing_closure_branch_diagnostic(
    expression_kind: &str,
    branch_kind: &str,
    span: crate::diag::Span,
) -> Diagnostic {
    Diagnostic::coded_at(
        "AU2002",
        span,
        format!(
            "{expression_kind} expressions cannot merge capturing closure values in this language version"
        ),
    )
    .with_help(format!(
        "call the closure inside each {branch_kind}, or use capture-free lambdas or named functions that share one `def(...) -> ...` type"
    ))
}

pub(super) fn function_type_mismatch_message(expected: &Type, actual: &Type) -> String {
    let (
        Type::Function {
            params: expected_params,
            ..
        },
        Type::Function {
            params: actual_params,
            ..
        },
    ) = (expected, actual)
    else {
        return format!("expected `{expected}`, found `{actual}`");
    };
    let capability_name = |passing: ReceiverKind| match passing {
        ReceiverKind::Borrow => "shared",
        ReceiverKind::BorrowMut => "mut",
        ReceiverKind::Value => "own",
    };
    if let Some((index, (expected_param, actual_param))) = expected_params
        .iter()
        .zip(actual_params)
        .enumerate()
        .find(|(_, (expected, actual))| expected.passing != actual.passing)
    {
        return format!(
            "function parameter {} has `{}` capability, but `{}` requires `{}`; update that function type parameter to use the matching bare, `mut`, or `own` prefix",
            index + 1,
            capability_name(actual_param.passing),
            expected,
            capability_name(expected_param.passing),
        );
    }
    format!("expected `{expected}`, found `{actual}`")
}

pub(super) fn closure_signature_matches_function(closure: &Type, function: &Type) -> bool {
    let (
        Type::Closure {
            params: closure_params,
            return_type: closure_return,
            ..
        },
        Type::Function {
            params: function_params,
            return_type: function_return,
        },
    ) = (closure, function)
    else {
        return false;
    };
    closure_params.len() == function_params.len()
        && closure_params
            .iter()
            .zip(function_params)
            .all(|(closure, function)| {
                closure.passing == function.passing && closure.ty == function.ty
            })
        && closure_return == function_return
}

/// Joins the non-structural callable contract carried alongside a type.
///
/// Function names and default availability are usable only when every
/// possible runtime value agrees. Written function types carry empty names
/// and no defaults, so joining through an erased annotation stays erased.
pub(super) fn merge_type_callable_contracts(left: &Type, right: &Type) -> Type {
    debug_assert!(
        left == right,
        "callable contract joins require one structural type"
    );
    match (left, right) {
        (
            Type::Function {
                params: left_params,
                return_type: left_return,
            },
            Type::Function {
                params: right_params,
                return_type: right_return,
            },
        ) => Type::Function {
            params: left_params
                .iter()
                .zip(right_params)
                .map(|(left, right)| FunctionParamContract {
                    keyword_only: left.keyword_only || right.keyword_only,
                    name: if left.name == right.name {
                        left.name.clone()
                    } else {
                        String::new()
                    },
                    ty: merge_type_callable_contracts(&left.ty, &right.ty),
                    passing: left.passing,
                    has_default: left.has_default && right.has_default,
                    default_erased: left.default_erased
                        || right.default_erased
                        || left.has_default != right.has_default,
                })
                .collect(),
            return_type: Box::new(merge_type_callable_contracts(left_return, right_return)),
        },
        (Type::Tuple(left_elements), Type::Tuple(right_elements)) => Type::Tuple(
            left_elements
                .iter()
                .zip(right_elements)
                .map(|(left, right)| merge_type_callable_contracts(left, right))
                .collect(),
        ),
        (Type::Named(name, left_args), Type::Named(_, right_args)) => Type::Named(
            name.clone(),
            left_args
                .iter()
                .zip(right_args)
                .map(|(left, right)| merge_type_callable_contracts(left, right))
                .collect(),
        ),
        _ => left.clone(),
    }
}

/// Erases non-ABI callable metadata at a mutable storage boundary.
///
/// A collection element or class field can be replaced through an alias that
/// is invisible to the local flow analysis. Its structural function type
/// remains precise, but parameter names and omitted-argument availability are
/// not sound. Exact positional calls remain available; code that needs a
/// named/default contract must keep a separately inferred concrete function
/// value outside mutable storage.
pub(super) fn erase_type_callable_contracts(ty: &Type) -> Type {
    match ty {
        Type::Union(_) => ty.clone(),
        Type::Function {
            params,
            return_type,
        } => Type::Function {
            params: params
                .iter()
                .map(|param| FunctionParamContract {
                    keyword_only: param.keyword_only,
                    name: String::new(),
                    ty: erase_type_callable_contracts(&param.ty),
                    passing: param.passing,
                    has_default: false,
                    default_erased: true,
                })
                .collect(),
            return_type: Box::new(erase_type_callable_contracts(return_type)),
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
                        keyword_only: param.keyword_only,
                        name: String::new(),
                        ty: erase_type_callable_contracts(&param.ty),
                        passing: param.passing,
                        has_default: false,
                        default_erased: true,
                    })
                    .collect(),
            ),
            return_type: Box::new(erase_type_callable_contracts(return_type)),
            captures: captures.clone(),
            call_kind: *call_kind,
        },
        Type::Tuple(elements) => {
            Type::Tuple(elements.iter().map(erase_type_callable_contracts).collect())
        }
        Type::Named(name, args) => Type::Named(
            name.clone(),
            args.iter().map(erase_type_callable_contracts).collect(),
        ),
        Type::TypeParam(_) | Type::Module(_) | Type::Unit => ty.clone(),
    }
}

pub(super) fn required_ordered_arg<'a>(
    ordered_args: &[Option<&'a Argument>],
    index: usize,
    span: crate::diag::Span,
    message: impl Into<String>,
) -> Result<&'a Argument> {
    ordered_args
        .get(index)
        .and_then(|argument| *argument)
        .ok_or_else(|| Diagnostic::at(span, message.into()))
}

#[derive(Clone)]
pub(super) struct ResolvedCallableInfo {
    pub(super) display_name: String,
    pub(super) decl: FunctionDecl,
    pub(super) signature: FunctionSignature,
    pub(super) type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    pub(super) seed_substitutions: HashMap<String, Type>,
}

#[derive(Debug)]
pub(super) struct CheckedCallableTypes {
    pub(super) params: Vec<Type>,
    pub(super) return_type: Type,
}

impl<'a> FunctionChecker<'a> {
    pub(super) fn bound_argument<'b>(
        &self,
        ordered_args: &'b [Option<&'b Argument>],
        index: usize,
        span: crate::diag::Span,
        message: impl Into<String>,
    ) -> Result<&'b Argument> {
        ordered_args
            .get(index)
            .copied()
            .flatten()
            .ok_or_else(|| Diagnostic::at(span, format!("internal error: {}", message.into())))
    }

    pub(super) fn collection_callback_return_type(
        &self,
        collection: &str,
        method_name: &str,
        callback: &Argument,
        element_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let contextual_params = [FunctionParamContract {
            keyword_only: false,
            name: "value".to_string(),
            ty: element_ty.clone(),
            passing: ReceiverKind::Borrow,
            has_default: false,
            default_erased: false,
        }];
        let bool_ty = Type::named("bool");
        let callback_ty = match &callback.value.kind {
            ExprKind::Lambda {
                captures,
                params,
                body,
            } => self.type_of_lambda(
                LambdaTypingRequest {
                    explicit_captures: captures.as_deref(),
                    params,
                    body,
                    span: callback.value.span,
                    expected: None,
                    callable_context: Some(LambdaCallableContext {
                        params: &contextual_params,
                        return_type: (method_name == "filter").then_some(&bool_ty),
                    }),
                },
                locals,
            )?,
            _ => self.type_of_expr(&callback.value, locals)?,
        };
        let (params, return_type) = match &callback_ty {
            Type::Function {
                params,
                return_type,
            } => (params.clone(), return_type.clone()),
            Type::Closure {
                params,
                return_type,
                call_kind: ClosureCallKind::Repeatable,
                ..
            } => (params.as_ref().clone(), return_type.clone()),
            Type::Closure {
                call_kind: ClosureCallKind::Consuming,
                ..
            } => {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    callback.span,
                    format!(
                        "`{collection}.{method_name}` callback must be repeatable, found `{callback_ty}`"
                    ),
                )
                .with_help(
                    "clone or precompute the consumed capture outside the lambda, or use a named function that does not consume closure state",
                ))
            }
            _ => {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    callback.span,
                    format!(
                        "`{collection}.{method_name}` expects a function value, found `{callback_ty}`"
                    ),
                ))
            }
        };
        if params.len() != 1 || params[0].passing != ReceiverKind::Borrow {
            return Err(Diagnostic::coded_at(
                "AU2002",
                callback.span,
                format!(
                    "`{collection}.{method_name}` callback must take exactly one shared parameter of type `{element_ty}`, found `{}`",
                    Type::Function {
                        params,
                        return_type,
                    }
                ),
            )
            .with_help(
                "declare the callback parameter with the bare type spelling; `mut` and `own` callbacks are not accepted",
            ));
        }
        if params[0].ty != *element_ty {
            return Err(Diagnostic::coded_at(
                "AU2002",
                callback.span,
                format!(
                    "`{collection}.{method_name}` callback expects shared `{element_ty}`, found shared `{}`",
                    params[0].ty
                ),
            ));
        }
        Ok(*return_type)
    }

    pub(super) fn vec_callback_return_type(
        &self,
        method_name: &str,
        callback: &Argument,
        element_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        self.collection_callback_return_type("list", method_name, callback, element_ty, locals)
    }

    pub(super) fn array_callback_return_type(
        &self,
        callback: &Argument,
        element_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        self.collection_callback_return_type("Array", "map", callback, element_ty, locals)
    }

    pub(super) fn check_param_defaults(
        &self,
        params: &[Param],
        type_param_scope: &BTreeMap<String, ()>,
        self_type: Option<&Type>,
        allow_defaults: bool,
        owner: &str,
    ) -> Result<()> {
        let mut saw_default = false;
        let param_names = params
            .iter()
            .map(|param| param.name.clone())
            .collect::<Vec<_>>();

        for param in params {
            if param.mode == ParamMode::BorrowMut && param.default.is_some() {
                return Err(Diagnostic::coded_at(
                    "AU3002",
                    param.span,
                    format!(
                        "`mut` parameter `{}` cannot have a default: the default creates a caller-invisible temporary, so mutations through it would be silently lost; require the caller to pass a value, or take the parameter as `own T` and return the result",
                        param.name
                    ),
                ));
            }
            let lowered = lower_type_with_self(
                &param.ty,
                self.type_names,
                self.type_arities,
                self.canonical_type_names,
                type_param_scope,
                self_type,
            )?;
            match &param.default {
                Some(default) => {
                    if !allow_defaults {
                        return Err(Diagnostic::at(
                            param.span,
                            format!(
                                "default arguments are not allowed in {} declarations",
                                owner
                            ),
                        ));
                    }
                    saw_default = true;
                    if matches!(default.kind, ExprKind::BuiltinOmitted) {
                        continue;
                    }
                    if let Some(name) = default_argument_references_param(default, &param_names) {
                        return Err(Diagnostic::at(
                            default.span,
                            format!(
                                "default argument for parameter `{}` may not reference parameter `{}`",
                                param.name, name
                            ),
                        ));
                    }
                    let default_ty =
                        self.type_of_expr_hint(default, &mut HashMap::new(), Some(&lowered))?;
                    if default_ty != lowered {
                        return Err(Diagnostic::at(
                            default.span,
                            format!(
                                "default argument for parameter `{}` has type `{}`, expected `{}`",
                                param.name, default_ty, lowered
                            ),
                        ));
                    }
                }
                None if saw_default => {
                    return Err(Diagnostic::at(
                        param.span,
                        "parameters with default arguments must come after required parameters",
                    ));
                }
                None => {}
            }
        }

        Ok(())
    }

    pub(super) fn collect_lambda_capture_uses(
        expr: &Expr,
        bound: &BTreeSet<String>,
        seen: &mut BTreeSet<String>,
        captures: &mut Vec<(String, crate::diag::Span)>,
    ) {
        match &expr.kind {
            ExprKind::Name(name) => {
                if !bound.contains(name) && seen.insert(name.clone()) {
                    captures.push((name.clone(), expr.span));
                }
            }
            ExprKind::Lambda { params, body, .. } => {
                let mut nested_bound = bound.clone();
                nested_bound.extend(params.iter().map(|param| param.name.clone()));
                Self::collect_lambda_capture_uses(body, &nested_bound, seen, captures);
            }
            ExprKind::Group(inner)
            | ExprKind::Try(inner)
            | ExprKind::Unary { expr: inner, .. }
            | ExprKind::IsNone { value: inner, .. }
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. } => {
                Self::collect_lambda_capture_uses(inner, bound, seen, captures);
            }
            ExprKind::Binary { left, right, .. }
            | ExprKind::Membership {
                value: left,
                container: right,
                ..
            } => {
                Self::collect_lambda_capture_uses(left, bound, seen, captures);
                Self::collect_lambda_capture_uses(right, bound, seen, captures);
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                Self::collect_lambda_capture_uses(condition, bound, seen, captures);
                Self::collect_lambda_capture_uses(then_expr, bound, seen, captures);
                Self::collect_lambda_capture_uses(else_expr, bound, seen, captures);
            }
            ExprKind::Call { callee, args } => {
                Self::collect_lambda_capture_uses(callee, bound, seen, captures);
                for argument in args {
                    Self::collect_lambda_capture_uses(&argument.value, bound, seen, captures);
                }
            }
            ExprKind::Member { object, .. } => {
                Self::collect_lambda_capture_uses(object, bound, seen, captures);
            }
            ExprKind::Index { object, index } => {
                Self::collect_lambda_capture_uses(object, bound, seen, captures);
                Self::collect_lambda_capture_uses(index, bound, seen, captures);
            }
            ExprKind::Slice {
                object, start, end, ..
            } => {
                Self::collect_lambda_capture_uses(object, bound, seen, captures);
                if let Some(start) = start {
                    Self::collect_lambda_capture_uses(start, bound, seen, captures);
                }
                if let Some(end) = end {
                    Self::collect_lambda_capture_uses(end, bound, seen, captures);
                }
            }
            ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => {
                for element in elements {
                    Self::collect_lambda_capture_uses(element, bound, seen, captures);
                }
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    Self::collect_lambda_capture_uses(&entry.key, bound, seen, captures);
                    Self::collect_lambda_capture_uses(&entry.value, bound, seen, captures);
                }
            }
            ExprKind::Comprehension { output, clauses } => {
                let mut comprehension_bound = bound.clone();
                for clause in clauses {
                    Self::collect_lambda_capture_uses(
                        &clause.iterable,
                        &comprehension_bound,
                        seen,
                        captures,
                    );
                    collect_binding_target_names(&clause.target, &mut comprehension_bound);
                    for filter in &clause.filters {
                        Self::collect_lambda_capture_uses(
                            filter,
                            &comprehension_bound,
                            seen,
                            captures,
                        );
                    }
                }
                match output {
                    ComprehensionOutput::List(value) | ComprehensionOutput::Set(value) => {
                        Self::collect_lambda_capture_uses(
                            value,
                            &comprehension_bound,
                            seen,
                            captures,
                        );
                    }
                    ComprehensionOutput::Map { key, value } => {
                        Self::collect_lambda_capture_uses(
                            key,
                            &comprehension_bound,
                            seen,
                            captures,
                        );
                        Self::collect_lambda_capture_uses(
                            value,
                            &comprehension_bound,
                            seen,
                            captures,
                        );
                    }
                }
            }
            ExprKind::FString(parts) => {
                for part in parts {
                    match part {
                        crate::ast::FormatPart::Expr(value)
                        | crate::ast::FormatPart::Formatted { expr: value, .. } => {
                            Self::collect_lambda_capture_uses(value, bound, seen, captures);
                        }
                        crate::ast::FormatPart::Literal(_) => {}
                    }
                }
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                Self::collect_lambda_capture_uses(scrutinee, bound, seen, captures);
                for arm in arms {
                    let mut arm_bound = bound.clone();
                    Self::collect_pattern_binding_names(&arm.pattern, &mut arm_bound);
                    Self::collect_lambda_capture_uses(&arm.value, &arm_bound, seen, captures);
                }
            }
            ExprKind::CompareChain { first, links } => {
                Self::collect_lambda_capture_uses(first, bound, seen, captures);
                for link in links {
                    Self::collect_lambda_capture_uses(&link.operand, bound, seen, captures);
                }
            }
            ExprKind::Int(_)
            | ExprKind::DurationNanos(_)
            | ExprKind::BuiltinOmitted
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::String(_) => {}
        }
    }

    pub(super) fn type_of_lambda(
        &self,
        request: LambdaTypingRequest<'_>,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let LambdaTypingRequest {
            explicit_captures,
            params,
            body,
            span,
            expected,
            callable_context,
        } = request;
        let expected_signature = match (callable_context, expected) {
            (Some(context), _) => Some((context.params, context.return_type)),
            (
                None,
                Some(Type::Function {
                    params,
                    return_type,
                }),
            ) => Some((params.as_slice(), Some(return_type.as_ref()))),
            (
                None,
                Some(Type::Closure {
                    params,
                    return_type,
                    ..
                }),
            ) => Some((params.as_slice(), Some(return_type.as_ref()))),
            (None, Some(expected)) => {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    span,
                    format!("lambda requires a callable context, found expected type `{expected}`"),
                ))
            }
            (None, None) => None,
        };
        if !params.is_empty() && expected_signature.is_none() {
            return Err(Diagnostic::coded_at(
                "AU2002",
                span,
                "lambda parameter types require an expected `def(...) -> ...` context",
            )
            .with_help(
                "add a `def(...) -> ...` annotation to the immutable local, or pass the lambda directly to a callback parameter",
            ));
        }
        if let Some((expected_params, _)) = expected_signature {
            if params.len() != expected_params.len() {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    span,
                    format!(
                        "lambda expects {} contextual parameter{}, but its function type provides {}",
                        params.len(),
                        if params.len() == 1 { "" } else { "s" },
                        expected_params.len()
                    ),
                ));
            }
            for (param, expected_param) in params.iter().zip(expected_params) {
                let passing = resolve_param_passing(param.mode);
                if passing != expected_param.passing {
                    return Err(Diagnostic::coded_at(
                        "AU2002",
                        param.span,
                        format!(
                            "lambda parameter `{}` has `{}` capability, but the expected function type requires `{}`",
                            param.name,
                            Self::capability_name(passing),
                            Self::capability_name(expected_param.passing),
                        ),
                    )
                    .with_help(
                        "use a bare, `mut`, or `own` lambda parameter matching the expected function type",
                    ));
                }
            }
        }

        let bound = params
            .iter()
            .map(|param| param.name.clone())
            .collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        let mut capture_uses = Vec::new();
        Self::collect_lambda_capture_uses(body, &bound, &mut seen, &mut capture_uses);

        let used_outer = capture_uses
            .iter()
            .filter(|(name, _)| locals.contains_key(name))
            .map(|(name, _)| name.clone())
            .collect::<BTreeSet<_>>();
        let explicit_modes = explicit_captures.map(|listed| {
            listed
                .iter()
                .map(|capture| (capture.name.clone(), (capture.mode, capture.span)))
                .collect::<BTreeMap<_, _>>()
        });
        if let (Some(listed), Some(modes)) = (explicit_captures, explicit_modes.as_ref()) {
            if let Some(capture) = listed
                .iter()
                .find(|capture| !used_outer.contains(&capture.name))
            {
                return Err(Diagnostic::coded_at(
                    "AU3004",
                    capture.span,
                    format!(
                        "capture-list entry `{}` is not used by the lambda body",
                        capture.name
                    ),
                ));
            }
            if let Some(missing) = used_outer.iter().find(|name| !modes.contains_key(*name)) {
                let use_span = capture_uses
                    .iter()
                    .find(|(name, _)| name == missing)
                    .map(|(_, span)| *span)
                    .unwrap_or(span);
                return Err(Diagnostic::coded_at(
                    "AU3004",
                    use_span,
                    format!(
                        "outer local `{missing}` is used by the lambda but missing from its exhaustive capture list"
                    ),
                )
                .with_help(format!("add `{missing}`, `mut {missing}`, or `own {missing}` to the capture list")));
            }
        }

        let mut captures = Vec::new();
        let mut lambda_locals = HashMap::new();
        let capture_sequence = explicit_captures
            .map(|listed| {
                listed
                    .iter()
                    .map(|capture| (capture.name.clone(), capture.span))
                    .collect::<Vec<_>>()
            })
            .unwrap_or(capture_uses);
        for (name, capture_span) in capture_sequence {
            let Some(binding) = locals.get(&name) else {
                continue;
            };
            self.ensure_pattern_binding_not_stale(&name, capture_span, binding)?;
            if binding.moved {
                return Err(self.moved_value_diagnostic(&name, capture_span, binding));
            }
            if !binding.moved_fields.is_empty() {
                return Err(Diagnostic::coded_at(
                    "AU3001",
                    capture_span,
                    format!("cannot capture partially moved value `{name}`"),
                ));
            }
            if type_contains_loan_closure(&binding.ty) {
                return Err(Diagnostic::coded_at(
                    "AU3010",
                    capture_span,
                    format!(
                        "capture `{name}` contains a live view and cannot be nested in another closure"
                    ),
                )
                .with_help(
                    "keep the loan closure in its matching inferred local and call it directly",
                ));
            }
            let explicit_mode = explicit_modes
                .as_ref()
                .and_then(|modes| modes.get(&name))
                .map(|(mode, _)| *mode);
            if explicit_mode.is_none() && binding.passing != ReceiverKind::Value {
                let shared_parameter = self.implicit_borrowed_params.contains_key(&name);
                let mut diagnostic = Diagnostic::coded_at(
                    "AU3002",
                    capture_span,
                    format!(
                        "lambda cannot capture shared {} `{name}` by value",
                        if shared_parameter {
                            "parameter"
                        } else {
                            "value"
                        }
                    ),
                );
                if let Some(origin) = binding.borrowed_at {
                    diagnostic = diagnostic.with_secondary(
                        origin,
                        format!(
                            "shared {} `{name}` is declared here",
                            if shared_parameter {
                                "parameter"
                            } else {
                                "value"
                            }
                        ),
                    );
                }
                return Err(diagnostic.with_help(format!(
                    "clone `{name}` into an owned local before creating the lambda, or declare the enclosing parameter as `own {}`",
                    binding.ty
                )));
            }
            let mode = match explicit_mode {
                Some(ParamMode::Default) => ClosureCaptureMode::SharedView,
                Some(ParamMode::BorrowMut) => {
                    if binding.passing == ReceiverKind::Borrow || !binding.mutable_place {
                        return Err(Diagnostic::coded_at(
                            "AU3004",
                            capture_span,
                            format!(
                                "capture `mut {name}` requires a mutable place or mutable view"
                            ),
                        ));
                    }
                    ClosureCaptureMode::MutableView
                }
                Some(ParamMode::Own) if binding.view.is_some() => {
                    return Err(Diagnostic::coded_at(
                        "AU3004",
                        capture_span,
                        format!("capture `own {name}` cannot take ownership of a view's pointee"),
                    )
                    .with_help(format!(
                        "clone `{name}` into an owned local first, then capture that local"
                    )));
                }
                Some(ParamMode::Own) | None if self.is_copy_type(&binding.ty) => {
                    ClosureCaptureMode::Copy
                }
                Some(ParamMode::Own) | None => ClosureCaptureMode::Move,
            };
            if matches!(
                mode,
                ClosureCaptureMode::SharedView | ClosureCaptureMode::MutableView
            ) {
                let kind = if mode == ClosureCaptureMode::MutableView {
                    crate::ast::ViewKind::Mutable
                } else {
                    crate::ast::ViewKind::Shared
                };
                let source = self.canonicalize_view_place(PlacePath::root(name.clone()), locals);
                let parent = binding.view.as_ref().map(|_| name.as_str());
                self.ensure_view_loan_available(&source, kind, parent, capture_span, locals)?;
            }
            captures.push(ClosureCapture {
                name: name.clone(),
                ty: binding.ty.clone(),
                mode,
                span: capture_span,
            });
            lambda_locals.insert(
                name.clone(),
                LocalBinding {
                    ty: binding.ty.clone(),
                    assignable: false,
                    mutable_place: mode == ClosureCaptureMode::MutableView,
                    managed_resource: false,
                    passing: match mode {
                        ClosureCaptureMode::SharedView => ReceiverKind::Borrow,
                        ClosureCaptureMode::MutableView => ReceiverKind::BorrowMut,
                        ClosureCaptureMode::Copy | ClosureCaptureMode::Move => ReceiverKind::Value,
                    },
                    borrow_origin: matches!(
                        mode,
                        ClosureCaptureMode::SharedView | ClosureCaptureMode::MutableView
                    )
                    .then(|| name.clone()),
                    borrowed_at: matches!(
                        mode,
                        ClosureCaptureMode::SharedView | ClosureCaptureMode::MutableView
                    )
                    .then_some(capture_span),
                    match_borrow_place: None,
                    stale_match_borrow_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
                    shared_match_places: BTreeMap::new(),
                    captured: mode != ClosureCaptureMode::MutableView,
                    view: None,
                    closure_loans: Vec::new(),
                    narrowed: BTreeMap::new(),
                    stale_narrowing: BTreeMap::new(),
                },
            );
        }

        let expected_params = expected_signature.map(|(params, _)| params);
        let param_contracts = params
            .iter()
            .enumerate()
            .map(|(index, param)| FunctionParamContract {
                keyword_only: param.keyword_only,
                name: param.name.clone(),
                ty: expected_params
                    .and_then(|params| params.get(index))
                    .map(|param| param.ty.clone())
                    .unwrap_or(Type::Unit),
                passing: resolve_param_passing(param.mode),
                has_default: false,
                default_erased: false,
            })
            .collect::<Vec<_>>();
        for (param, contract) in params.iter().zip(&param_contracts) {
            lambda_locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty: contract.ty.clone(),
                    assignable: false,
                    mutable_place: contract.passing == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing: contract.passing,
                    borrow_origin: (contract.passing != ReceiverKind::Value)
                        .then(|| param.name.clone()),
                    borrowed_at: (contract.passing != ReceiverKind::Value).then_some(param.span),
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
                    narrowed: BTreeMap::new(),
                    stale_narrowing: BTreeMap::new(),
                },
            );
        }

        let expected_return = expected_signature.and_then(|(_, return_type)| return_type);
        self.reject_mutable_returned_view_value(body, &mut lambda_locals, false)?;
        self.reject_owned_view_value(body, &mut lambda_locals, "an owned lambda result")?;
        let return_type = self.type_of_expr_hint(body, &mut lambda_locals, expected_return)?;
        if let Some(expected_return) = expected_return {
            if return_type != *expected_return {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    body.span,
                    format!("lambda body has type `{return_type}`, expected `{expected_return}`"),
                ));
            }
        }
        if type_contains_loan_closure(&return_type) {
            return Err(Diagnostic::coded_at(
                "AU3010",
                body.span,
                "a closure cannot return another closure containing a live view",
            )
            .with_help(
                "keep the loan-bearing closure in its matching inferred local and call it directly",
            ));
        }
        self.consume_value_expr(body, &mut lambda_locals)?;
        let call_kind = if captures.iter().any(|capture| {
            lambda_locals
                .get(&capture.name)
                .is_some_and(|binding| binding.moved || !binding.moved_fields.is_empty())
        }) {
            ClosureCallKind::Consuming
        } else if captures
            .iter()
            .any(|capture| capture.mode == ClosureCaptureMode::MutableView)
        {
            ClosureCallKind::MutableRepeatable
        } else {
            ClosureCallKind::Repeatable
        };
        let id = ClosureId::new(self.module_name, self.closure_owner.clone(), span);
        let info = ClosureInfo {
            id: id.clone(),
            span,
            params: param_contracts,
            return_type,
            captures,
            call_kind,
        };
        let ty = info.ty();
        self.closure_infos.borrow_mut().insert(id, info.clone());
        for capture in &info.captures {
            if capture.mode == ClosureCaptureMode::Move {
                self.consume_binding(&capture.name, capture.span, locals)?;
            }
        }
        Ok(ty)
    }

    pub(super) fn function_value_type(
        &self,
        function: &FunctionInfo,
        expected: Option<&Type>,
        explicit_type_args: Option<&[Type]>,
        span: crate::diag::Span,
        display_name: &str,
    ) -> Result<Type> {
        if function.decl.view_return.is_some() {
            return Err(Diagnostic::coded_at(
                "AU3010",
                span,
                format!(
                    "view-returning callable `{}` cannot be stored as a structural function value",
                    function.decl.name
                ),
            )
            .with_help(
                "call it directly and bind the result with `view`, because structural `def(...) -> R` types cannot encode a returned-view origin",
            ));
        }
        let mut substitutions = if let Some(explicit_type_args) = explicit_type_args {
            if explicit_type_args.len() != function.decl.type_params.len() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "{display_name} expects {} type argument{}, found {}",
                        function.decl.type_params.len(),
                        if function.decl.type_params.len() == 1 {
                            ""
                        } else {
                            "s"
                        },
                        explicit_type_args.len(),
                    ),
                ));
            }
            substitutions_from_decl_type_args(&function.decl.type_params, explicit_type_args)
        } else {
            HashMap::new()
        };

        if !function.decl.type_params.is_empty() && explicit_type_args.is_none() {
            let Some(Type::Function {
                params: expected_params,
                return_type: expected_return,
            }) = expected
            else {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "generic {display_name} requires explicit type arguments or an expected function type"
                    ),
                )
                .with_help(format!(
                    "write `{}[...]` with all type arguments, or assign it where a concrete `def(...) -> ...` type is expected",
                    function.decl.name
                )));
            };
            if function
                .signature
                .param_passings
                .iter()
                .zip(expected_params)
                .any(|(actual, expected)| *actual != expected.passing)
                || function.signature.params.len() != expected_params.len()
            {
                let actual = Type::Function {
                    params: function
                        .decl
                        .params
                        .iter()
                        .zip(&function.signature.params)
                        .zip(&function.signature.param_passings)
                        .map(|((decl, ty), passing)| FunctionParamContract {
                            keyword_only: decl.keyword_only,
                            name: decl.name.clone(),
                            ty: ty.clone(),
                            passing: *passing,
                            has_default: decl.default.is_some(),
                            default_erased: false,
                        })
                        .collect(),
                    return_type: Box::new(function.signature.return_type.clone()),
                };
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    span,
                    function_type_mismatch_message(expected.expect("matched above"), &actual),
                ));
            }
            for (pattern, actual) in function.signature.params.iter().zip(expected_params) {
                unify_type_pattern(pattern, &actual.ty, &mut substitutions).map_err(|error| {
                    Diagnostic::coded_at(
                        "AU2002",
                        span,
                        format!("cannot specialize {display_name}: {}", error.message),
                    )
                })?;
            }
            unify_type_pattern(
                &function.signature.return_type,
                expected_return,
                &mut substitutions,
            )
            .map_err(|error| {
                Diagnostic::coded_at(
                    "AU2002",
                    span,
                    format!("cannot specialize {display_name}: {}", error.message),
                )
            })?;
        }

        for type_param in &function.decl.type_params {
            let Some(resolved) = substitutions.get(type_param) else {
                return Err(Diagnostic::at(
                    span,
                    format!("cannot infer type parameter `{type_param}` for {display_name}"),
                ));
            };
            let mut bounds = function
                .type_param_bounds
                .get(type_param)
                .cloned()
                .unwrap_or_default();
            for bound in &mut bounds {
                *bound = substitute_trait_bound(bound, &substitutions);
            }
            self.assert_type_satisfies_bounds(resolved, &bounds, span)?;
        }
        self.enforce_rng_clone_obligations(
            display_name,
            &function.signature.rng_clone_safe_type_params,
            &substitutions,
            span,
        )?;

        Ok(Type::Function {
            params: function
                .decl
                .params
                .iter()
                .zip(&function.signature.params)
                .zip(&function.signature.param_passings)
                .map(|((decl, ty), passing)| FunctionParamContract {
                    keyword_only: decl.keyword_only,
                    name: decl.name.clone(),
                    ty: substitute_type(ty, &substitutions),
                    passing: *passing,
                    has_default: decl.default.is_some(),
                    default_erased: false,
                })
                .collect(),
            return_type: Box::new(substitute_type(
                &function.signature.return_type,
                &substitutions,
            )),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_check_function_value_args(
        &self,
        params: &[FunctionParamContract],
        return_type: &Type,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected_return: Option<&Type>,
    ) -> Result<Type> {
        let erased_contract = params.iter().any(|param| param.name.is_empty());
        if erased_contract && args.iter().any(|argument| argument.name.is_some()) {
            return Err(Diagnostic::coded_at(
                "AU2003",
                span,
                "this function value's named argument contract was erased at a written-type or mutable-storage boundary, or because its possible targets do not all agree",
            )
            .with_help(
                "call it with the complete positional argument list, or keep one concrete named function value",
            ));
        }
        let positional_omission_uses_erased_default = args.len() < params.len()
            && params
                .iter()
                .skip(args.len())
                .any(|param| param.default_erased);
        if args.iter().all(|argument| argument.name.is_none())
            && positional_omission_uses_erased_default
        {
            return Err(Diagnostic::coded_at(
                "AU2003",
                span,
                format!(
                    "this function value has an erased default contract and requires the complete positional list of {} argument{}",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                ),
            ));
        }
        let synthetic_params = params
            .iter()
            .enumerate()
            .map(|(index, param)| Param {
                keyword_only: false,
                name: if param.name.is_empty() {
                    format!("argument{}", index + 1)
                } else {
                    param.name.clone()
                },
                mode: match param.passing {
                    ReceiverKind::Borrow => ParamMode::Default,
                    ReceiverKind::BorrowMut => ParamMode::BorrowMut,
                    ReceiverKind::Value => ParamMode::Own,
                },
                // The callable checker consumes the already-lowered
                // `param_types`; this placeholder is never lowered.
                ty: TypeRef::named("None", Vec::new(), false, span),
                default: param.has_default.then_some(Expr {
                    kind: ExprKind::BuiltinOmitted,
                    span,
                }),
                span,
            })
            .collect::<Vec<_>>();
        let param_types = params
            .iter()
            .map(|param| param.ty.clone())
            .collect::<Vec<_>>();
        let param_passings = params.iter().map(|param| param.passing).collect::<Vec<_>>();
        self.type_check_callable_args(
            "function value",
            &[],
            &synthetic_params,
            &param_passings,
            &param_types,
            return_type,
            &BTreeMap::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            args,
            span,
            locals,
            expected_return,
            HashMap::new(),
        )
    }

    pub(super) fn unsupported_call_target_diagnostic(
        &self,
        callee: &Expr,
        span: crate::diag::Span,
    ) -> Diagnostic {
        let bare_name = match &callee.kind {
            ExprKind::Name(name) => Some(name.as_str()),
            ExprKind::Specialize { expr, .. } => match &expr.kind {
                ExprKind::Name(name) => Some(name.as_str()),
                _ => None,
            },
            _ => None,
        };

        match bare_name {
            Some("set") => Diagnostic::coded_at(
                "AU2005",
                span,
                "empty set construction requires an explicit element type",
            )
            .with_help("write `set[T]()` with the intended element type"),
            Some("str") => Diagnostic::coded_at(
                "AU2005",
                span,
                "strings use quoted literals; `str(...)` is not a constructor",
            ),
            Some(form @ ("enumerate" | "zip")) => Diagnostic::coded_at(
                "AU2005",
                span,
                format!(
                    "`{form}` is a `for` loop form, not a value; write `for ... in {form}(...):`"
                ),
            ),
            Some("Some" | "None" | "Ok" | "Err" | "Closed") => Diagnostic::at(
                span,
                "bare enum variants require an expected enum type or a qualified form such as `Result.Ok(...)`",
            ),
            _ => Diagnostic::at(span, "unsupported call target"),
        }
    }

    pub(super) fn spawn_type_name_from_expr(expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Name(name) => Some(name.clone()),
            ExprKind::Member { object, field } => Some(format!(
                "{}.{}",
                Self::spawn_type_name_from_expr(object)?,
                field
            )),
            ExprKind::Group(inner) => Self::spawn_type_name_from_expr(inner),
            _ => None,
        }
    }

    pub(super) fn spawn_type_ref_from_expr(expr: &Expr) -> Option<TypeRef> {
        match &expr.kind {
            ExprKind::Name(_) | ExprKind::Member { .. } => Some(TypeRef::named(
                Self::spawn_type_name_from_expr(expr)?,
                Vec::new(),
                false,
                expr.span,
            )),
            ExprKind::Group(inner) => Self::spawn_type_ref_from_expr(inner),
            ExprKind::Index { object, index } => {
                let mut outer = Self::spawn_type_ref_from_expr(object)?;
                let crate::ast::TypeRefKind::Named { args, .. } = &mut outer.kind else {
                    return None;
                };
                if let ExprKind::Tuple(elements) = &index.kind {
                    args.extend(
                        elements
                            .iter()
                            .map(Self::spawn_type_ref_from_expr)
                            .collect::<Option<Vec<_>>>()?,
                    );
                } else {
                    args.push(Self::spawn_type_ref_from_expr(index)?);
                }
                Some(outer)
            }
            ExprKind::Tuple(elements) => Some(TypeRef::tuple(
                elements
                    .iter()
                    .map(Self::spawn_type_ref_from_expr)
                    .collect::<Option<Vec<_>>>()?,
                false,
                expr.span,
            )),
            _ => None,
        }
    }

    pub(super) fn resolve_spawn_callable(&self, callee: &Expr) -> Result<ResolvedCallableInfo> {
        let mut indexed_type_args = Vec::new();
        let (base_callee, callable_type_args) = match &callee.kind {
            ExprKind::Index { object, index } => {
                if let ExprKind::Tuple(elements) = &index.kind {
                    for element in elements {
                        let Some(type_arg) = Self::spawn_type_ref_from_expr(element) else {
                            return Err(Diagnostic::at(
                                callee.span,
                                "task target indexing is not a callable type specialization",
                            ));
                        };
                        indexed_type_args.push(type_arg);
                    }
                } else {
                    let Some(type_arg) = Self::spawn_type_ref_from_expr(index) else {
                        return Err(Diagnostic::at(
                            callee.span,
                            "task target indexing is not a callable type specialization",
                        ));
                    };
                    indexed_type_args.push(type_arg);
                }
                (&**object, Some(indexed_type_args.as_slice()))
            }
            _ => self.peel_specialization(callee),
        };

        match &base_callee.kind {
            ExprKind::Name(function_name) => {
                if self.resolve_extern_function_info(function_name).is_some() {
                    return Err(Diagnostic::coded_at(
                        "AU2999",
                        callee.span,
                        format!(
                            "extern function `{function_name}` is direct-call-only and cannot be handed to a task"
                        ),
                    )
                    .with_help(
                        "call the extern function synchronously inside a named Aura task function",
                    ));
                }
                let function = self.functions.get(function_name).ok_or_else(|| {
                    Diagnostic::at(
                        callee.span,
                        format!(
                            "task start target must be a callable function, found `{}`",
                            function_name
                        ),
                    )
                })?;
                let seed_substitutions = if let Some(type_args) = callable_type_args {
                    self.explicit_type_substitutions(
                        &function.decl.type_params,
                        type_args,
                        callee.span,
                        &format!("function `{function_name}`"),
                    )?
                } else {
                    HashMap::new()
                };
                Ok(ResolvedCallableInfo {
                    display_name: function_name.clone(),
                    decl: function.decl.clone(),
                    signature: function.signature.clone(),
                    type_param_bounds: function.type_param_bounds.clone(),
                    seed_substitutions,
                })
            }
            ExprKind::Member { object, field } => {
                let (base_object, object_type_args) = self.peel_specialization(object);
                if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(class_info) = namespace.classes.get(&item_name) {
                            if let Some(method) = class_info.methods.get(field) {
                                if method.decl.receiver.is_none() {
                                    let mut seed_substitutions = if let Some(type_args) =
                                        object_type_args
                                    {
                                        self.explicit_type_substitutions(
                                            &class_info.decl.type_params,
                                            type_args,
                                            object.span,
                                            &format!("class `{}`", item_name),
                                        )?
                                    } else {
                                        HashMap::new()
                                    };
                                    if let Some(type_args) = callable_type_args {
                                        let method_substitutions = self
                                            .explicit_type_substitutions(
                                                &method.decl.type_params,
                                                type_args,
                                                callee.span,
                                                &format!(
                                                    "associated method `{}.{field}`",
                                                    item_name
                                                ),
                                            )?;
                                        for (name, ty) in method_substitutions {
                                            if let Some(existing) =
                                                seed_substitutions.insert(name.clone(), ty.clone())
                                            {
                                                if existing != ty {
                                                    return Err(Diagnostic::at(
                                                        callee.span,
                                                        format!(
                                                            "conflicting explicit type arguments resolve `{name}` as both `{existing}` and `{ty}`"
                                                        ),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                    return Ok(ResolvedCallableInfo {
                                        display_name: format!("{}.{}", item_name, field),
                                        decl: method.decl.clone(),
                                        signature: method.signature.clone(),
                                        type_param_bounds: method.type_param_bounds.clone(),
                                        seed_substitutions,
                                    });
                                }
                            }
                        }
                    }
                }

                if let Some((module_path, function_name)) =
                    self.qualified_module_item(base_callee)
                {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if namespace.extern_functions.contains_key(&function_name) {
                            return Err(Diagnostic::coded_at(
                                "AU2999",
                                callee.span,
                                format!(
                                    "extern function `{module_path}.{function_name}` is direct-call-only and cannot be handed to a task"
                                ),
                            )
                            .with_help(
                                "call the extern function synchronously inside a named Aura task function",
                            ));
                        }
                        if let Some(function) = namespace
                            .functions
                            .get(&function_name)
                            .or_else(|| namespace.all_functions.get(&function_name))
                        {
                            let seed_substitutions = if let Some(type_args) = callable_type_args {
                                self.explicit_type_substitutions(
                                    &function.decl.type_params,
                                    type_args,
                                    callee.span,
                                    &format!("function `{}.{}`", module_path, function_name),
                                )?
                            } else {
                                HashMap::new()
                            };
                            return Ok(ResolvedCallableInfo {
                                display_name: format!("{}.{}", module_path, function_name),
                                decl: function.decl.clone(),
                                signature: function.signature.clone(),
                                type_param_bounds: function.type_param_bounds.clone(),
                                seed_substitutions,
                            });
                        }
                    }
                }

                if let ExprKind::Name(class_name) = &base_object.kind {
                    if let Some(class_info) = self.resolve_class_info(class_name) {
                        if let Some(method) = class_info.methods.get(field) {
                            if method.decl.receiver.is_none() {
                                let mut seed_substitutions =
                                    if let Some(type_args) = object_type_args {
                                    self.explicit_type_substitutions(
                                        &class_info.decl.type_params,
                                        type_args,
                                        object.span,
                                        &format!("class `{}`", class_name),
                                    )?
                                } else {
                                    HashMap::new()
                                };
                                if let Some(type_args) = callable_type_args {
                                    let method_substitutions =
                                        self.explicit_type_substitutions(
                                            &method.decl.type_params,
                                            type_args,
                                            callee.span,
                                            &format!(
                                                "associated method `{}.{field}`",
                                                class_name
                                            ),
                                        )?;
                                    for (name, ty) in method_substitutions {
                                        if let Some(existing) =
                                            seed_substitutions.insert(name.clone(), ty.clone())
                                        {
                                            if existing != ty {
                                                return Err(Diagnostic::at(
                                                    callee.span,
                                                    format!(
                                                        "conflicting explicit type arguments resolve `{name}` as both `{existing}` and `{ty}`"
                                                    ),
                                                ));
                                            }
                                        }
                                    }
                                }
                                return Ok(ResolvedCallableInfo {
                                    display_name: format!("{}.{}", class_name, field),
                                    decl: method.decl.clone(),
                                    signature: method.signature.clone(),
                                    type_param_bounds: method.type_param_bounds.clone(),
                                    seed_substitutions,
                                });
                            }
                        }
                    }
                }

                Err(Diagnostic::at(
                    callee.span,
                    "task starting currently supports named functions and associated methods without `self`",
                ))
            }
            _ => Err(Diagnostic::at(
                callee.span,
                "task starting currently supports named functions and associated methods without `self`",
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_check_callable_args(
        &self,
        callee_name: &str,
        callee_type_params: &[String],
        param_decls: &[Param],
        param_passings: &[ReceiverKind],
        param_types: &[Type],
        return_type: &Type,
        callee_type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        callee_rng_clone_safe_type_params: &BTreeSet<String>,
        callee_array_equality_safe_type_params: &BTreeSet<String>,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected_return: Option<&Type>,
        seed_substitutions: HashMap<String, Type>,
    ) -> Result<Type> {
        self.type_check_callable_args_detailed(
            callee_name,
            callee_type_params,
            param_decls,
            param_passings,
            param_types,
            return_type,
            callee_type_param_bounds,
            callee_rng_clone_safe_type_params,
            callee_array_equality_safe_type_params,
            args,
            span,
            locals,
            expected_return,
            seed_substitutions,
            ClosureArgumentPolicy::Reject,
        )
        .map(|checked| checked.return_type)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_check_callable_args_detailed(
        &self,
        callee_name: &str,
        callee_type_params: &[String],
        param_decls: &[Param],
        param_passings: &[ReceiverKind],
        param_types: &[Type],
        return_type: &Type,
        callee_type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        callee_rng_clone_safe_type_params: &BTreeSet<String>,
        callee_array_equality_safe_type_params: &BTreeSet<String>,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected_return: Option<&Type>,
        seed_substitutions: HashMap<String, Type>,
        closure_argument_policy: ClosureArgumentPolicy,
    ) -> Result<CheckedCallableTypes> {
        self.type_check_callable_args_seeded(
            callee_name,
            callee_type_params,
            param_decls,
            param_passings,
            param_types,
            return_type,
            callee_type_param_bounds,
            callee_rng_clone_safe_type_params,
            callee_array_equality_safe_type_params,
            args,
            span,
            locals,
            expected_return,
            seed_substitutions,
            Vec::new(),
            closure_argument_policy,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_check_callable_args_seeded(
        &self,
        callee_name: &str,
        callee_type_params: &[String],
        param_decls: &[Param],
        param_passings: &[ReceiverKind],
        param_types: &[Type],
        return_type: &Type,
        callee_type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        callee_rng_clone_safe_type_params: &BTreeSet<String>,
        callee_array_equality_safe_type_params: &BTreeSet<String>,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected_return: Option<&Type>,
        seed_substitutions: HashMap<String, Type>,
        seeded_borrowed_places: Vec<BorrowedCallPlace>,
        closure_argument_policy: ClosureArgumentPolicy,
    ) -> Result<CheckedCallableTypes> {
        let ordered_args = bind_call_arguments(
            callee_name,
            &callable_params_from_decl(param_decls),
            args,
            span,
            CallConvention::PositionalOrNamed,
        )?;

        let mut substitutions = seed_substitutions;
        if let Some(expected_return) = expected_return {
            if let Err(error) = unify_type_pattern(return_type, expected_return, &mut substitutions)
            {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "result type mismatch for {}: {}",
                        callee_name, error.message
                    ),
                ));
            }
        }
        let mut resolved_args = Vec::new();
        for (((argument, expected), param_decl), passing) in ordered_args
            .into_iter()
            .zip(param_types.iter())
            .zip(param_decls.iter())
            .zip(param_passings.iter())
        {
            let locals_before = locals.clone();
            let hinted_expected = substitute_type(expected, &substitutions);
            if *passing == ReceiverKind::BorrowMut && matches!(hinted_expected, Type::Union(_)) {
                if let Some(argument) = argument {
                    // A `mut` union parameter exposes the whole declared place;
                    // a refinement of the argument does not narrow the contract.
                    self.suppress_narrowing.set(true);
                    let actual =
                        self.type_of_expr_without_move_state(&argument.value, locals, None);
                    self.suppress_narrowing.set(false);
                    let actual = actual?;
                    if actual != hinted_expected {
                        return Err(Diagnostic::coded_at("AU2010", argument.value.span,
                            format!("mutable argument requires exact type '{hinted_expected}', found '{actual}'")));
                    }
                }
            }
            let actual = if let Some(argument) = argument {
                match self.type_of_expr_hint(&argument.value, locals, Some(&hinted_expected)) {
                    Ok(actual) => actual,
                    Err(error) if has_unresolved_type_params(&hinted_expected) => {
                        match self.type_of_expr(&argument.value, locals) {
                            Ok(actual) => actual,
                            Err(_) => return Err(error),
                        }
                    }
                    Err(error) => return Err(error),
                }
            } else {
                let default = param_decl.default.as_ref().ok_or_else(|| {
                    Diagnostic::at(
                        span,
                        "internal error: optional parameter is missing its default expression",
                    )
                })?;
                if matches!(default.kind, ExprKind::BuiltinOmitted) {
                    hinted_expected.clone()
                } else {
                    match self.type_of_expr_hint(default, locals, Some(&hinted_expected)) {
                        Ok(actual) => actual,
                        Err(error) if has_unresolved_type_params(&hinted_expected) => {
                            match self.type_of_expr(default, locals) {
                                Ok(actual) => actual,
                                Err(_) => return Err(error),
                            }
                        }
                        Err(error) => return Err(error),
                    }
                }
            };
            if *passing == ReceiverKind::Borrow && matches!(hinted_expected, Type::Union(_)) {
                if let Some(argument) = argument {
                    self.validate_borrowed_union_injection(
                        &argument.value,
                        &hinted_expected,
                        locals,
                    )?;
                }
            }
            let nested_move_span = argument
                .map(|argument| argument.value.span)
                .or_else(|| param_decl.default.as_ref().map(|default| default.span))
                .unwrap_or(span);
            let nested_moved_accesses = self.newly_moved_place_accesses(
                &locals_before,
                locals,
                "nested argument consumption",
                nested_move_span,
            );
            let mut nested_borrowed_places = Vec::new();
            if let Some(argument) = argument {
                self.collect_expr_borrowed_places(
                    &argument.value,
                    &locals_before,
                    &mut nested_borrowed_places,
                )?;
            }
            if let Err(error) = unify_type_pattern(expected, &actual, &mut substitutions) {
                let span = argument
                    .map(|argument| argument.span)
                    .unwrap_or(param_decl.span);
                let detail = self
                    .written_alias_type(&param_decl.ty, &hinted_expected)
                    .map(|expected| format!("expected {expected}, found {actual}"))
                    .unwrap_or(error.message);
                return Err(Diagnostic::at(
                    span,
                    format!("argument type mismatch for {}: {}", callee_name, detail),
                ));
            }
            resolved_args.push((
                argument,
                actual,
                nested_moved_accesses,
                nested_borrowed_places,
            ));
        }

        for type_param in callee_type_params {
            let Some(resolved) = substitutions.get(type_param) else {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "cannot infer type parameter `{}` for {}",
                        type_param, callee_name
                    ),
                ));
            };
            if matches!(
                resolved,
                Type::TypeParam(name)
                    if name == type_param && !self.type_params.contains_key(name)
            ) {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "cannot infer type parameter `{}` for {}",
                        type_param, callee_name
                    ),
                ));
            }
        }

        for (type_param, bounds) in callee_type_param_bounds {
            let Some(resolved_ty) = substitutions.get(type_param) else {
                continue;
            };
            let resolved_bounds = bounds
                .iter()
                .map(|bound| substitute_trait_bound(bound, &substitutions))
                .collect::<Vec<_>>();
            self.assert_type_satisfies_bounds(resolved_ty, &resolved_bounds, span)?;
        }

        self.enforce_rng_clone_obligations(
            callee_name,
            callee_rng_clone_safe_type_params,
            &substitutions,
            span,
        )?;
        self.enforce_array_equality_obligations(
            callee_name,
            callee_array_equality_safe_type_params,
            &substitutions,
            span,
        )?;

        let resolved_return_type = substitute_type(return_type, &substitutions);
        let resolved_param_types = param_types
            .iter()
            .map(|param| substitute_type(param, &substitutions))
            .collect::<Vec<_>>();

        if closure_argument_policy == ClosureArgumentPolicy::Reject {
            if let Some((argument, _actual, _, _)) = resolved_args
                .iter()
                .find(|(_, actual, _, _)| type_contains_loan_closure(actual))
            {
                return Err(Diagnostic::coded_at(
                    "AU3010",
                    argument.map(|argument| argument.span).unwrap_or(span),
                    "a call cannot erase the region of a closure containing a live view",
                )
                .with_help(
                    "keep the loan-bearing closure in its matching inferred local and call it directly",
                ));
            }
            if type_contains_loan_closure(&resolved_return_type) {
                return Err(Diagnostic::coded_at(
                    "AU3010",
                    span,
                    "a call cannot return a closure containing a live view",
                ));
            }
        }

        for (((argument, _, _, _), _param_decl), param_passing) in resolved_args
            .iter()
            .zip(param_decls.iter())
            .zip(param_passings.iter().copied())
        {
            if let Some(argument) = *argument {
                self.reject_mutable_returned_view_value(
                    &argument.value,
                    locals,
                    param_passing == ReceiverKind::BorrowMut,
                )?;
            }
        }

        let mut enclosing_accesses = seeded_borrowed_places.clone();
        for (
            (((argument, _actual, _nested_moved, _nested_borrowed), _expected), param_decl),
            param_passing,
        ) in resolved_args
            .iter()
            .zip(param_types.iter())
            .zip(param_decls.iter())
            .zip(param_passings.iter().copied())
        {
            let Some(argument) = *argument else {
                continue;
            };
            let label = format!("parameter `{}`", param_decl.name);
            self.collect_result_place_accesses(
                &argument.value,
                locals,
                param_passing,
                &label,
                &mut enclosing_accesses,
            )?;
        }
        for ((argument, _actual, _nested_moved, nested_borrowed), param_decl) in
            resolved_args.iter().zip(param_decls.iter())
        {
            let own_access = argument.map(|argument| {
                (
                    argument.value.span,
                    format!("parameter `{}`", param_decl.name),
                )
            });
            let other_enclosing = enclosing_accesses
                .iter()
                .filter(|access| {
                    own_access.as_ref().is_none_or(|(span, label)| {
                        access.origin_span != *span || access.param_name != *label
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            self.reject_retained_access_overlap(&other_enclosing, nested_borrowed)?;
        }

        let mut source_order_accesses = seeded_borrowed_places.clone();
        let mut branch_only_accesses: Vec<BorrowedCallPlace> = Vec::new();
        for source_argument in args {
            let Some(index) = resolved_args.iter().position(|(argument, _, _, _)| {
                argument.is_some_and(|argument| std::ptr::eq(argument, source_argument))
            }) else {
                continue;
            };
            self.reject_retained_access_overlap(&source_order_accesses, &resolved_args[index].2)?;
            let mut point_reads = Vec::new();
            self.collect_expr_place_reads(
                &source_argument.value,
                locals,
                "argument read",
                &mut point_reads,
            );
            let label = format!("parameter `{}`", param_decls[index].name);
            let mut direct_accesses = Vec::new();
            self.collect_result_place_accesses(
                &source_argument.value,
                locals,
                param_passings[index],
                &label,
                &mut direct_accesses,
            )?;
            point_reads.retain(|read| {
                !direct_accesses
                    .iter()
                    .any(|direct_access| read.path.overlaps(&direct_access.path))
            });
            self.reject_retained_access_overlap(&source_order_accesses, &point_reads)?;
            // An argument that is itself a place is compared pairwise below,
            // where a parameter-aware diagnostic is produced, so this
            // source-ordered pass only reports it against places that the
            // pairwise pass cannot see: the extra places contributed by an
            // earlier branching or composite argument.
            let whole_argument_place = self.borrow_call_place(&source_argument.value);
            let (whole_accesses, branch_accesses): (Vec<_>, Vec<_>) = direct_accesses
                .iter()
                .cloned()
                .partition(|access| whole_argument_place.as_ref() == Some(&access.path));
            self.reject_retained_access_overlap(&branch_only_accesses, &whole_accesses)?;
            self.reject_retained_access_overlap(&source_order_accesses, &branch_accesses)?;
            source_order_accesses.extend(direct_accesses);
            branch_only_accesses.extend(branch_accesses);
        }

        let mut borrowed_places = seeded_borrowed_places;
        for (
            (
                ((argument, actual, nested_moved_accesses, _nested_borrowed_places), expected),
                param_decl,
            ),
            param_passing,
        ) in resolved_args
            .into_iter()
            .zip(param_types.iter())
            .zip(param_decls.iter())
            .zip(param_passings.iter().copied())
        {
            let expected = substitute_type(expected, &substitutions);
            let repeatable_closure_compatible = matches!(
                closure_argument_policy,
                ClosureArgumentPolicy::RepeatableParameter(name) if name == param_decl.name
            ) && matches!(
                (&actual, &expected),
                (
                    Type::Closure {
                        params: actual_params,
                        return_type: actual_return,
                        call_kind: ClosureCallKind::Repeatable,
                        ..
                    },
                    Type::Function {
                        params: expected_params,
                        return_type: expected_return,
                    }
                ) if actual_params.as_ref() == expected_params && actual_return == expected_return
            );
            if actual != expected && !repeatable_closure_compatible {
                let span = argument
                    .map(|argument| argument.span)
                    .unwrap_or(param_decl.span);
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "argument type mismatch for {}: expected `{}`, found `{}`",
                        callee_name, expected, actual
                    ),
                ));
            }
            if let Some(argument) = argument {
                if param_passing == ReceiverKind::Value {
                    self.reject_owned_view_value(
                        &argument.value,
                        locals,
                        &format!("owned parameter `{}`", param_decl.name),
                    )?;
                }
                match param_passing {
                    ReceiverKind::Value => {
                        if self.is_copy_type(&expected) {
                            if let Some(place) = self.borrow_call_place(&argument.value) {
                                self.ensure_place_not_shared_by_match_for_move(
                                    &place,
                                    argument.span,
                                    locals,
                                )?;
                            }
                        } else {
                            if let Some(place) = self.borrow_call_place(&argument.value) {
                                self.reject_overlapping_borrow(
                                    &borrowed_places,
                                    &place,
                                    ReceiverKind::Value,
                                    &param_decl.name,
                                    callee_name,
                                    argument.span,
                                )?;
                                borrowed_places.push(BorrowedCallPlace {
                                    path: place,
                                    passing: ReceiverKind::Value,
                                    param_name: param_decl.name.clone(),
                                    origin_span: argument.value.span,
                                });
                            }
                            for moved_access in nested_moved_accesses {
                                self.reject_overlapping_borrow(
                                    &borrowed_places,
                                    &moved_access.path,
                                    ReceiverKind::Value,
                                    &param_decl.name,
                                    callee_name,
                                    moved_access.origin_span,
                                )?;
                                borrowed_places.push(BorrowedCallPlace {
                                    path: moved_access.path,
                                    passing: ReceiverKind::Value,
                                    param_name: param_decl.name.clone(),
                                    origin_span: moved_access.origin_span,
                                });
                            }
                            self.consume_value_expr(&argument.value, locals)?;
                        }
                    }
                    ReceiverKind::Borrow => {
                        if let Some(place) = self.borrow_call_place(&argument.value) {
                            self.reject_overlapping_borrow(
                                &borrowed_places,
                                &place,
                                ReceiverKind::Borrow,
                                &param_decl.name,
                                callee_name,
                                argument.span,
                            )?;
                            borrowed_places.push(BorrowedCallPlace {
                                path: place,
                                passing: ReceiverKind::Borrow,
                                param_name: param_decl.name.clone(),
                                origin_span: argument.value.span,
                            });
                        }
                    }
                    ReceiverKind::BorrowMut => {
                        let argument_place = self.borrow_call_place(&argument.value);
                        if let Some(place) = argument_place.as_ref() {
                            self.ensure_place_mutation_allowed(place, argument.span, locals)?;
                            self.invalidate_narrowing(
                                place,
                                argument.span,
                                "a call with mutable access",
                                locals,
                            );
                            let through_view = locals
                                .get(&place.root)
                                .and_then(|binding| binding.view.as_ref())
                                .map(|_| place.root.as_str());
                            self.ensure_place_not_locked_by_view(
                                place,
                                through_view,
                                argument.span,
                                locals,
                            )?;
                        }
                        if !self.is_mutable_place(&argument.value, locals)? {
                            return Err(Diagnostic::at(
                                argument.span,
                                format!(
                                    "argument for parameter `{}` in {} must be a mutable place",
                                    param_decl.name, callee_name
                                ),
                            ));
                        }
                        if let Some(place) = argument_place {
                            self.reject_overlapping_borrow(
                                &borrowed_places,
                                &place,
                                ReceiverKind::BorrowMut,
                                &param_decl.name,
                                callee_name,
                                argument.span,
                            )?;
                            borrowed_places.push(BorrowedCallPlace {
                                path: place,
                                passing: ReceiverKind::BorrowMut,
                                param_name: param_decl.name.clone(),
                                origin_span: argument.value.span,
                            });
                        }
                    }
                }
            }
        }

        self.invalidate_match_borrow_bindings_for_borrowed_places(&borrowed_places, locals);

        Ok(CheckedCallableTypes {
            params: resolved_param_types,
            return_type: resolved_return_type,
        })
    }

    pub(super) fn require_task_startable_function(
        &self,
        function_name: &str,
        params: &[Param],
        param_passings: &[ReceiverKind],
        span: crate::diag::Span,
    ) -> Result<()> {
        if let Some(param) = params
            .iter()
            .zip(param_passings)
            .find_map(|(param, passing)| (*passing == ReceiverKind::BorrowMut).then_some(param))
        {
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                format!(
                    "task starting does not support mutable parameter `{}` on function `{}`; child tasks cannot write back through the starting call frame",
                    param.name, function_name
                ),
            ));
        }
        Ok(())
    }
}
