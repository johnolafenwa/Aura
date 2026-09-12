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
    /// Empty for an explicit positional-only slot of a written
    /// `def(T) -> R` type; a named slot or an inferred value carries the
    /// exposed name. Names, the keyword-only boundary, and default
    /// availability are part of the complete callable contract (Q17 A,
    /// Q19 A); ABI equality of `Type` ignores them.
    pub name: String,
    pub ty: Type,
    pub passing: ReceiverKind,
    /// True when the contract promises a default for this slot (`= ...` in
    /// a written type, or a declaration default). Only the selected target
    /// supplies the default expression.
    pub has_default: bool,
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

impl ClosureCallKind {
    /// Shared < Mutable < Consuming: a value may be packed into a callable
    /// type of the same or a weaker kind, never a stronger one (C2).
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Repeatable => 0,
            Self::MutableRepeatable => 1,
            Self::Consuming => 2,
        }
    }

    pub(crate) fn admits(self, source: Self) -> bool {
        source.rank() <= self.rank()
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Repeatable => "Shared",
            Self::MutableRepeatable => "Mutable",
            Self::Consuming => "Consuming",
        }
    }

    pub(crate) fn spelling(self) -> &'static str {
        match self {
            Self::Repeatable => "",
            Self::MutableRepeatable => "mut ",
            Self::Consuming => "own ",
        }
    }
}

/// An owned, environment-erased callable storage type (C1, Q13 A): the
/// complete call contract plus the call kind, with the capture set erased.
/// Non-Copy and non-cloneable; Transfer only as a checked `TaskCallable`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallableType {
    pub task: bool,
    pub call_kind: ClosureCallKind,
    pub params: Vec<FunctionParamContract>,
    pub return_type: Type,
}

impl CallableType {
    /// The thin contract every packed value must meet.
    pub(crate) fn contract(&self) -> Type {
        Type::Function {
            params: self.params.clone(),
            return_type: Box::new(self.return_type.clone()),
        }
    }

    pub(crate) fn constructor_name(&self) -> &'static str {
        if self.task {
            "TaskCallable"
        } else {
            "Callable"
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClosureCapture {
    pub name: String,
    pub ty: Type,
    pub mode: ClosureCaptureMode,
    pub span: crate::diag::Span,
    /// An owned (`Copy`/`Move`) capture the body mutates in place (C2):
    /// the closure is Mutable and the environment keeps the updated value.
    #[serde(default)]
    pub mutated: bool,
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

/// Synthetic name of a positional-only contract slot while binding a call
/// through a function value; user arguments can never spell it.
pub(crate) const POSITIONAL_ONLY_SLOT_PREFIX: &str = "__positional_slot_";

/// Rewrites binder diagnostics that name a synthetic positional-only slot.
pub(crate) fn describe_positional_only_slots(mut diagnostic: Diagnostic) -> Diagnostic {
    if let Some(start) = diagnostic.message.find(POSITIONAL_ONLY_SLOT_PREFIX) {
        let rest = &diagnostic.message[start + POSITIONAL_ONLY_SLOT_PREFIX.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            let spelled = format!("`{POSITIONAL_ONLY_SLOT_PREFIX}{digits}`");
            diagnostic.message = diagnostic
                .message
                .replace(
                    &format!("argument {spelled}"),
                    &format!("positional argument {digits}"),
                )
                .replace(&spelled, &format!("positional argument {digits}"));
        }
    }
    diagnostic
}

/// Describes one contract slot for diagnostics.
fn contract_slot_label(contract: &FunctionParamContract, index: usize) -> String {
    if contract.name.is_empty() {
        format!("positional parameter {}", index + 1)
    } else {
        format!("parameter `{}`", contract.name)
    }
}

/// Explains why a value with the `actual` slot contract cannot stand where
/// the `expected` slot contract is written (Q17 A). A written destination may
/// hide an exposed name, drop default availability, or restrict a named
/// positional-or-keyword slot to keyword-only. It cannot make a keyword-only
/// slot positional, rename a slot, or promise a default the source lacks.
/// Parameter types and capabilities are compared by the caller.
pub(crate) fn callable_slot_admission(
    expected: &FunctionParamContract,
    actual: &FunctionParamContract,
    index: usize,
) -> std::result::Result<(), String> {
    if expected.keyword_only {
        if actual.name.is_empty() {
            return Err(format!(
                "{} is positional-only, but the destination makes it keyword-only `{}`",
                contract_slot_label(actual, index),
                expected.name
            ));
        }
        if actual.name != expected.name {
            return Err(format!(
                "keyword-only parameter `{}` would be renamed to `{}`; renaming requires an explicit wrapper",
                actual.name, expected.name
            ));
        }
    } else {
        if actual.keyword_only {
            return Err(format!(
                "parameter `{}` is keyword-only and cannot become positional",
                actual.name
            ));
        }
        if !expected.name.is_empty() {
            if actual.name.is_empty() {
                return Err(format!(
                    "{} has no exposed name, but the destination names it `{}`",
                    contract_slot_label(actual, index),
                    expected.name
                ));
            }
            if actual.name != expected.name {
                return Err(format!(
                    "parameter `{}` would be renamed to `{}`; renaming requires an explicit wrapper",
                    actual.name, expected.name
                ));
            }
        }
    }
    if expected.has_default && !actual.has_default {
        return Err(format!(
            "the destination promises a default for {}, which the source does not declare",
            contract_slot_label(expected, index)
        ));
    }
    Ok(())
}

/// Checks that every slot of `actual` may stand where `expected` is written,
/// including parameter types and capabilities.
pub(crate) fn callable_contract_admission(
    expected: &[FunctionParamContract],
    actual: &[FunctionParamContract],
) -> std::result::Result<(), String> {
    if expected.len() != actual.len() {
        return Err(format!(
            "expected {} parameter{}, found {}",
            expected.len(),
            if expected.len() == 1 { "" } else { "s" },
            actual.len()
        ));
    }
    for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
        if expected.ty != actual.ty || expected.passing != actual.passing {
            return Err(format!(
                "{} has a different type or capability",
                contract_slot_label(expected, index)
            ));
        }
        callable_slot_admission(expected, actual, index)?;
    }
    Ok(())
}

/// Walks two ABI-equal types and checks every callable position: the value's
/// complete contract must be admitted by the written destination contract.
pub(crate) fn check_callable_positions(
    expected: &Type,
    actual: &Type,
) -> std::result::Result<(), String> {
    match (expected, actual) {
        (
            Type::Function {
                params: expected_params,
                return_type: expected_return,
            },
            Type::Function {
                params: actual_params,
                return_type: actual_return,
            },
        ) => {
            callable_contract_admission(expected_params, actual_params)?;
            check_callable_positions(expected_return, actual_return)
        }
        (
            Type::Function {
                params: expected_params,
                return_type: expected_return,
            },
            Type::Closure {
                params: actual_params,
                return_type: actual_return,
                ..
            },
        ) => {
            callable_contract_admission(expected_params, actual_params)?;
            check_callable_positions(expected_return, actual_return)
        }
        (
            Type::Closure {
                params: expected_params,
                return_type: expected_return,
                ..
            },
            Type::Closure {
                params: actual_params,
                return_type: actual_return,
                ..
            },
        ) => {
            callable_contract_admission(expected_params, actual_params)?;
            check_callable_positions(expected_return, actual_return)
        }
        (Type::Callable(expected_callable), Type::Callable(actual_callable)) => {
            if expected_callable.task != actual_callable.task
                || expected_callable.call_kind != actual_callable.call_kind
            {
                return Err(format!(
                    "expected `{expected}`, found `{actual}`; pack the value through the destination constructor to change its storage kind"
                ));
            }
            callable_contract_admission(&expected_callable.params, &actual_callable.params)?;
            check_callable_positions(&expected_callable.return_type, &actual_callable.return_type)
        }
        (Type::Tuple(expected), Type::Tuple(actual)) => expected
            .iter()
            .zip(actual)
            .try_for_each(|(expected, actual)| check_callable_positions(expected, actual)),
        (Type::Named(_, expected), Type::Named(_, actual)) => expected
            .iter()
            .zip(actual)
            .try_for_each(|(expected, actual)| check_callable_positions(expected, actual)),
        (Type::Union(expected), Type::Union(actual)) => expected
            .members
            .iter()
            .zip(&actual.members)
            .try_for_each(|(expected, actual)| check_callable_positions(expected, actual)),
        _ => Ok(()),
    }
}

/// Requires identical complete contracts at every callable position of two
/// ABI-equal types: an inferred join never invents a common contract.
pub(crate) fn same_callable_contracts(
    left: &Type,
    right: &Type,
) -> std::result::Result<(), String> {
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
        ) => {
            if let Some((index, (left_param, right_param))) = left_params
                .iter()
                .zip(right_params)
                .enumerate()
                .find(|(_, (left, right))| left != right)
            {
                return Err(format!(
                    "{} differs from {} in its name, keyword-only boundary, or default availability",
                    contract_slot_label(left_param, index),
                    contract_slot_label(right_param, index)
                ));
            }
            same_callable_contracts(left_return, right_return)
        }
        (Type::Callable(left_callable), Type::Callable(right_callable)) => {
            if let Some((index, (left_param, right_param))) = left_callable
                .params
                .iter()
                .zip(&right_callable.params)
                .enumerate()
                .find(|(_, (left, right))| left != right)
            {
                return Err(format!(
                    "{} differs from {} in its name, keyword-only boundary, or default availability",
                    contract_slot_label(left_param, index),
                    contract_slot_label(right_param, index)
                ));
            }
            same_callable_contracts(&left_callable.return_type, &right_callable.return_type)
        }
        (Type::Tuple(left), Type::Tuple(right)) => left
            .iter()
            .zip(right)
            .try_for_each(|(left, right)| same_callable_contracts(left, right)),
        (Type::Named(_, left), Type::Named(_, right)) => left
            .iter()
            .zip(right)
            .try_for_each(|(left, right)| same_callable_contracts(left, right)),
        (Type::Union(left), Type::Union(right)) => left
            .members
            .iter()
            .zip(&right.members)
            .try_for_each(|(left, right)| same_callable_contracts(left, right)),
        _ => Ok(()),
    }
}

pub(crate) fn callable_contract_mismatch(
    span: crate::diag::Span,
    reason: impl std::fmt::Display,
) -> Diagnostic {
    Diagnostic::coded_at(
        "AU2015",
        span,
        format!("callable contract mismatch: {reason}"),
    )
    .with_help(
        "annotate the destination with a contract every value satisfies, or adapt a value explicitly through a thin callable alias call such as `Alias(function)`",
    )
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
            // A packed Shared value is borrowed for the operation with its
            // own contract (C10); Mutable and Consuming values stay out.
            Type::Callable(callable) if callable.call_kind == ClosureCallKind::Repeatable => {
                (callable.params.clone(), Box::new(callable.return_type.clone()))
            }
            Type::Closure {
                call_kind: ClosureCallKind::Consuming | ClosureCallKind::MutableRepeatable,
                ..
            }
            | Type::Callable(_) => {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    callback.span,
                    format!(
                        "`{collection}.{method_name}` callback must be repeatable, found `{callback_ty}`"
                    ),
                )
                .with_help(
                    "clone or precompute the consumed capture outside the lambda, or use a named function that does not consume or mutate closure state",
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
        // The site calls its callback positionally, so a keyword-only element
        // parameter cannot masquerade as a positional callback (C10), and a
        // view result would escape the element.
        if params.first().is_some_and(|param| param.keyword_only) {
            return Err(Diagnostic::coded_at(
                "AU2004",
                callback.span,
                format!(
                    "`{collection}.{method_name}` calls its callback positionally, but parameter `{}` of `{callback_ty}` is keyword-only",
                    params[0].name
                ),
            ));
        }
        if matches!(return_type.as_ref(), Type::ReturnedView(_)) {
            return Err(Diagnostic::coded_at(
                "AU3010",
                callback.span,
                format!(
                    "`{collection}.{method_name}` callback cannot return a view, found `{callback_ty}`"
                ),
            ));
        }
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
                // A required parameter may follow a defaulted one only across
                // the `*` boundary or inside the keyword-only group, where
                // binding is by name and order carries no meaning.
                None if saw_default && !param.keyword_only => {
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
            (None, Some(expected @ Type::Callable(callable))) => {
                return Err(callable_contract_mismatch(
                    span,
                    format!(
                        "implicit erased storage; a lambda becomes `{expected}` only through an explicit `{}(lambda ...)` constructor call",
                        callable.constructor_name()
                    ),
                ))
            }
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
            // A written contract's exposed names, keyword-only boundary, and
            // default promises are part of the contract a lambda must meet
            // (Q19 A); a builtin callback context supplies only positional
            // parameter types.
            let contract_is_written = callable_context.is_none();
            for (index, (param, expected_param)) in params.iter().zip(expected_params).enumerate() {
                if contract_is_written {
                    if expected_param.has_default {
                        return Err(callable_contract_mismatch(
                            param.span,
                            format!(
                                "a lambda cannot promise the default for {}; declare a named function",
                                contract_slot_label(expected_param, index)
                            ),
                        ));
                    }
                    if param.keyword_only != expected_param.keyword_only {
                        return Err(callable_contract_mismatch(
                            param.span,
                            format!(
                                "lambda parameter `{}` {} the `*` keyword-only boundary, but the expected contract {}",
                                param.name,
                                if param.keyword_only { "follows" } else { "precedes" },
                                if expected_param.keyword_only {
                                    "makes it keyword-only"
                                } else {
                                    "keeps it positional"
                                }
                            ),
                        ));
                    }
                    if !expected_param.name.is_empty() && param.name != expected_param.name {
                        return Err(callable_contract_mismatch(
                            param.span,
                            format!(
                                "lambda parameter `{}` must be named `{}` to meet the expected contract",
                                param.name, expected_param.name
                            ),
                        ));
                    }
                }
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
                mutated: false,
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
        // Owned captures mutated by the body make the closure Mutable (C2);
        // the set is scoped to this lambda so nested lambdas do not leak.
        let outer_mutated_captures = std::mem::take(&mut *self.mutated_captures.borrow_mut());
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
        let mutated_captures = std::mem::replace(
            &mut *self.mutated_captures.borrow_mut(),
            outer_mutated_captures,
        );
        for capture in &mut captures {
            capture.mutated = matches!(
                capture.mode,
                ClosureCaptureMode::Copy | ClosureCaptureMode::Move
            ) && mutated_captures.contains(&capture.name);
        }
        let call_kind = if captures.iter().any(|capture| {
            lambda_locals
                .get(&capture.name)
                .is_some_and(|binding| binding.moved || !binding.moved_fields.is_empty())
        }) {
            ClosureCallKind::Consuming
        } else if captures
            .iter()
            .any(|capture| capture.mode == ClosureCaptureMode::MutableView || capture.mutated)
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
        // A parameter-origin view result is part of the complete contract
        // (C9, Q22 A); only a receiver origin has no place in a value type.
        if function
            .decl
            .view_return
            .as_ref()
            .is_some_and(|view| view.origin == "self")
        {
            return Err(Diagnostic::coded_at(
                "AU3010",
                span,
                format!(
                    "view-returning callable `{}` cannot be stored as a structural function value",
                    function.decl.name
                ),
            )
            .with_help(
                "call it directly and bind the result with `view`, because a structural `def(...)` type can name only a parameter as its view origin",
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

        let params = function
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
            })
            .collect::<Vec<_>>();
        let return_type = super::wrap_returned_view(
            &params,
            substitute_type(&function.signature.return_type, &substitutions),
            function.decl.view_return.as_ref(),
        );
        Ok(Type::Function {
            params,
            return_type: Box::new(return_type),
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
        if let Some((argument, name)) = args.iter().find_map(|argument| {
            argument
                .name
                .as_deref()
                .filter(|name| !params.iter().any(|param| param.name == *name))
                .map(|name| (argument, name))
        }) {
            if params.iter().any(|param| param.name.is_empty()) {
                return Err(Diagnostic::coded_at(
                    "AU2004",
                    argument.span,
                    format!(
                        "this function value's contract has no parameter named `{name}`"
                    ),
                )
                .with_help(
                    "a positional-only slot of a written `def(...)` type is called positionally; name the slot in the type to call it by name",
                ));
            }
        }
        let synthetic_params = params
            .iter()
            .enumerate()
            .map(|(index, param)| Param {
                keyword_only: param.keyword_only,
                name: if param.name.is_empty() {
                    format!("{POSITIONAL_ONLY_SLOT_PREFIX}{}", index + 1)
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
        // A call through a view-returning contract has the pointee type; the
        // returned-view binding rules see the contract through the callee.
        let return_type = super::returned_view_pointee(return_type);
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
        )
        .map_err(describe_positional_only_slots)?;

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
                if error.code == "AU2010" {
                    return Err(Diagnostic::coded_at(
                        "AU2010",
                        span,
                        format!(
                            "argument type mismatch for {}: {}",
                            callee_name, error.message
                        ),
                    ));
                }
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
            let unresolved = match substitutions.get(type_param) {
                None => true,
                Some(resolved) => matches!(
                    resolved,
                    Type::TypeParam(name)
                        if name == type_param && !self.type_params.contains_key(name)
                ),
            };
            if !unresolved {
                continue;
            }
            // A parameter that only occurs as a union member cannot be read
            // back from a member argument such as `None`; the union rule asks
            // for explicit specialization instead of inverted normalization
            // (ADR-0052 A7).
            let only_union_member = param_types.iter().any(|param| {
                matches!(param, Type::Union(union)
                    if union.members.iter().any(|member| matches!(member, Type::TypeParam(name) if name == type_param)))
            });
            if only_union_member {
                return Err(Diagnostic::coded_at(
                    "AU2010",
                    span,
                    format!(
                        "cannot infer type parameter `{}` for {} from its union member arguments; specialize the callable explicitly",
                        type_param, callee_name
                    ),
                ));
            }
            return Err(Diagnostic::at(
                span,
                format!(
                    "cannot infer type parameter `{}` for {}",
                    type_param, callee_name
                ),
            ));
        }

        // An argument typed before its union parameter resolved is typed
        // again under the resolved union so the checker records the boundary
        // injection it implies (ADR-0052 A7).
        for ((argument, actual, _, _), expected) in resolved_args.iter_mut().zip(param_types.iter())
        {
            let Some(argument) = argument else {
                continue;
            };
            if !has_unresolved_type_params(expected) {
                continue;
            }
            let resolved = substitute_type(expected, &substitutions);
            if *actual == resolved || !matches!(resolved, Type::Union(_)) {
                continue;
            }
            if let Ok(retyped) =
                self.type_of_expr_without_move_state(&argument.value, locals, Some(&resolved))
            {
                *actual = retyped;
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
            // A repeatable callback parameter admits a Repeatable closure or a
            // packed Shared value with an ABI-equal contract that the site can
            // call positionally (C10); the value is borrowed, never cloned or
            // erased.
            let repeatable_closure_compatible = matches!(
                closure_argument_policy,
                ClosureArgumentPolicy::RepeatableParameter(name) if name == param_decl.name
            ) && match (&actual, &expected) {
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
                    },
                ) => actual_params.as_ref() == expected_params && actual_return == expected_return,
                (
                    Type::Callable(callable),
                    Type::Function {
                        params: expected_params,
                        return_type: expected_return,
                    },
                ) => {
                    callable.call_kind == ClosureCallKind::Repeatable
                        && callable.params == *expected_params
                        && callable.return_type == **expected_return
                        && !callable.params.iter().any(|param| param.keyword_only)
                }
                _ => false,
            };
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

/// Name of the synthesized receiver capture of a bound method closure (C6,
/// Q18 A). Source locals cannot spell a `__` prefix, so it never collides.
pub(crate) const BOUND_RECEIVER_CAPTURE: &str = "__receiver";

struct BoundMethodTarget<'a> {
    /// The selected implementation: its slots and body run the call.
    decl: &'a FunctionDecl,
    /// The public contract a trait method exposes (parameter names, the
    /// keyword-only boundary, default availability): the trait's own
    /// declaration (Q19 A). `None` for an inherent method, whose own
    /// declaration is its contract.
    contract: Option<&'a FunctionDecl>,
    signature: &'a FunctionSignature,
    type_param_bounds: &'a BTreeMap<String, Vec<TraitBound>>,
    substitutions: HashMap<String, Type>,
    display: String,
}

impl FunctionChecker<'_> {
    /// The closure metadata registered for a bound method at `span` in the
    /// current closure owner, if the member expression there bound one.
    pub(super) fn bound_method_closure_at(&self, span: crate::diag::Span) -> Option<ClosureInfo> {
        let id = ClosureId::new(self.module_name, self.closure_owner.clone(), span);
        self.closure_infos
            .borrow()
            .get(&id)
            .filter(|info| {
                matches!(info.captures.as_slice(), [capture] if capture.name == BOUND_RECEIVER_CAPTURE)
            })
            .cloned()
    }

    /// Resolves `Class.method` / `module.Class.method` naming an associated
    /// method (no receiver) together with its display owner.
    pub(super) fn associated_method_target(
        &self,
        object: &Expr,
        field: &str,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<(&super::ClassInfo, &super::MethodInfo, String)> {
        let (base_object, _) = self.peel_specialization(object);
        let (class, owner) = match &base_object.kind {
            ExprKind::Name(class_name) if !locals.contains_key(class_name) => {
                (self.resolve_class_info(class_name)?, class_name.clone())
            }
            _ => {
                let (module_path, class_name) = self.qualified_module_item(base_object)?;
                let class = self
                    .module_namespace(&module_path)?
                    .classes
                    .get(&class_name)?;
                (class, format!("{module_path}.{class_name}"))
            }
        };
        let method = class.methods.get(field)?;
        method
            .decl
            .receiver
            .is_none()
            .then_some((class, method, owner))
    }

    /// An associated method named without a call is a thin function value
    /// carrying the method's complete contract (C6). Generic owners still
    /// need a call because the value would have no type arguments.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn associated_method_value_type(
        &self,
        class: &super::ClassInfo,
        method: &super::MethodInfo,
        owner: &str,
        field: &str,
        object: &Expr,
        expected: Option<&Type>,
        explicit_type_args: Option<&[Type]>,
        span: crate::diag::Span,
    ) -> Result<Type> {
        if self.is_external_module(&class.module_name) && !method.decl.public {
            return Err(Diagnostic::at(
                span,
                format!("method `{field}` is private on `{}`", class.decl.name),
            ));
        }
        if !class.decl.type_params.is_empty() || matches!(object.kind, ExprKind::Specialize { .. })
        {
            return Err(Diagnostic::coded_at(
                "AU2005",
                span,
                format!(
                    "associated method values on generic classes are not supported in this language version; call `{owner}.{field}(...)` directly or wrap it in a named function"
                ),
            ));
        }
        let function = FunctionInfo {
            module_name: class.module_name.clone(),
            decl: method.decl.clone(),
            signature: method.signature.clone(),
            type_param_bounds: method.type_param_bounds.clone(),
        };
        self.function_value_type(
            &function,
            expected,
            explicit_type_args,
            span,
            &format!("associated method `{owner}.{field}`"),
        )
    }

    /// `receiver.method` outside call position is a compiler-synthesized
    /// closure whose single capture is the receiver (C6, Q18 A). The closure
    /// keeps the method's complete contract, and its call kind follows the
    /// receiver capability: `self` is Repeatable, `mut self` is Mutable, and
    /// `own self` is Consuming. Returns `None` when the member is not a
    /// receiver method so field reads and the ordinary member errors continue.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_of_bound_method(
        &self,
        expr: &Expr,
        object: &Expr,
        field: &str,
        object_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
        explicit_type_args: Option<&[Type]>,
    ) -> Result<Option<Type>> {
        let Type::Named(type_name, type_args) = object_ty else {
            return Ok(None);
        };
        if self
            .builtin_enum_variant_payload(object_ty, type_name, field)
            .is_some()
            || super::BuiltinMember::resolve(type_name, field).is_some()
        {
            return Ok(None);
        }
        let span = expr.span;
        let class = self.resolve_class_info(type_name);
        if class.is_some_and(|class| class.fields.contains_key(field)) {
            return Ok(None);
        }
        let class_target = class.and_then(|class| {
            let method = class.methods.get(field)?;
            if self.is_external_module(&class.module_name) && !method.decl.public {
                return None;
            }
            Some(BoundMethodTarget {
                decl: &method.decl,
                contract: None,
                signature: &method.signature,
                type_param_bounds: &method.type_param_bounds,
                substitutions: substitutions_from_decl_type_args(
                    &class.decl.type_params,
                    type_args,
                ),
                display: format!("{}.{}", class.decl.name, field),
            })
        });
        let target = match class_target {
            Some(target) => target,
            None => match self.trait_method_for_concrete_type(object_ty, field, span)? {
                Some((trait_impl, method, substitutions)) => BoundMethodTarget {
                    decl: &method.decl,
                    contract: self.trait_method_contract(trait_impl, field),
                    signature: &method.signature,
                    type_param_bounds: &method.type_param_bounds,
                    substitutions,
                    display: format!("{}.{}", trait_impl.trait_name, field),
                },
                None => return Ok(None),
            },
        };
        let Some(receiver) = target.decl.receiver else {
            return Ok(None);
        };
        // A generic method's type arguments must be concrete from explicit
        // specialization or the expected contract (C6); a bare reference has
        // no concrete callable type.
        let substitutions =
            self.bound_method_substitutions(&target, field, expected, explicit_type_args, span)?;
        // A view of another explicit parameter stays in the bound contract
        // (C9); only the receiver origin moves into hidden storage.
        if target
            .decl
            .view_return
            .as_ref()
            .is_some_and(|view| view.origin == "self")
        {
            return Err(Diagnostic::coded_at(
                "AU3010",
                span,
                format!(
                    "view-returning method `{}` cannot become a bound method value",
                    target.display
                ),
            )
            .with_help(
                "call it directly and bind the result with `view`, because a bound method's contract cannot name its moved receiver as a view origin",
            ));
        }
        // A trait method selected for a concrete receiver exposes the
        // trait's public contract — its parameter names, keyword-only
        // boundary, and default availability — not the implementation's
        // local names (Q19 A); every slot forwards to the implementation by
        // ordinal, and so does a returned-view origin.
        let public_params = target
            .contract
            .map(|contract| contract.params.as_slice())
            .unwrap_or(target.decl.params.as_slice());
        let params = public_params
            .iter()
            .zip(&target.signature.params)
            .zip(&target.signature.param_passings)
            .map(|((param, ty), passing)| FunctionParamContract {
                keyword_only: param.keyword_only,
                name: param.name.clone(),
                ty: substitute_type(ty, &substitutions),
                passing: *passing,
                has_default: param.default.is_some(),
            })
            .collect::<Vec<_>>();
        let view_return = target.decl.view_return.as_ref().map(|view| {
            let origin = target
                .decl
                .params
                .iter()
                .position(|param| param.name == view.origin)
                .and_then(|ordinal| params.get(ordinal))
                .map_or_else(|| view.origin.clone(), |param| param.name.clone());
            crate::ast::ViewReturn {
                mutable: view.mutable,
                origin,
                span: view.span,
            }
        });
        let return_type = super::wrap_returned_view(
            &params,
            substitute_type(&target.signature.return_type, &substitutions),
            view_return.as_ref(),
        );
        // A Copy receiver is snapshotted into the closure; any other receiver
        // must be an owned place or a fresh temporary that moves in once.
        let mode = if self.is_copy_type(object_ty) {
            ClosureCaptureMode::Copy
        } else {
            self.acquire_bound_receiver(object, &target.display, locals)?;
            ClosureCaptureMode::Move
        };
        let mutated = receiver == ReceiverKind::BorrowMut;
        let call_kind = super::closure_call_kind_for(receiver);
        let id = ClosureId::new(self.module_name, self.closure_owner.clone(), span);
        let info = ClosureInfo {
            id: id.clone(),
            span,
            params,
            return_type,
            captures: vec![ClosureCapture {
                name: BOUND_RECEIVER_CAPTURE.to_string(),
                ty: object_ty.clone(),
                mode,
                span: object.span,
                mutated,
            }],
            call_kind,
        };
        let ty = info.ty();
        self.closure_infos.borrow_mut().insert(id, info);
        Ok(Some(ty))
    }

    /// True when `field` names a receiver method (inherent or trait) of
    /// `object_ty`, so an index after it spells method type arguments rather
    /// than a runtime index.
    pub(super) fn member_names_receiver_method(&self, object_ty: &Type, field: &str) -> bool {
        let Type::Named(type_name, _) = object_ty else {
            return false;
        };
        if let Some(class) = self.resolve_class_info(type_name) {
            if class.fields.contains_key(field) {
                return false;
            }
            if let Some(method) = class.methods.get(field) {
                return method.decl.receiver.is_some();
            }
        }
        self.trait_method_for_concrete_type(object_ty, field, crate::diag::Span::new(0, 0))
            .ok()
            .flatten()
            .is_some_and(|(_, method, _)| method.decl.receiver.is_some())
    }

    fn bound_method_substitutions(
        &self,
        target: &BoundMethodTarget<'_>,
        field: &str,
        expected: Option<&Type>,
        explicit_type_args: Option<&[Type]>,
        span: crate::diag::Span,
    ) -> Result<HashMap<String, Type>> {
        let mut substitutions = target.substitutions.clone();
        if target.decl.type_params.is_empty() {
            return Ok(substitutions);
        }
        let display = &target.display;
        if let Some(explicit) = explicit_type_args {
            if explicit.len() != target.decl.type_params.len() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "method `{display}` expects {} type argument{}, found {}",
                        target.decl.type_params.len(),
                        if target.decl.type_params.len() == 1 {
                            ""
                        } else {
                            "s"
                        },
                        explicit.len()
                    ),
                ));
            }
            substitutions.extend(substitutions_from_decl_type_args(
                &target.decl.type_params,
                explicit,
            ));
        } else {
            let expected_contract = match expected {
                Some(Type::Function {
                    params,
                    return_type,
                }) => Some((params.as_slice(), return_type.as_ref())),
                Some(Type::Closure {
                    params,
                    return_type,
                    ..
                }) => Some((params.as_slice(), return_type.as_ref())),
                _ => None,
            };
            let Some((expected_params, expected_return)) = expected_contract else {
                return Err(Diagnostic::coded_at(
                    "AU2005",
                    span,
                    format!(
                        "generic method `{display}` needs explicit type arguments or an expected callable contract to become a method value; write `.{field}[...]`, or call `.{field}(...)` directly"
                    ),
                ));
            };
            if expected_params.len() != target.signature.params.len() {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    span,
                    format!(
                        "cannot specialize method `{display}`: expected {} parameter{}, found {}",
                        target.signature.params.len(),
                        if target.signature.params.len() == 1 {
                            ""
                        } else {
                            "s"
                        },
                        expected_params.len()
                    ),
                ));
            }
            for (pattern, actual) in target.signature.params.iter().zip(expected_params) {
                unify_type_pattern(
                    &substitute_type(pattern, &target.substitutions),
                    &actual.ty,
                    &mut substitutions,
                )
                .map_err(|error| {
                    Diagnostic::coded_at(
                        "AU2002",
                        span,
                        format!("cannot specialize method `{display}`: {}", error.message),
                    )
                })?;
            }
            unify_type_pattern(
                &substitute_type(&target.signature.return_type, &target.substitutions),
                expected_return,
                &mut substitutions,
            )
            .map_err(|error| {
                Diagnostic::coded_at(
                    "AU2002",
                    span,
                    format!("cannot specialize method `{display}`: {}", error.message),
                )
            })?;
        }
        for type_param in &target.decl.type_params {
            let Some(resolved) = substitutions.get(type_param) else {
                return Err(Diagnostic::at(
                    span,
                    format!("cannot infer type parameter `{type_param}` for method `{display}`"),
                ));
            };
            let mut bounds = target
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
            display,
            &target.signature.rng_clone_safe_type_params,
            &substitutions,
            span,
        )?;
        Ok(substitutions)
    }

    fn acquire_bound_receiver(
        &self,
        object: &Expr,
        display: &str,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        match &object.kind {
            ExprKind::Group(inner) => self.acquire_bound_receiver(inner, display, locals),
            ExprKind::Name(name) => {
                let Some(binding) = locals.get(name) else {
                    return self.consume_value_expr(object, locals);
                };
                if binding.view.is_some() {
                    return Err(Diagnostic::coded_at(
                        "AU3004",
                        object.span,
                        format!(
                            "bound method `{display}` cannot take ownership of the pointee of view `{name}`"
                        ),
                    )
                    .with_help(format!(
                        "clone `{name}` into an owned local first, then bind the method on that local"
                    )));
                }
                if binding.passing != ReceiverKind::Value {
                    let access = if binding.passing == ReceiverKind::BorrowMut {
                        "mutable"
                    } else {
                        "shared"
                    };
                    let noun = if self.implicit_borrowed_params.contains_key(name) {
                        "parameter"
                    } else {
                        "value"
                    };
                    let mut diagnostic = Diagnostic::coded_at(
                        "AU3002",
                        object.span,
                        format!(
                            "bound method `{display}` cannot take {access} {noun} `{name}` by value"
                        ),
                    );
                    if let Some(origin) = binding.borrowed_at {
                        diagnostic = diagnostic.with_secondary(
                            origin,
                            format!("{access} {noun} `{name}` is declared here"),
                        );
                    }
                    return Err(diagnostic.with_help(format!(
                        "clone `{name}` into an owned local before binding the method, or declare the enclosing parameter as `own {}`",
                        binding.ty
                    )));
                }
                self.consume_binding(name, object.span, locals)
            }
            _ => self.consume_value_expr(object, locals),
        }
    }
}
